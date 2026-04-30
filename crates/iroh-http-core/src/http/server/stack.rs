//! Tower stack composition shared by [`crate::http::serve`] (server) and
//! [`crate::http::client::fetch`] (client).
//!
//! Closes Slice B of #182.
//!
//! Before this slice the per-connection pipeline was assembled by
//! [`super::pipeline::build_stack`] from a hand-coded
//! `PipelineParams` struct, while the outgoing-fetch pipeline was assembled
//! inline in `client.rs` from a one-off `HyperClientSvc` wrapper.
//! Compression on serve and decompression on fetch composed with different
//! code paths to do structurally the same thing.
//!
//! After this slice there is exactly one composition function per direction
//! ([`build_stack`] for inbound, [`build_client_stack`] for outbound),
//! both consume the same typed [`StackConfig`], and every middleware is
//! driven by an [`Option`]/`bool` field through `option_layer` rather than
//! a `cfg!` macro. Toggling `cfg.compression = None` produces a service
//! whose runtime shape is identical to the layer never having been built.
//!
//! Layer ordering (outermost first), inbound:
//!
//! ```text
//! [body limit →] HandleLayerError → [load shed →] [timeout →]
//!   [compression →] [decompression →] svc
//! ```
//!
//! Layer ordering (outermost first), outbound:
//!
//! ```text
//! [decompression →] hyper SendRequest
//! ```
//!
//! ## Why two functions, one config
//!
//! Server and client use the same primitive layers from `tower-http`, but
//! in opposite roles: a server *responds* compressed (compression layer)
//! and *accepts* compressed requests (decompression layer); a client *sends*
//! plain and *accepts* compressed responses (decompression layer). A single
//! [`StackConfig`] captures the operator's intent; the direction-specific
//! function decides which subset to apply.
//!
//! Slice D (#186) replaces [`build_client_stack`]'s ad-hoc hyper wrapper
//! with a proper `Service<Request<Body>, Response<Body>, FetchError>`
//! contract; until then, the wrapper lives here so the construction site
//! is unified even if the inner type still spells `hyper::Error`.

use std::convert::Infallible;
use std::time::Duration;

use bytes::Bytes;
use tower::ServiceBuilder;

use crate::http::body::BoxError;
use crate::Body;

// ── Public-facing config type ────────────────────────────────────────────────

/// Compression options for response bodies (server) and outgoing requests
/// (client; reserved).
///
/// Lives in this module because it configures the compression layer in
/// [`build_stack`]/[`build_client_stack`]. Re-exported from the crate root
/// so adapter callsites are unchanged.
#[derive(Debug, Clone)]
pub struct CompressionOptions {
    /// Minimum body size in bytes before compression is applied.
    /// Default: [`CompressionOptions::DEFAULT_MIN_BODY_BYTES`] (1 KiB).
    pub min_body_bytes: usize,
    /// Zstd compression level (1–22). `None` uses the zstd default (3).
    pub level: Option<u32>,
}

impl CompressionOptions {
    /// Default minimum body size before compression is applied.
    ///
    /// 1 KiB. Matches the documented default and the threshold most HTTP
    /// servers tune to: below ~1 KiB the CPU cost of compression typically
    /// outweighs the bandwidth savings on a single TCP/QUIC packet.
    pub const DEFAULT_MIN_BODY_BYTES: usize = 1024;
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            min_body_bytes: Self::DEFAULT_MIN_BODY_BYTES,
            level: None,
        }
    }
}

// ── Internal stack types ─────────────────────────────────────────────────────

/// Server-side stack alias: type-erased `Service<Request<Body>>` returning
/// `Response<Body>` with [`Infallible`] errors.
///
/// Boxing once at the construction site (in `server::serve_with_events`)
/// means every future addition to the layer stack — `AddExtensionLayer`,
/// `TraceLayer`, a response-signing layer, anything — is **one append**
/// in the builder without rippling a new concrete type signature through
/// the per-bistream code path.
pub(crate) type ServeService =
    tower::util::BoxCloneService<hyper::Request<Body>, hyper::Response<Body>, Infallible>;

/// Client-side stack alias.
///
/// Carries `hyper::Error` (Slice D will replace this with a typed
/// `FetchError`). Boxed via [`tower::util::BoxService`] rather than
/// [`tower::util::BoxCloneService`] because hyper's `SendRequest` is not
/// `Clone`; the boxed service is consumed by a single `oneshot` call.
pub(crate) type ClientService =
    tower::util::BoxService<hyper::Request<Body>, hyper::Response<Body>, hyper::Error>;

