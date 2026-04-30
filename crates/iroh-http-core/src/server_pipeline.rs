//! Per-connection tower pipeline assembly for [`crate::server`].
//!
//! Closes the recommendation §5.1 of `reviews/2026-04-30-post-rework-review.md`
//! (issue #169): the layer stack — compression, decompression, body limit,
//! load-shed, timeout, layer-error handling — used to be assembled inline
//! inside the accept loop with `` branches
//! duplicated across the assembly. That logic now lives here as
//! [`serve_bistream`], called once per accepted QUIC bi-stream from
//! `server.rs`. The accept loop in `server.rs` is left with only the
//! lifecycle plumbing (counters, drop guards) it needs to own.
//!
//! Layer ordering (outermost first):
//!
//! ```text
//! [CompressionLayer →] [RequestDecompressionLayer →] HandleLayerError
//!   → [LoadShed →] Timeout → [BodyLimit →] svc
//! ```
//!
//! * `BodyLimit` and `LoadShed` are runtime-optional via `option_layer`.
//! * `CompressionLayer` and `RequestDecompressionLayer` are gated on the
//!   `compression` cargo feature.
//! * The compression predicate composition lives in
//!   [`build_compression_layer`] — uses `tower-http`'s built-in
//!   [`NotForContentType`](tower_http::compression::predicate::NotForContentType)
//!   predicates wherever possible (closes #172).

use std::time::Duration;

use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tower::{timeout::TimeoutLayer, ServiceBuilder};

use crate::io::IrohStream;
use crate::Body;

use crate::CompressionOptions;

/// Runtime knobs collected from [`crate::server::ServeOptions`] and the
/// owning endpoint, in the shape the pipeline needs.
#[derive(Clone)]
pub(crate) struct PipelineParams {
    /// Per-request timeout. `Duration::MAX` disables it.
    pub timeout: Duration,
    /// Maximum decoded request body size before the layer rejects with 413.
    pub max_request_body_bytes: Option<usize>,
    /// `true` ⇒ wrap with `LoadShedLayer` so saturated capacity returns 503
    /// immediately rather than blocking the caller.
    pub load_shed_enabled: bool,
    /// Effective hyper header limit (already clamped to the 8192 floor).
    pub effective_header_limit: usize,
    /// Operator's compression configuration. `None` ⇒ no compression on the
    /// response side. Decompression on the request side is always-on.
    pub compression: Option<CompressionOptions>,
}

/// Build the per-bistream tower pipeline and drive a single hyper HTTP/1.1
/// connection over `io` with the resulting service.
///
/// Returns the future. The accept loop spawns it; this function neither
/// spawns nor logs any lifecycle event of its own (drop guards live in the
/// caller's task). Connection-level errors are logged at `debug!` level
/// because most of them are routine end-of-life conditions on a P2P
/// transport (peer reset, idle timeout, decoder rejection of malformed
/// inbound bytes) — promoting them to `warn` would flood operators with
/// noise. Layer-side service errors that *do* warrant escalation are
/// surfaced via `HandleLayerErrorLayer`, which converts them into
/// structured HTTP responses before they ever reach this point.
///
/// `svc` is a fully type-erased [`ServeService`] (a [`BoxCloneService`]).
/// The accept loop boxes the per-connection stack \u2014 [`IrohHttpService`]
/// wrapped with `ConcurrencyLimitLayer` and any other operator layers \u2014
/// once at construction time, so adding a new layer in [`crate::server`]
/// does not change the signature seen here.
///
/// [`BoxCloneService`]: tower::util::BoxCloneService
/// [`IrohHttpService`]: crate::server::IrohHttpService
/// Type-erased per-connection service handed to [`serve_bistream`].
///
/// Boxing once at the construction site (in `server::serve_with_events`)
/// means every future addition to the layer stack \u2014 `AddExtensionLayer`,
/// `TraceLayer`, a response-signing layer, anything \u2014 is **one append**
/// in the builder without rippling a new concrete type signature through
/// this module and the accept loop. This is the structural payoff of #175 /
/// ADR-014 D2: the recurring "tower body type soup" was a symptom of a
/// non-erased inner type. With the body normalised to [`Body`] at every
/// seam (ADR-014 D2) and the error fixed at [`Infallible`], the box
/// compiles cleanly.
pub(crate) type ServeService = tower::util::BoxCloneService<
    hyper::Request<Body>,
    hyper::Response<Body>,
    std::convert::Infallible,
