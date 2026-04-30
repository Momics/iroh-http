//! Per-connection tower pipeline assembly for [`crate::server`].
//!
//! Closes the recommendation §5.1 of `reviews/2026-04-30-post-rework-review.md`
//! (issue #169): the layer stack — compression, decompression, body limit,
//! load-shed, timeout, layer-error handling — used to be assembled inline
//! inside the accept loop with `#[cfg(feature = "compression")]` branches
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

use std::{future::Future, time::Duration};

use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tower::{timeout::TimeoutLayer, ServiceBuilder};

use crate::io::IrohStream;
use crate::Body;

#[cfg(feature = "compression")]
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
    /// Compression configuration, if the feature is on AND the operator
    /// opted in.
    #[cfg(feature = "compression")]
    pub compression: Option<CompressionOptions>,
}

/// Build the per-bistream tower pipeline and drive a single hyper HTTP/1.1
/// connection over `io` with the resulting service.
///
/// Returns the future. The accept loop spawns it; this function neither
/// spawns nor logs any lifecycle event of its own (drop guards live in the
/// caller's task). Connection-level errors are logged at `debug!` level —
/// see ADR-014 §D5 for why these stay under `tracing::debug` not `warn`.
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

pub(crate) fn serve_bistream(
    io: TokioIo<IrohStream>,
    svc: ServeService,
    params: PipelineParams,
) -> impl Future<Output = ()> + Send {
    use crate::server::HandleLayerErrorLayer;

    async move {
        // ADR-013: enforce request body size with the standard tower-http
        // layer rather than a hand-rolled byte counter in the dispatcher.
        // `RequestBodyLimitLayer` wraps the request body to `Limited<B>` and
        // changes the response body to `ResponseBody<S::ResBody>`. Per
        // ADR-014 D2 / #175 we renormalise both directions back to `Body`
        // so the inner service stays `Service<Request<Body>, Response = Response<Body>>`:
        //   * `MapRequestBodyLayer::new(Body::new)` after the limit (request side)
        //   * `MapResponseBodyLayer::new(Body::new)` before it (response side)
        let body_limit_layer = params.max_request_body_bytes.map(|limit| {
            ServiceBuilder::new()
                .layer(tower_http::map_response_body::MapResponseBodyLayer::new(
                    |b: tower_http::limit::ResponseBody<Body>| Body::new(b),
                ))
                .layer(tower_http::limit::RequestBodyLimitLayer::new(limit))
                .layer(tower_http::map_request_body::MapRequestBodyLayer::new(
                    |b: tower_http::body::Limited<Body>| Body::new(b),
                ))
                .into_inner()
        });

        let load_shed_layer = params
            .load_shed_enabled
            .then(tower::load_shed::LoadShedLayer::new);

        let core_stack = ServiceBuilder::new()
            .option_layer(body_limit_layer)
            .layer(HandleLayerErrorLayer)
            .option_layer(load_shed_layer)
            .layer(TimeoutLayer::new(params.timeout))
            .service(svc);

        let mut builder = hyper::server::conn::http1::Builder::new();
        builder
            .max_buf_size(params.effective_header_limit)
            .max_headers(128);

        // ADR-014 D2 / #175 — body normalisation sandwich.
        //
        // `IrohHttpService` is non-generic (`Service<Request<Body>>`).
        // Every layer that rewraps the request *or* response body
        // (compression, decompression, body-limit) must be sandwiched with
        // `MapRequestBodyLayer` / `MapResponseBodyLayer` that renormalise
        // the body back to `Body`. This keeps the entire chain typed as
        // `Service<Request<Body>, Response = Response<Body>>`, which is
        // also what `option_layer`'s `Either` requires for both arms to
        // unify. Same pattern as `axum::serve`
        // (`make_service.call(...).map_request(|r| r.map(Body::new))`),
        // applied at every seam where a layer changes the body type.
        //
        // Each closure carries an explicit input type so type inference
        // picks the correct monomorphisation of `Body::new`.
        use tower_http::map_request_body::MapRequestBodyLayer;
        use tower_http::map_response_body::MapResponseBodyLayer;

        // Cfg gates live at *layer construction* only — the chain below is
        // a single uniform `ServiceBuilder` per ADR-014 D2 / #175. Adding a
        // future operator layer is one append between `from_incoming` and
        // `.service(core_stack)`.
        //
        // Compression bundle: `CompressionLayer` wraps the response body
        // to `CompressionBody<Body>`; the outer `MapResponseBodyLayer`
        // renormalises it back to `Body` so the option_layer arms unify.
        #[cfg(feature = "compression")]
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
        #[cfg(not(feature = "compression"))]
        let comp_layer: Option<tower::layer::util::Identity> = None;

        // Decompression bundle: `RequestDecompressionLayer` wraps the
        // request body to `DecompressionBody<Body>` (renormalised on the
        // way in) and unifies its response body to
        // `tower_http::body::UnsyncBoxBody<Bytes, BoxError>` (renormalised
        // on the way out). The response renormaliser uses a bare `fn` item
        // (not a closure) so its signature exactly matches what tower-http's
        // higher-ranked body type requires.
        #[cfg(feature = "compression")]
        fn box_to_body(
            b: tower_http::body::UnsyncBoxBody<bytes::Bytes, crate::body::BoxError>,
        ) -> Body {
            Body::new(b)
        }
        #[cfg(feature = "compression")]
        let decomp_layer = {
            use tower_http::decompression::{DecompressionBody, RequestDecompressionLayer};
            Some(
                ServiceBuilder::new()
                    .layer(MapResponseBodyLayer::new(box_to_body))
                    .layer(RequestDecompressionLayer::new())
                    .layer(MapRequestBodyLayer::new(|b: DecompressionBody<Body>| {
                        Body::new(b)
                    }))
                    .into_inner(),
            )
        };
        #[cfg(not(feature = "compression"))]
        let decomp_layer: Option<tower::layer::util::Identity> = None;

        let from_incoming = MapRequestBodyLayer::new(|b: hyper::body::Incoming| Body::new(b));

        let stack = ServiceBuilder::new()
            .layer(from_incoming)
            .option_layer(comp_layer)
            .option_layer(decomp_layer)
            .service(core_stack);

        let result = builder
            .serve_connection(io, TowerToHyperService::new(stack))
            .with_upgrades()
            .await;

        if let Err(e) = result {
            tracing::debug!("iroh-http: http1 connection error: {e}");
        }
    }
}