/// Tower stack configuration shared by [`build_stack`] and
/// [`build_client_stack`].
///
/// Compression, body-limit and load-shed are toggled at runtime via
/// `option_layer` (zero overhead when `None`/`false`). Timeout uses
/// `Duration::MAX` as the disabled sentinel: it always composes through
/// the same `TimeoutLayer` arm, so the future type stays uniform across
/// configurations and the resulting `Either` chain satisfies
/// `Service<Request<Body>>` (see ADR-014 D2 stop-signal).
///
/// Decompression on the server, and response decompression on the client,
/// are not exposed as toggles here: the layer's `Service::Future` type is
/// not uniform with the no-op arm, so an `option_layer` would fail to
/// compose into a single boxed service. Both directions remain always-on
/// in this slice; if a real opt-out becomes needed, the implementation is
/// to branch [`build_stack`] into two pre-boxed flavours rather than
/// fight `Either`'s `Service` bound.
#[derive(Clone, Debug, Default)]
pub(crate) struct StackConfig {
    /// Per-request timeout. `None` is treated as `Duration::MAX` (no
    /// effective deadline); the `TimeoutLayer` itself is always applied so
    /// the inner `Future` type is invariant under config changes.
    pub timeout: Option<Duration>,
    /// Maximum decoded request body size before the server rejects with 413.
    pub max_request_body_bytes: Option<usize>,
    /// `true` ⇒ wrap with `LoadShedLayer` so saturated capacity returns 503
    /// immediately rather than blocking the caller.
    pub load_shed: bool,
    /// Operator's compression configuration. `None` disables response
    /// compression on the server side; ignored by [`build_client_stack`]
    /// (clients do not yet compress request bodies).
    pub compression: Option<CompressionOptions>,
}

// ── Server pipeline ──────────────────────────────────────────────────────────