>;

pub(crate) async fn serve_bistream(
    io: TokioIo<IrohStream>,
    svc: ServeService,
    params: PipelineParams,
) {
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder
        .max_buf_size(params.effective_header_limit)
        .max_headers(128);

    // ADR-014 D2 / #175 — body normalisation at the hyper seam.
    //
    // hyper hands `Request<Incoming>`; the rest of the chain (built by
    // `build_stack`) is uniformly `Service<Request<Body>>`. The single
    // `MapRequestBodyLayer` here is the only place that names
    // `hyper::body::Incoming` — every layer downstream sees `Body`.
    // Same shape as `axum::serve`'s
    // `make_service.call(...).map_request(|r| r.map(Body::new))`.
    use tower_http::map_request_body::MapRequestBodyLayer;
    let from_incoming = MapRequestBodyLayer::new(|b: hyper::body::Incoming| Body::new(b));

    let inner = build_stack(svc, &params, tower::layer::util::Identity::new());
    let stack = ServiceBuilder::new().layer(from_incoming).service(inner);

    let result = builder
        .serve_connection(io, TowerToHyperService::new(stack))
        .with_upgrades()
        .await;

    if let Err(e) = result {
        tracing::debug!("iroh-http: http1 connection error: {e}");
    }
}

/// Compose the per-connection tower stack from operator knobs.
///
/// This is the entire per-bistream pipeline expressed once:
///
/// ```text
///   request body limit  →  HandleLayerError  →  load shed
///                       →  request timeout   →  compression (response)
///                       →  decompression (request, always-on)
///                       →  `extra` (test seam: Identity in production)
///                       →  IrohHttpService
/// ```
///
/// `extra` is a layer-shaped seam that production fills with
/// [`tower::layer::util::Identity`] and tests fill with a no-op
/// `MapRequestBodyLayer`. Its only purpose is to let the guardrail in
/// `tests` exercise this exact function — so any future structural change
/// (a body type that stops being `Body`, an error that stops being
/// `Infallible`, an unnameable inferred type that breaks `BoxCloneService`)
/// fails to compile here, not at integration-test time over the network.
///
/// Returns the fully-erased [`ServeService`] so the call site does not have
/// to name the inner `Either<…, …>` chain produced by the `option_layer`s.
pub(crate) fn build_stack<L>(svc: ServeService, params: &PipelineParams, extra: L) -> ServeService
where
    L: tower::Layer<ServeService>,
    L::Service: tower::Service<
            hyper::Request<Body>,
            Response = hyper::Response<Body>,
            Error = std::convert::Infallible,
        > + Clone
        + Send
        + 'static,
    <L::Service as tower::Service<hyper::Request<Body>>>::Future: Send + 'static,
{
    use crate::server::HandleLayerErrorLayer;
    use tower::ServiceExt;
    use tower_http::map_request_body::MapRequestBodyLayer;
    use tower_http::map_response_body::MapResponseBodyLayer;

    // ADR-013: enforce request body size with the standard tower-http
    // layer rather than a hand-rolled byte counter in the dispatcher.
    // `RequestBodyLimitLayer` wraps the request body to `Limited<B>` and
    // changes the response body to `ResponseBody<S::ResBody>`. Per
    // ADR-014 D2 / #175 we renormalise both directions back to `Body` so
    // the inner service stays `Service<Request<Body>, Response = Response<Body>>`.
    let body_limit_layer = params.max_request_body_bytes.map(|limit| {
        ServiceBuilder::new()
            .layer(MapResponseBodyLayer::new(
                |b: tower_http::limit::ResponseBody<Body>| Body::new(b),
            ))
            .layer(tower_http::limit::RequestBodyLimitLayer::new(limit))
            .layer(MapRequestBodyLayer::new(
                |b: tower_http::body::Limited<Body>| Body::new(b),
            ))
            .into_inner()
    });

    let load_shed_layer = params
        .load_shed_enabled
        .then(tower::load_shed::LoadShedLayer::new);

    // Compression bundle: `CompressionLayer` wraps the response body
    // to `CompressionBody<Body>`; the outer `MapResponseBodyLayer`
    // renormalises it back to `Body` so the `option_layer` arms unify.
    // `params.compression` is `None` when the operator opted out at
    // runtime; in that case `CompressionLayer` is omitted entirely.
    let comp_layer = params
        .compression
        .as_ref()
        .map(build_compression_layer)
        .map(|comp| {
            ServiceBuilder::new()
                .layer(MapResponseBodyLayer::new(
                    |b: tower_http::compression::CompressionBody<Body>| Body::new(b),
                ))
                .layer(comp)
                .into_inner()
        });

    // Decompression bundle: `RequestDecompressionLayer` wraps the request
    // body to `DecompressionBody<Body>` (renormalised on the way in) and
    // unifies its response body to `tower_http::body::UnsyncBoxBody<Bytes,
    // BoxError>` (renormalised on the way out). The response renormaliser
    // uses a bare `fn` item (not a closure) so its signature exactly
    // matches what tower-http's higher-ranked body type requires.
    // Always-on: every server accepts compressed requests, even when it
    // does not send compressed responses.
    fn box_to_body(
        b: tower_http::body::UnsyncBoxBody<bytes::Bytes, crate::body::BoxError>,
    ) -> Body {
        Body::new(b)
    }
    let decomp_layer = {
        use tower_http::decompression::{DecompressionBody, RequestDecompressionLayer};
        ServiceBuilder::new()
            .layer(MapResponseBodyLayer::new(box_to_body))
            .layer(RequestDecompressionLayer::new())
            .layer(MapRequestBodyLayer::new(|b: DecompressionBody<Body>| {
                Body::new(b)
            }))
            .into_inner()
    };

    ServiceBuilder::new()
        .option_layer(body_limit_layer)
        .layer(HandleLayerErrorLayer)
        .option_layer(load_shed_layer)
        .layer(TimeoutLayer::new(params.timeout))
        .option_layer(comp_layer)
        .layer(decomp_layer)
        .layer(extra)
        .service(svc)
        .boxed_clone()
}

