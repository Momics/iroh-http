//! Iroh endpoint lifecycle — create, share, and close.
//!
//! [`IrohEndpoint`] is a thin façade over [`EndpointInner`], which is
//! composed of the four named subsystems from ADR-014 D1:
//!
//! - [`transport::Transport`] — raw QUIC endpoint and stable identity.
//! - [`http_runtime::HttpRuntime`] — pool, HTTP limits, in-flight counters.
//! - [`session_runtime::SessionRuntime`] — serve loop, lifecycle signals,
//!   transport events, path subscriptions.
//! - [`ffi_bridge::FfiBridge`] — the opaque-handle store reachable from JS.
//!
//! No business logic lives in this module — only orchestration and the
//! public API surface. Sub-modules:
//! - [`bind`] — `IrohEndpoint::bind()` constructor.
//! - [`observe`] — observability and peer-info methods.
//! - [`config`] — `NodeOptions` and friends.
//! - [`stats`] — snapshot and event types.
// `handles()` returns `&HandleStore` which is in the disallowed-types list.
// The disallowed_types lint is used to prevent mod http from depending on mod
// ffi; endpoint/ is neither mod http nor mod ffi, so the allow is correct here.
#![allow(clippy::disallowed_types)]

use std::sync::Arc;

pub(in crate::endpoint) mod bind;
pub(in crate::endpoint) mod ffi_bridge;
pub(in crate::endpoint) mod http_runtime;
pub(in crate::endpoint) mod observe;
pub(in crate::endpoint) mod session_runtime;
pub(in crate::endpoint) mod transport;

pub mod config;
pub mod stats;

pub use bind::parse_direct_addrs;
pub use config::{
    DiscoveryOptions, NetworkingOptions, NodeOptions, PoolOptions, StreamingOptions,
};
pub use http::server::stack::CompressionOptions;
pub use stats::{ConnectionEvent, EndpointStats, NodeAddrInfo, PathInfo, PeerStats};

use crate::ffi::handles::HandleStore;
use crate::http;
use crate::http::transport::pool::ConnectionPool;

use ffi_bridge::FfiBridge;
use http_runtime::HttpRuntime;
use session_runtime::SessionRuntime;
use transport::Transport;

/// A shared Iroh endpoint.
///
/// Clone-able (cheap Arc clone).  All fetch and serve calls on the same node
/// share one endpoint and therefore one stable QUIC identity.
#[derive(Clone)]
pub struct IrohEndpoint {
    pub(in crate::endpoint) inner: Arc<EndpointInner>,
}

/// Composition of the four ADR-014 D1 subsystems. No business logic; the
/// public API on [`IrohEndpoint`] reaches into the appropriate subsystem.
pub(in crate::endpoint) struct EndpointInner {
    pub(in crate::endpoint) transport: Transport,
    pub(in crate::endpoint) http: HttpRuntime,
    pub(in crate::endpoint) session: SessionRuntime,
    pub(in crate::endpoint) ffi: FfiBridge,
}

impl IrohEndpoint {
    // ── Stable identity ──────────────────────────────────────────────────────

    /// The node's public key as a lowercase base32 string.
    pub fn node_id(&self) -> &str {
        &self.inner.transport.node_id_str
    }

    /// The node's raw secret key bytes (32 bytes).
    ///
    /// # Security
    ///
    /// **These 32 bytes are the irrecoverable private key for this node.**
    /// Anyone who obtains them can impersonate this node permanently.
    /// Never log, print, or include in error payloads. Encrypt at rest.
    /// Zeroize after use.
    #[must_use]
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.inner.transport.ep.secret_key().to_bytes()
    }

    // ── Handle store ─────────────────────────────────────────────────────────

    /// Per-endpoint handle store.
    pub fn handles(&self) -> &HandleStore {
        &self.inner.ffi.handles
    }

    /// Immediately run a TTL sweep on all handle registries.
    pub fn sweep_now(&self) {
        let ttl = self.inner.ffi.handles.config.ttl;
        if !ttl.is_zero() {
            self.inner.ffi.handles.sweep(ttl);
        }
    }

    // ── HTTP runtime accessors ────────────────────────────────────────────────

    /// Maximum byte size of an HTTP/1.1 head.
    pub fn max_header_size(&self) -> usize {
        self.inner.http.max_header_size
    }

    /// Maximum decompressed response-body bytes accepted per outgoing fetch.
    pub fn max_response_body_bytes(&self) -> usize {
        self.inner.http.max_response_body_bytes
    }

    /// Compression options, if the `compression` feature is enabled.
    pub fn compression(&self) -> Option<&CompressionOptions> {
        self.inner.http.compression.as_ref()
    }

    /// Access the connection pool.
    pub(crate) fn pool(&self) -> &ConnectionPool {
        &self.inner.http.pool
    }

    /// Shared active-connections counter (used by the accept loop).
    pub(crate) fn active_connections_arc(&self) -> Arc<std::sync::atomic::AtomicUsize> {
        self.inner.http.active_connections.clone()
    }

    /// Shared active-requests counter (used by the accept loop).
    pub(crate) fn active_requests_arc(&self) -> Arc<std::sync::atomic::AtomicUsize> {
        self.inner.http.active_requests.clone()
    }

    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Closed-signal sender shared with the accept loop.
    pub(crate) fn connection_closed_tx(&self) -> tokio::sync::watch::Sender<bool> {
        self.inner.session.closed_tx.clone()
    }

    /// Access the raw Iroh endpoint.
    pub fn raw(&self) -> &iroh::Endpoint {
        &self.inner.transport.ep
    }

    /// Graceful close: signal the serve loop to stop accepting, wait for
    /// in-flight requests to drain, then close the QUIC endpoint.
    pub async fn close(&self) {
        let handle = self
            .inner
            .session
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(h) = handle {
            h.drain().await;
        }
        self.inner.transport.ep.close().await;
        let _ = self.inner.session.closed_tx.send(true);
    }

    /// Immediate close: abort the serve loop with no drain period.
    pub async fn close_force(&self) {
        let handle = self
            .inner
            .session
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(h) = handle {
            h.abort();
        }
        self.inner.transport.ep.close().await;
        let _ = self.inner.session.closed_tx.send(true);
    }

    /// Wait until this endpoint has been closed. Returns immediately if already closed.
    pub async fn wait_closed(&self) {
        let mut rx = self.inner.session.closed_rx.clone();
        let _ = rx.wait_for(|v| *v).await;
    }

    /// Store a serve handle so that `close()` can drain it.
    pub fn set_serve_handle(&self, handle: http::server::ServeHandle) {
        *self
            .inner
            .session
            .serve_done_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(handle.subscribe_done());
        *self
            .inner
            .session
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(handle);
    }

    /// Signal the serve loop to stop accepting new connections.
    pub fn stop_serve(&self) {
        if let Some(h) = self
            .inner
            .session
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            h.shutdown();
        }
    }

    /// Wait until the serve loop has fully exited.
    pub async fn wait_serve_stop(&self) {
        let rx = self
            .inner
            .session
            .serve_done_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(mut rx) = rx {
            let _ = rx.wait_for(|v| *v).await;
        }
    }

    // ── Events ────────────────────────────────────────────────────────────────

    /// Take the transport event receiver, handing it off to a platform drain task.
    /// May only be called once per endpoint.
    pub fn subscribe_events(
        &self,
    ) -> Option<tokio::sync::mpsc::Receiver<crate::http::events::TransportEvent>> {
        self.inner
            .session
            .event_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }
}
