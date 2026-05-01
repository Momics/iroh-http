//! HttpRuntime subsystem — connection pool, HTTP limits, in-flight counters.

use std::sync::{atomic::AtomicUsize, Arc};

use crate::http::server::stack::CompressionOptions;
use crate::http::transport::pool::ConnectionPool;

/// HTTP-layer runtime state.
pub(in crate::endpoint) struct HttpRuntime {
    /// Connection pool for reusing QUIC connections across fetch calls.
    pub(in crate::endpoint) pool: ConnectionPool,
    /// Maximum byte size of an HTTP/1.1 head (request or response).
    pub(in crate::endpoint) max_header_size: usize,
    /// Maximum decompressed response body bytes per fetch. Default: 256 MiB.
    pub(in crate::endpoint) max_response_body_bytes: usize,
    /// Number of currently active QUIC connections (incremented by serve loop,
    /// decremented via RAII guard when each connection task exits).
    pub(in crate::endpoint) active_connections: Arc<AtomicUsize>,
    /// Number of currently in-flight HTTP requests.
    pub(in crate::endpoint) active_requests: Arc<AtomicUsize>,
    /// Body compression options, if the feature is enabled.
    pub(in crate::endpoint) compression: Option<CompressionOptions>,
}