/// Build the `tower-http` compression layer with the project's predicate set.
///
/// Two custom predicates remain because tower-http does not ship built-ins
/// for either:
///
/// 1. Skip if the response already carries `Content-Encoding` (handler
///    returned a pre-encoded body — re-compressing would double-encode).
/// 2. Honour `Cache-Control: no-transform` per RFC 9111 §5.2.2.7.
fn build_compression_layer(
    comp: &CompressionOptions,
) -> tower_http::compression::CompressionLayer<impl tower_http::compression::Predicate> {
    use http::{Extensions, HeaderMap, StatusCode, Version};
    use tower_http::compression::{
        predicate::{NotForContentType, Predicate, SizeAbove},
        CompressionLayer, CompressionLevel,
    };

    let mut layer = CompressionLayer::new().zstd(true);
    if let Some(level) = comp.level {
        layer = layer.quality(CompressionLevel::Precise(level as i32));
    }

    let not_pre_compressed = |_: StatusCode, _: Version, h: &HeaderMap, _: &Extensions| {
        !h.contains_key(http::header::CONTENT_ENCODING)
    };
    let not_no_transform = |_: StatusCode, _: Version, h: &HeaderMap, _: &Extensions| {
        h.get(http::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                !v.split(',')
                    .any(|d| d.trim().eq_ignore_ascii_case("no-transform"))
            })
            .unwrap_or(true)
    };

    let predicate = SizeAbove::new(comp.min_body_bytes.min(u16::MAX as usize) as u16)
        .and(NotForContentType::IMAGES)
        .and(NotForContentType::SSE)
        .and(NotForContentType::const_new("audio/"))
        .and(NotForContentType::const_new("video/"))
        .and(NotForContentType::const_new("application/zstd"))
        .and(NotForContentType::const_new("application/octet-stream"))
        .and(not_pre_compressed)
        .and(not_no_transform);

    layer.compress_when(predicate)
}

