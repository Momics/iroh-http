//! Configuration types for [`crate::IrohEndpoint::bind`].
//!
//! These were previously inlined in `endpoint.rs`; extracting them keeps the
//! endpoint module focused on the facade implementation while leaving the
//! public types right next to the rest of the public API surface.

/// Networking / QUIC transport configuration.
#[derive(Debug, Clone, Default)]
pub struct NetworkingOptions {
    /// Relay server mode. `"default"`, `"staging"`, `"disabled"`, or `"custom"`. Default: `"default"`.
    pub relay_mode: Option<String>,
    /// Custom relay server URLs. Only used when `relay_mode` is `"custom"`.
    pub relays: Vec<String>,
    /// UDP socket addresses to bind. Empty means OS-assigned.
    pub bind_addrs: Vec<String>,
    /// Milliseconds before an idle QUIC connection is cleaned up.
    pub idle_timeout_ms: Option<u64>,
    /// HTTP proxy URL for relay traffic.
    pub proxy_url: Option<String>,
    /// Read `HTTP_PROXY` / `HTTPS_PROXY` env vars for proxy config.
    pub proxy_from_env: bool,
    /// Disable relay servers and DNS discovery entirely. Overrides `relay_mode`.
    /// Useful for in-process tests where endpoints connect via direct addresses.
    pub disabled: bool,
}

/// DNS-based peer discovery configuration.
#[derive(Debug, Clone)]
pub struct DiscoveryOptions {
    /// DNS discovery server URL. Uses n0 DNS defaults when `None`.
    pub dns_server: Option<String>,
    /// Whether to enable DNS discovery. Default: `true`.
    pub enabled: bool,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            dns_server: None,
            enabled: true,
        }
    }
}

/// Connection-pool tuning.
#[derive(Debug, Clone, Default)]
pub struct PoolOptions {
    /// Maximum number of idle connections to keep in the pool.
    pub max_connections: Option<usize>,
    /// Milliseconds a pooled connection may remain idle before being evicted.
    pub idle_timeout_ms: Option<u64>,
}

/// Body-streaming and handle-store configuration.
#[derive(Debug, Clone, Default)]
pub struct StreamingOptions {
    /// Capacity (in chunks) of each body channel. Default: 32.
    pub channel_capacity: Option<usize>,
    /// Maximum byte length of a single chunk in `send_chunk`. Default: 65536.
    pub max_chunk_size_bytes: Option<usize>,
    /// Milliseconds to wait for a slow body reader. Default: 30000.
    pub drain_timeout_ms: Option<u64>,
    /// TTL in ms for slab handle entries. `0` disables sweeping. Default: 300000.
    pub handle_ttl_ms: Option<u64>,
    /// How often (in ms) the TTL sweep task runs. Default: 60000 (60 s).
    /// Reducing this lowers the worst-case leaked-handle window at the cost of
    /// more frequent write-lock acquisitions on every handle registry.
    /// Useful for short-lived endpoints and test fixtures.
    pub sweep_interval_ms: Option<u64>,
}

/// Configuration passed to [`crate::IrohEndpoint::bind`].
#[derive(Debug, Clone, Default)]
pub struct NodeOptions {
    /// 32-byte Ed25519 secret key. Generate a fresh one when `None`.
    pub key: Option<[u8; 32]>,
    /// Networking / QUIC transport configuration.
    pub networking: NetworkingOptions,
    /// DNS-based peer discovery configuration.
    pub discovery: DiscoveryOptions,
    /// Connection-pool tuning.
    pub pool: PoolOptions,
    /// Body-streaming and handle-store configuration.
    pub streaming: StreamingOptions,
    /// ALPN capabilities to advertise.
    ///
    /// Valid values: [`ALPN_STR`](crate::ALPN_STR) (`"iroh-http/2"`) and
    /// [`ALPN_DUPLEX_STR`](crate::ALPN_DUPLEX_STR) (`"iroh-http/2-duplex"`).
    ///
    /// When empty (the default), both protocols are advertised. When non-empty,
    /// the base protocol (`iroh-http/2`) is automatically injected if not
    /// already present. Unknown values cause [`crate::IrohEndpoint::bind`] to
    /// return an error.
    pub capabilities: Vec<String>,
    /// Write TLS session keys to $SSLKEYLOGFILE. Dev/debug only.
    pub keylog: bool,
    /// Maximum byte size of the HTTP/1.1 request or response head.
    /// `None` = 65536.  `Some(0)` is rejected.
    pub max_header_size: Option<usize>,
    /// Maximum decompressed response body bytes the client will accept per
    /// outgoing `fetch()`.  Default: 256 MiB.  Protects against compression
    /// bombs from malicious peers.
    pub max_response_body_bytes: Option<usize>,
    pub compression: Option<CompressionOptions>,
}

/// Compression options for response bodies.
/// Only used when the `compression` feature is enabled.
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
