//! [`HttpRuntime`] subsystem — connection pool, HTTP limits, request counters.
//!
//! Per ADR-014 D1 this is one of the four named subsystems composed into
//! [`super::EndpointInner`]. It owns everything that scales with HTTP
//! traffic: the pool, header/body limits, in-flight counters, and the
//! optional compression policy.

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use crate::http::transport::pool::ConnectionPool;

use crate::http::server::stack::CompressionOptions;

/// HTTP-layer runtime state.
pub(crate) struct HttpRuntime {
    /// Connection pool for reusing QUIC connections across fetch/connect calls.
    pub pool: ConnectionPool,
    /// Maximum byte size of an HTTP/1.1 head (request or response).
    pub max_header_size: usize,
    /// Maximum decompressed response body bytes per fetch. Default: 256 MiB.
    pub max_response_body_bytes: usize,
    /// Number of currently active QUIC connections (incremented by serve loop,
    /// decremented via RAII guard when each connection task exits).
    pub active_connections: Arc<AtomicUsize>,
    /// Number of currently in-flight HTTP requests.
    pub active_requests: Arc<AtomicUsize>,
    /// Body compression options, if the feature is enabled.
    pub compression: Option<CompressionOptions>,
}

#[cfg(test)]
impl HttpRuntime {
    /// Construct a minimal `HttpRuntime` for unit tests. Limits are set
    /// to compile-time defaults; no network state is involved.
    pub fn new_for_test() -> Self {
        Self {
            pool: ConnectionPool::new(Some(8), None, None),
            max_header_size: 64 * 1024,
            max_response_body_bytes: crate::http::server::DEFAULT_MAX_RESPONSE_BODY_BYTES,
            active_connections: Arc::new(AtomicUsize::new(0)),
            active_requests: Arc::new(AtomicUsize::new(0)),
            compression: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn new_for_test_has_default_limits() {
        let rt = HttpRuntime::new_for_test();
        assert_eq!(rt.max_header_size, 64 * 1024);
        assert_eq!(rt.active_connections.load(Ordering::Relaxed), 0);
        assert_eq!(rt.active_requests.load(Ordering::Relaxed), 0);
    }
}