#[cfg(test)]
mod tests {
    //! ADR-014 D2 / #175 guardrail.
    //!
    //! Exercises the *real* per-bistream chain (`build_stack`) to prove the
    //! structural property the issue was filed for: every layer in the
    //! production pipeline composes uniformly into [`ServeService`], and a
    //! request still flows through to a `Response<Body>`. If a future
    //! change to `IrohHttpService`'s body or error type breaks uniformity,
    //! `build_stack` itself stops compiling — the failure surfaces here,
    //! not later in an integration test over the network.
    //!
    //! No hyper / no Iroh / no networking: feeds requests directly into
    //! the boxed [`ServeService`] with `tower::ServiceExt::oneshot`.

    use super::*;
    use bytes::Bytes;
    use http_body_util::BodyExt;
    use std::convert::Infallible;
    use tower::ServiceExt;

    /// Stand-in for `IrohHttpService` shaped exactly like the real one
    /// (`Service<Request<Body>, Response = Response<Body>, Error = Infallible>`).
    /// Echoes the request body back in the response.
    #[derive(Clone)]
    struct EchoService;

    impl tower::Service<hyper::Request<Body>> for EchoService {
        type Response = hyper::Response<Body>;
        type Error = Infallible;
        type Future = std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
        >;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: hyper::Request<Body>) -> Self::Future {
            Box::pin(async move {
                let bytes = req
                    .into_body()
                    .collect()
                    .await
                    .map(|c| c.to_bytes())
                    .unwrap_or_default();
                Ok(hyper::Response::new(Body::full(bytes)))
            })
        }
    }

    fn default_params() -> PipelineParams {
        PipelineParams {
            timeout: Duration::MAX,
            max_request_body_bytes: Some(1024 * 1024),
            load_shed_enabled: true,
            effective_header_limit: 8192,
            compression: None,
        }
    }

    fn boxed_echo() -> ServeService {
        use tower::ServiceBuilder;
        ServiceBuilder::new().service(EchoService).boxed_clone()
    }

    /// Drives the *production* `build_stack` with a no-op `extra` layer.
    /// The body limit, load-shed, timeout and decompression layers are all
    /// in-circuit; only compression is gated off (per `params.compression`).
    /// If any of them stops composing into `ServeService`, this fails to
    /// compile — that is the structural guardrail.
    #[tokio::test]
    async fn real_chain_round_trips_a_request() {
        let extra = tower_http::map_request_body::MapRequestBodyLayer::new(|b: Body| b);
        let stack = build_stack(boxed_echo(), &default_params(), extra);

        let req = hyper::Request::builder()
            .uri("/")
            .body(Body::full("ping"))
            .unwrap();
        let resp = stack.oneshot(req).await.expect("service infallible");
        assert_eq!(resp.status(), hyper::StatusCode::OK);
        let body = resp
            .into_body()
            .collect()
            .await
            .expect("body collect")
            .to_bytes();
        assert_eq!(body, Bytes::from_static(b"ping"));
    }

    /// Same chain, with response-side compression enabled. Proves the
    /// `option_layer(comp_layer)` arm composes and produces the same
    /// `ServeService` shape.
    #[tokio::test]
    async fn real_chain_with_compression_enabled_still_round_trips() {
        let mut params = default_params();
        params.compression = Some(crate::CompressionOptions {
            level: None,
            min_body_bytes: 0,
        });
        let extra = tower::layer::util::Identity::new();
        let stack = build_stack(boxed_echo(), &params, extra);

        let req = hyper::Request::builder()
            .uri("/")
            .header("accept-encoding", "zstd")
            .body(Body::full("ping"))
            .unwrap();
        let resp = stack.oneshot(req).await.expect("service infallible");
        assert_eq!(resp.status(), hyper::StatusCode::OK);
        // Body is opaque (may be zstd-encoded). We only assert structural
        // success; wire-format coverage lives in the dedicated compression
        // integration tests.
        let _ = resp.into_body().collect().await;
    }
}
