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
/// `svc` is typically `ConcurrencyLimit<IrohHttpService>` with the
/// per-connection `remote_node_id` populated; the only requirements are
/// the shape of its `Service` impl, which mirrors what hyper expects.
/// Concrete inner service expected by [`serve_bistream`] —
/// `ConcurrencyLimit<IrohHttpService>` with the per-connection
/// `remote_node_id` already populated.
pub(crate) type ServeService = tower::limit::ConcurrencyLimit<crate::server::IrohHttpService>;

pub(crate) fn serve_bistream(
    io: TokioIo<IrohStream>,
    svc: ServeService,
    params: PipelineParams,
) -> impl Future<Output = ()> + Send {
    use crate::server::HandleLayerErrorLayer;

    async move {
        // ADR-013: enforce request body size with the standard tower-http
        // layer rather than a hand-rolled byte counter in the dispatcher.
        // `RequestBodyLimit<S>` changes the response body type to
        // `ResponseBody<S::ResBody>`. To keep both arms of `option_layer`
        // unifiable as `Response<Body>`, we collapse the response body back
        // to our `Body` newtype with `MapResponseBodyLayer::new(Body::new)`
        // immediately after the limit layer.
        let body_limit_layer = params.max_request_body_bytes.map(|limit| {
            ServiceBuilder::new()
                .layer(tower_http::map_response_body::MapResponseBodyLayer::new(
                    Body::new,
                ))
                .layer(tower_http::limit::RequestBodyLimitLayer::new(limit))
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

        #[cfg(feature = "compression")]
        let result = {
            use tower_http::decompression::RequestDecompressionLayer;
            let req_decomp = RequestDecompressionLayer::new();
            if let Some(comp) = params.compression.as_ref().map(build_compression_layer) {
                let stack = ServiceBuilder::new()
                    .layer(req_decomp)
                    .layer(comp)
                    .service(core_stack);
                builder
                    .serve_connection(io, TowerToHyperService::new(stack))
                    .with_upgrades()
                    .await
            } else {
                let stack = ServiceBuilder::new().layer(req_decomp).service(core_stack);
                builder
                    .serve_connection(io, TowerToHyperService::new(stack))
                    .with_upgrades()
                    .await
            }
        };
        #[cfg(not(feature = "compression"))]
        let result = builder
            .serve_connection(io, TowerToHyperService::new(core_stack))
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