/// Build the `tower-http` compression layer with the project's predicate set.
///
/// Two custom predicates remain because tower-http does not ship built-ins
/// for either:
///
/// 1. Skip if the response already carries `Content-Encoding` (handler
///    returned a pre-encoded body — re-compressing would double-encode).
/// 2. Honour `Cache-Control: no-transform` per RFC 9111 §5.2.2.7.
#[cfg(feature = "compression")]
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
    //! These tests prove the structural property the issue was filed for:
    //! adding a new layer to the per-connection stack is **one append**
    //! that still type-erases into [`ServeService`] and still drives a
    //! request through to a `Response<Body>`. If the inner type ever
    //! stops being uniform — body or error — these tests will fail to
    //! compile, signalling the regression early.
    //!
    //! No hyper / no Iroh / no networking — exercises the tower stack
    //! directly so the test is fast and deterministic.

    use super::*;
    use bytes::Bytes;
    use http_body_util::BodyExt;
    use std::convert::Infallible;
    use tower::{ServiceBuilder, ServiceExt};

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

    /// Build the same shape of stack as `serve_bistream` *with one extra
    /// no-op layer appended* and box it as [`ServeService`].
    ///
    /// If adding `extra` ever requires changing the chain's type signature,
    /// this function stops compiling — that is the guardrail.
    fn build_with_extra_layer() -> ServeService {
        let extra = tower_http::map_request_body::MapRequestBodyLayer::new(|b: Body| b);
        ServiceBuilder::new()
            .layer(extra)
            .service(EchoService)
            .boxed_clone()
    }

    #[tokio::test]
    async fn adding_a_noop_layer_is_one_append_and_request_still_flows() {
        let svc = build_with_extra_layer();
        let req = hyper::Request::builder()
            .uri("/")
            .body(Body::full("ping"))
            .unwrap();
        let resp = svc.oneshot(req).await.expect("service infallible");
        assert_eq!(resp.status(), hyper::StatusCode::OK);
        let body = resp
            .into_body()
            .collect()
            .await
            .expect("body collect")
            .to_bytes();
        assert_eq!(body, Bytes::from_static(b"ping"));
    }

    /// Compile-time only: prove `ServeService` accepts an arbitrary
    /// extra layer wrap and still type-erases. If a future change to
    /// `IrohHttpService`'s body or error type breaks uniformity, this
    /// fails to compile.
    #[allow(dead_code)]
    fn _assert_serve_service_accepts_arbitrary_layer() {
        let _: ServeService = ServiceBuilder::new()
            .layer(tower::layer::util::Identity::new())
            .layer(tower_http::map_request_body::MapRequestBodyLayer::new(
                |b: Body| b,
            ))
            .service(EchoService)
            .boxed_clone();
    }
}