/// Compose the inbound per-connection tower stack.
///
/// ```text
///   request body limit  →  HandleLayerError  →  load shed
///                       →  request timeout   →  compression (response)
///                       →  decompression (request)
///                       →  svc
/// ```
///
/// Returns a fully-erased [`ServeService`] so the caller does not have to
/// name the inner `Either<…, …>` chain produced by the `option_layer`s.
/// `svc` is itself a [`ServeService`] — the per-connection wrapping
/// (concurrency limiter + `IrohHttpService`) is applied upstream in
/// `serve_with_events` before this function is called, so this signature is
/// stable across changes to the inner service.
pub(crate) fn build_stack(svc: ServeService, cfg: &StackConfig) -> ServeService {
    use crate::http::server::HandleLayerErrorLayer;
    use tower::ServiceExt;
    use tower_http::map_request_body::MapRequestBodyLayer;
    use tower_http::map_response_body::MapResponseBodyLayer;

    // ADR-013: enforce request body size with the standard tower-http
    // layer rather than a hand-rolled byte counter in the dispatcher.
    // `RequestBodyLimitLayer` wraps the request body to `Limited<B>` and
    // changes the response body to `ResponseBody<S::ResBody>`. Per
    // ADR-014 D2 / #175 we renormalise both directions back to `Body` so
    // the inner service stays `Service<Request<Body>, Response = Response<Body>>`.
    let body_limit_layer = cfg.max_request_body_bytes.map(|limit| {
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

    let load_shed_layer = cfg.load_shed.then(tower::load_shed::LoadShedLayer::new);

    let timeout = cfg.timeout.unwrap_or(Duration::MAX);
    let timeout_layer = tower::timeout::TimeoutLayer::new(timeout);

    // Compression bundle: `CompressionLayer` wraps the response body
    // to `CompressionBody<Body>`; the outer `MapResponseBodyLayer`
    // renormalises it back to `Body` so the `option_layer` arms unify.
    let comp_layer = cfg
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
    // BoxError>` (renormalised on the way out). Always-on: every server
    // accepts compressed requests, even when it does not send compressed
    // responses. See [`StackConfig`] doc for why this is not toggleable.
    //
    // The bundle is inlined here rather than factored into a sibling
    // `build_decompression_layer()` because returning the assembled
    // `Stack<MapRequestBody<RequestDecompression<MapResponseBody<...>>>>`
    // through an `impl Layer<ServeService>` erases the `Service::Future:
    // Send + 'static` bound that `boxed_clone` later needs. Spelling the
    // concrete `Stack<...>` type would be a 5-line type alias that has to
    // be edited every time tower-http changes a body wrapper. Inline form
    // keeps the layer ordering visible in one place; mirror this when the
    // client side eventually grows real layers in Slice D.
    fn box_to_body(b: tower_http::body::UnsyncBoxBody<Bytes, BoxError>) -> Body {
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
        .layer(timeout_layer)
        .option_layer(comp_layer)
        .layer(decomp_layer)
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
pub(crate) fn build_compression_layer(
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

// ── Client pipeline ──────────────────────────────────────────────────────────

/// Wraps `SendRequest<Body>` as a `tower::Service` so compression/decompression
/// layers from `tower-http` can be composed around it.
///
/// Slice D (#186) deletes this wrapper outright in favour of a pure-Rust
/// `http::fetch` returning `Response<Body>` with a typed `FetchError`.
struct HyperClientSvc(hyper::client::conn::http1::SendRequest<Body>);

impl tower::Service<hyper::Request<Body>> for HyperClientSvc {
    type Response = hyper::Response<hyper::body::Incoming>;
    type Error = hyper::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: hyper::Request<Body>) -> Self::Future {
        Box::pin(self.0.send_request(req))
    }
}

/// Compose the outbound per-request tower stack.
///
/// ```text
///   decompression → incoming→Body → hyper SendRequest
/// ```
///
/// Returns a [`ClientService`] — boxed once so the caller (`fetch`) does
/// not have to spell the inner type. Currently honours none of
/// [`StackConfig`]'s fields directly; the body-renormalisation +
/// always-on decompression match the server's policy. Slice D introduces
/// client-side timeout / body-limit / typed `FetchError` in the same
/// shape.
pub(crate) fn build_client_stack(
    sender: hyper::client::conn::http1::SendRequest<Body>,
    _cfg: &StackConfig,
) -> ClientService {
    use tower::ServiceExt;
    use tower_http::decompression::{DecompressionBody, DecompressionLayer};
    use tower_http::map_response_body::MapResponseBodyLayer;

    ServiceBuilder::new()
        .layer(MapResponseBodyLayer::new(
            |b: DecompressionBody<hyper::body::Incoming>| Body::new(b),
        ))
        .layer(DecompressionLayer::new())
        .service(HyperClientSvc(sender))
        .boxed()
}

#[cfg(test)]
mod tests {
    //! ADR-014 D2 / #175 + Slice B (#184) guardrail.
    //!
    //! Exercises the *real* per-bistream chain ([`build_stack`]) to prove
    //! the structural property the issue was filed for: every layer in the
    //! production pipeline composes uniformly into [`ServeService`], and a
    //! request still flows through to a `Response<Body>`. If a future
    //! change to the inner service's body or error type breaks uniformity,
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

    fn default_cfg() -> StackConfig {
        StackConfig {
            timeout: None,
            max_request_body_bytes: Some(1024 * 1024),
            load_shed: true,
            compression: None,
        }
    }

    fn boxed_echo() -> ServeService {
        ServiceBuilder::new().service(EchoService).boxed_clone()
    }

    /// Drives the *production* `build_stack`. Body limit, load-shed and
    /// decompression are all in-circuit. If any of them stops composing
    /// into `ServeService`, this fails to compile — that is the structural
    /// guardrail.
    #[tokio::test]
    async fn real_chain_round_trips_a_request() {
        let stack = build_stack(boxed_echo(), &default_cfg());

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
        let mut cfg = default_cfg();
        cfg.compression = Some(CompressionOptions {
            level: None,
            min_body_bytes: 0,
        });
        let stack = build_stack(boxed_echo(), &cfg);

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

    /// #184 acceptance criterion 5 — extensibility regression test.
    ///
    /// Wraps the production `build_stack` output with one additional
    /// `MapRequestBodyLayer::new(identity)` and asserts the request still
    /// flows. Demonstrates ADR-014 D2's structural payoff at runtime: a
    /// new layer is one append, no signature change anywhere downstream
    /// because both `build_stack`'s input and output are
    /// [`ServeService`]. If a future layer addition breaks the
    /// "uniformly composes into `ServeService`" property this test fails
    /// to compile.
    #[tokio::test]
    async fn build_stack_accepts_additional_outer_layer() {
        use tower_http::map_request_body::MapRequestBodyLayer;

        let inner = build_stack(boxed_echo(), &default_cfg());
        let stack = ServiceBuilder::new()
            .layer(MapRequestBodyLayer::new(|b: Body| b))
            .service(inner);

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
}
