//! Iroh endpoint lifecycle — create, share, and close.

use iroh::address_lookup::{DnsAddressLookup, PkarrPublisher};
use iroh::endpoint::{IdleTimeout, QuicTransportConfig, TransportAddrUsage};
use iroh::{Endpoint, RelayMode, SecretKey};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::pool::ConnectionPool;
use crate::server::ServeHandle;
use crate::{ALPN, ALPN_DUPLEX};

/// Configuration passed to [`IrohEndpoint::bind`].
#[derive(Debug, Clone)]
pub struct NodeOptions {
    // ── Identity ────────────────────────────────────────────────────────────
    /// 32-byte Ed25519 secret key.  Generate a fresh one when `None`.
    pub key: Option<[u8; 32]>,

    // ── Connectivity ────────────────────────────────────────────────────────
    /// Relay server mode.  `"default"` uses n0's public relays, `"staging"` uses
    /// the canary relay, `"disabled"` disables relay entirely, and `"custom"` uses
    /// only the URLs in [`relays`](Self::relays).  Default: `"default"`.
    pub relay_mode: Option<String>,
    /// Custom relay server URLs.  Only used when `relay_mode` is `"custom"`.
    pub relays: Vec<String>,
    /// UDP socket addresses to bind.  Empty means OS-assigned (`"0.0.0.0:0"`).
    pub bind_addrs: Vec<String>,
    /// Milliseconds before an idle QUIC connection is cleaned up.
    pub idle_timeout_ms: Option<u64>,

    // ── Discovery ───────────────────────────────────────────────────────────
    /// DNS discovery server URL override.  Uses n0 DNS defaults when `None`.
    pub dns_discovery: Option<String>,
    /// Whether to enable DNS discovery.  Default: true.
    pub dns_discovery_enabled: bool,

    // ── Capabilities ────────────────────────────────────────────────────────
    /// ALPN capabilities to advertise.  When empty, the endpoint advertises
    /// `iroh-http/2` (HTTP) and `iroh-http/2-duplex` (duplex/session) in
    /// preference order.
    pub capabilities: Vec<String>,

    // ── Power-user options ──────────────────────────────────────────────────
    /// HTTP proxy URL for relay traffic.  For corporate networks.
    pub proxy_url: Option<String>,
    /// Read `HTTP_PROXY` / `HTTPS_PROXY` env vars for proxy config.
    pub proxy_from_env: bool,
    /// Write TLS session keys to `$SSLKEYLOGFILE`.  Dev/debug only.
    pub keylog: bool,
    /// Capacity (in chunks) of each body channel.  Default: 32.
    pub channel_capacity: Option<usize>,
    /// Maximum byte length of a single chunk in `send_chunk`.  Default: 65536.
    pub max_chunk_size_bytes: Option<usize>,
    /// Number of consecutive accept errors before the serve loop gives up.  Default: 5.
    /// Convenience alias — also accessible via `server_limits.max_consecutive_errors`.
    pub max_consecutive_errors: Option<usize>,
    /// Disable relay servers and DNS discovery entirely.
    /// Useful for in-process tests where endpoints connect via direct addresses.
    pub disable_networking: bool,
    /// Milliseconds to wait for a slow body reader before dropping the connection.
    /// Default: 30 000 (30 s).
    pub drain_timeout_ms: Option<u64>,
    /// TTL in milliseconds for slab handle entries.  Expired entries are swept
    /// every 60 s.  `0` disables sweeping.  Default: 300 000 (5 min).
    pub handle_ttl_ms: Option<u64>,
    /// Maximum number of idle connections to keep in the pool.
    pub max_pooled_connections: Option<usize>,
    /// Milliseconds a pooled connection may remain idle before being evicted.
    /// `None` (default) keeps connections indefinitely (until closed or the pool
    /// reaches `max_pooled_connections`).
    pub pool_idle_timeout_ms: Option<u64>,
    /// Maximum byte size of the HTTP/1.1 request or response head (status line + headers).
    /// Default: 65536 (64 KB).  Set to 0 to use the default.
    pub max_header_size: Option<usize>,

    // ── Server limits ───────────────────────────────────────────────────────
    /// Server-side limits forwarded to the serve loop.  Adding a field here
    /// forces a corresponding update in `EndpointInner` and `serve_options()`.
    pub server_limits: crate::server::ServerLimits,

    // ── Compression ─────────────────────────────────────────────────────────
    /// Body compression options.  `None` disables compression (default).
    /// Only effective when the `compression` feature is enabled.
    #[cfg(feature = "compression")]
    pub compression: Option<CompressionOptions>,
}

/// Compression options for response bodies.
/// Only used when the `compression` feature is enabled.
#[cfg(feature = "compression")]
#[derive(Debug, Clone)]
pub struct CompressionOptions {
    /// Minimum body size in bytes before compression is applied. Default: 512.
    pub min_body_bytes: usize,
    /// Zstd compression level (1–22). `None` uses the zstd default (3).
    pub level: Option<u32>,
}

impl Default for NodeOptions {
    fn default() -> Self {
        Self {
            key: None,
            relay_mode: None,
            relays: Vec::new(),
            bind_addrs: Vec::new(),
            idle_timeout_ms: None,
            dns_discovery: None,
            dns_discovery_enabled: true,
            capabilities: Vec::new(),
            proxy_url: None,
            proxy_from_env: false,
            keylog: false,
            channel_capacity: None,
            max_chunk_size_bytes: None,
            max_consecutive_errors: None,
            disable_networking: false,
            drain_timeout_ms: None,
            handle_ttl_ms: None,
            max_pooled_connections: None,
            pool_idle_timeout_ms: None,
            max_header_size: None,
            server_limits: crate::server::ServerLimits::default(),
            #[cfg(feature = "compression")]
            compression: None,
        }
    }
}

/// A shared Iroh endpoint.
///
/// Clone-able (cheap Arc clone).  All fetch and serve calls on the same node
/// share one endpoint and therefore one stable QUIC identity.
#[derive(Clone)]
pub struct IrohEndpoint {
    pub(crate) inner: Arc<EndpointInner>,
}

pub(crate) struct EndpointInner {
    pub ep: Endpoint,
    /// The node's own base32-encoded public key (stable for the lifetime of the key).
    pub node_id_str: String,
    /// Connection pool for reusing QUIC connections across fetch/connect calls.
    pub pool: ConnectionPool,
    /// Maximum byte size of an HTTP/1.1 head (request or response).
    pub max_header_size: usize,
    /// Server-side limits forwarded to the serve loop.
    pub server_limits: crate::server::ServerLimits,
    /// Per-endpoint index for global slab registries.
    pub endpoint_idx: u32,
    /// Active serve handle, if `serve()` has been called.
    pub serve_handle: std::sync::Mutex<Option<ServeHandle>>,
    /// Body compression options, if the feature is enabled.
    #[cfg(feature = "compression")]
    pub compression: Option<CompressionOptions>,
}

impl IrohEndpoint {
    /// Bind an Iroh endpoint with the supplied options.
    pub async fn bind(opts: NodeOptions) -> Result<Self, crate::CoreError> {
        let relay_mode = if opts.disable_networking {
            RelayMode::Disabled
        } else {
            match opts.relay_mode.as_deref() {
                None | Some("default") => RelayMode::Default,
                Some("staging") => RelayMode::Staging,
                Some("disabled") => RelayMode::Disabled,
                Some("custom") => {
                    if opts.relays.is_empty() {
                        return Err(crate::CoreError::invalid_input(
                            "relay_mode \"custom\" requires at least one URL in `relays`",
                        ));
                    }
                    let urls = opts
                        .relays
                        .iter()
                        .map(|u| {
                            u.parse::<iroh::RelayUrl>()
                                .map_err(crate::CoreError::invalid_input)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    RelayMode::custom(urls)
                }
                Some(other) => {
                    return Err(crate::CoreError::invalid_input(format!(
                        "unknown relay_mode: {other}"
                    )))
                }
            }
        };

        let alpns: Vec<Vec<u8>> = if opts.capabilities.is_empty() {
            // Advertise both ALPN variants.
            vec![ALPN_DUPLEX.to_vec(), ALPN.to_vec()]
        } else {
            let mut list: Vec<Vec<u8>> = opts
                .capabilities
                .iter()
                .map(|c| c.as_bytes().to_vec())
                .collect();
            // Always include the base protocol so the node can talk to base-only peers.
            if !list.iter().any(|a| a == ALPN) {
                list.push(ALPN.to_vec());
            }
            list
        };

        let mut builder = Endpoint::empty_builder(relay_mode).alpns(alpns);

        // DNS discovery (enabled by default unless disable_networking).
        if !opts.disable_networking && opts.dns_discovery_enabled {
            if let Some(ref url_str) = opts.dns_discovery {
                let url: url::Url = url_str.parse().map_err(|e| {
                    crate::CoreError::invalid_input(format!("invalid dns_discovery URL: {e}"))
                })?;
                builder = builder
                    .address_lookup(PkarrPublisher::builder(url.clone()))
                    .address_lookup(DnsAddressLookup::builder(
                        url.host_str().unwrap_or_default().to_string(),
                    ));
            } else {
                builder = builder
                    .address_lookup(PkarrPublisher::n0_dns())
                    .address_lookup(DnsAddressLookup::n0_dns());
            }
        }

        if let Some(key_bytes) = opts.key {
            builder = builder.secret_key(SecretKey::from_bytes(&key_bytes));
        }

        if let Some(ms) = opts.idle_timeout_ms {
            let timeout = IdleTimeout::try_from(Duration::from_millis(ms)).map_err(|e| {
                crate::CoreError::invalid_input(format!("idle_timeout_ms out of range: {e}"))
            })?;
            let transport = QuicTransportConfig::builder()
                .max_idle_timeout(Some(timeout))
                .build();
            builder = builder.transport_config(transport);
        }

        // Bind address(es).
        for addr_str in &opts.bind_addrs {
            let sock: std::net::SocketAddr = addr_str.parse().map_err(|e| {
                crate::CoreError::invalid_input(format!("invalid bind address \"{addr_str}\": {e}"))
            })?;
            builder = builder.bind_addr(sock).map_err(|e| {
                crate::CoreError::invalid_input(format!("bind address \"{addr_str}\": {e}"))
            })?;
        }

        // Proxy configuration.
        if let Some(ref proxy) = opts.proxy_url {
            let url: url::Url = proxy
                .parse()
                .map_err(|e| crate::CoreError::invalid_input(format!("invalid proxy URL: {e}")))?;
            builder = builder.proxy_url(url);
        } else if opts.proxy_from_env {
            builder = builder.proxy_from_env();
        }

        // TLS keylog for Wireshark debugging.
        if opts.keylog {
            builder = builder.keylog(true);
        }

        let ep = builder.bind().await.map_err(classify_bind_error)?;

        // Apply backpressure config for all future channel allocations.
        crate::stream::configure_backpressure(
            opts.channel_capacity
                .unwrap_or(crate::stream::DEFAULT_CHANNEL_CAPACITY),
            opts.max_chunk_size_bytes
                .unwrap_or(crate::stream::DEFAULT_MAX_CHUNK_SIZE),
            opts.drain_timeout_ms
                .unwrap_or(crate::stream::DEFAULT_DRAIN_TIMEOUT_MS),
        );

        // Start slab TTL sweep if configured.
        crate::stream::start_slab_sweep(
            opts.handle_ttl_ms
                .unwrap_or(crate::stream::DEFAULT_SLAB_TTL_MS),
        );

        let node_id_str = crate::base32_encode(ep.id().as_bytes());

        let endpoint_idx = crate::stream::alloc_endpoint_idx();
        crate::stream::register_endpoint(endpoint_idx);

        Ok(Self {
            inner: Arc::new(EndpointInner {
                ep,
                node_id_str,
                pool: ConnectionPool::new(
                    opts.max_pooled_connections,
                    opts.pool_idle_timeout_ms
                        .map(std::time::Duration::from_millis),
                ),
                // ISS-020: treat 0 as "use default" — it would otherwise underflow
                // the hyper minimum (ISS-001).  None also defaults to 64 KB.
                max_header_size: match opts.max_header_size {
                    None | Some(0) => 64 * 1024,
                    Some(n) => n,
                },
                server_limits: {
                    let mut sl = opts.server_limits.clone();
                    // Merge standalone max_consecutive_errors into server_limits
                    // (backward compat — adapters may set the standalone field).
                    if sl.max_consecutive_errors.is_none() {
                        sl.max_consecutive_errors = opts.max_consecutive_errors.or(Some(5));
                    }
                    sl
                },
                endpoint_idx,
                serve_handle: std::sync::Mutex::new(None),
                #[cfg(feature = "compression")]
                compression: opts.compression,
            }),
        })
    }

    /// The node's public key as a lowercase base32 string.
    pub fn node_id(&self) -> &str {
        &self.inner.node_id_str
    }

    /// The configured consecutive-error limit for the serve loop.
    pub fn max_consecutive_errors(&self) -> usize {
        self.inner.server_limits.max_consecutive_errors.unwrap_or(5)
    }

    /// Build a [`ServeOptions`] from the endpoint's stored configuration.
    ///
    /// Platform adapters should call this instead of constructing `ServeOptions`
    /// manually so that all server-limit fields are forwarded consistently.
    pub fn serve_options(&self) -> crate::server::ServeOptions {
        self.inner.server_limits.clone()
    }

    /// The node's raw secret key bytes (32 bytes).
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.inner.ep.secret_key().to_bytes()
    }

    /// Graceful close: signal the serve loop to stop accepting, wait for
    /// in-flight requests to drain (up to the configured drain timeout),
    /// then close the QUIC endpoint.
    ///
    /// If no serve loop is running, closes the endpoint immediately.
    pub async fn close(&self) {
        // ISS-027: drain in-flight requests *before* unregistering slab entries so
        // that request handlers can still access their reader/writer/trailer handles
        // during the drain window.
        let handle = self
            .inner
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(h) = handle {
            h.drain().await;
        }
        crate::stream::unregister_endpoint(self.inner.endpoint_idx);
        self.inner.ep.close().await;
    }

    /// Immediate close: abort the serve loop and close the endpoint with
    /// no drain period.
    pub async fn close_force(&self) {
        crate::stream::unregister_endpoint(self.inner.endpoint_idx);
        let handle = self
            .inner
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(h) = handle {
            h.abort();
        }
        self.inner.ep.close().await;
    }

    /// Store a serve handle so that `close()` can drain it.
    pub fn set_serve_handle(&self, handle: ServeHandle) {
        *self
            .inner
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(handle);
    }

    /// Signal the serve loop to stop accepting new connections.
    ///
    /// Returns immediately — does NOT close the endpoint or drain in-flight
    /// requests.  The handle is preserved so `close()` can still drain later.
    pub fn stop_serve(&self) {
        if let Some(h) = self
            .inner
            .serve_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            h.shutdown();
        }
    }

    pub fn raw(&self) -> &Endpoint {
        &self.inner.ep
    }

    /// Per-endpoint index used by global slab registries (cancellation tokens, etc.).
    pub fn endpoint_idx(&self) -> u32 {
        self.inner.endpoint_idx
    }

    /// Maximum byte size of an HTTP/1.1 head.
    pub fn max_header_size(&self) -> usize {
        self.inner.max_header_size
    }

    /// Access the connection pool.
    pub(crate) fn pool(&self) -> &ConnectionPool {
        &self.inner.pool
    }

    /// Compression options, if the `compression` feature is enabled.
    #[cfg(feature = "compression")]
    pub fn compression(&self) -> Option<&CompressionOptions> {
        self.inner.compression.as_ref()
    }

    /// Returns the local socket addresses this endpoint is bound to.
    pub fn bound_sockets(&self) -> Vec<std::net::SocketAddr> {
        self.inner.ep.bound_sockets()
    }

    /// Full node address: node ID + relay URL(s) + direct socket addresses.
    pub fn node_addr(&self) -> NodeAddrInfo {
        let addr = self.inner.ep.addr();
        let mut addrs = Vec::new();
        for relay in addr.relay_urls() {
            addrs.push(relay.to_string());
        }
        for da in addr.ip_addrs() {
            addrs.push(da.to_string());
        }
        NodeAddrInfo {
            id: self.inner.node_id_str.clone(),
            addrs,
        }
    }

    /// Home relay URL, or `None` if not connected to a relay.
    pub fn home_relay(&self) -> Option<String> {
        self.inner
            .ep
            .addr()
            .relay_urls()
            .next()
            .map(|u| u.to_string())
    }

    /// Known addresses for a remote peer, or `None` if not in the endpoint's cache.
    pub async fn peer_info(&self, node_id_b32: &str) -> Option<NodeAddrInfo> {
        let bytes = crate::base32_decode(node_id_b32).ok()?;
        let arr: [u8; 32] = bytes.try_into().ok()?;
        let pk = iroh::PublicKey::from_bytes(&arr).ok()?;
        let info = self.inner.ep.remote_info(pk).await?;
        let id = crate::base32_encode(info.id().as_bytes());
        let mut addrs = Vec::new();
        for a in info.addrs() {
            match a.addr() {
                iroh::TransportAddr::Ip(sock) => addrs.push(sock.to_string()),
                iroh::TransportAddr::Relay(url) => addrs.push(url.to_string()),
                other => addrs.push(format!("{:?}", other)),
            }
        }
        Some(NodeAddrInfo { id, addrs })
    }

    /// Per-peer connection statistics.
    ///
    /// Returns path information for each known transport address, including
    /// whether each path is via a relay or direct, and which is active.
    pub async fn peer_stats(&self, node_id_b32: &str) -> Option<PeerStats> {
        let bytes = crate::base32_decode(node_id_b32).ok()?;
        let arr: [u8; 32] = bytes.try_into().ok()?;
        let pk = iroh::PublicKey::from_bytes(&arr).ok()?;
        let info = self.inner.ep.remote_info(pk).await?;

        let mut paths = Vec::new();
        let mut has_active_relay = false;
        let mut active_relay_url: Option<String> = None;

        for a in info.addrs() {
            let is_relay = a.addr().is_relay();
            let is_active = matches!(a.usage(), TransportAddrUsage::Active);

            let addr_str = match a.addr() {
                iroh::TransportAddr::Ip(sock) => sock.to_string(),
                iroh::TransportAddr::Relay(url) => {
                    if is_active {
                        has_active_relay = true;
                        active_relay_url = Some(url.to_string());
                    }
                    url.to_string()
                }
                other => format!("{:?}", other),
            };

            paths.push(PathInfo {
                relay: is_relay,
                addr: addr_str,
                active: is_active,
            });
        }

        // Enrich with QUIC connection-level stats if a pooled connection exists.
        let (rtt_ms, bytes_sent, bytes_received, lost_packets, sent_packets, congestion_window) =
            if let Some(pooled) = self.inner.pool.get_existing(pk, crate::ALPN).await {
                let s = pooled.conn.stats();
                let rtt = pooled.conn.rtt(iroh::endpoint::PathId::ZERO);
                (
                    rtt.map(|d| d.as_secs_f64() * 1000.0),
                    Some(s.udp_tx.bytes),
                    Some(s.udp_rx.bytes),
                    None, // quinn path stats not exposed via iroh ConnectionStats
                    None, // quinn path stats not exposed via iroh ConnectionStats
                    None, // quinn path stats not exposed via iroh ConnectionStats
                )
            } else {
                (None, None, None, None, None, None)
            };

        Some(PeerStats {
            relay: has_active_relay,
            relay_url: active_relay_url,
            paths,
            rtt_ms,
            bytes_sent,
            bytes_received,
            lost_packets,
            sent_packets,
            congestion_window,
        })
    }
}

// ── Bind-error classification ────────────────────────────────────────────────

/// Classify a bind error into a prefixed string for the JS error mapper.
fn classify_bind_error(e: impl std::fmt::Display) -> crate::CoreError {
    let msg = e.to_string();
    crate::CoreError::connection_failed(msg)
}

// ── NodeAddr info ────────────────────────────────────────────────────────────

/// Serialisable node address: node ID + relay and direct addresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAddrInfo {
    /// Base32-encoded public key.
    pub id: String,
    /// Relay URLs and/or `ip:port` direct addresses.
    pub addrs: Vec<String>,
}

// ── Observability types ──────────────────────────────────────────────────────

/// Per-peer connection statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStats {
    /// Whether the peer is connected via a relay server (vs direct).
    pub relay: bool,
    /// Active relay URL, if any.
    pub relay_url: Option<String>,
    /// All known paths to this peer.
    pub paths: Vec<PathInfo>,
    /// Round-trip time in milliseconds.  `None` if no active QUIC connection is pooled.
    pub rtt_ms: Option<f64>,
    /// Total UDP bytes sent to this peer.  `None` if no active QUIC connection is pooled.
    pub bytes_sent: Option<u64>,
    /// Total UDP bytes received from this peer.  `None` if no active QUIC connection is pooled.
    pub bytes_received: Option<u64>,
    /// Total packets lost on the QUIC path.  `None` if no active QUIC connection is pooled.
    pub lost_packets: Option<u64>,
    /// Total packets sent on the QUIC path.  `None` if no active QUIC connection is pooled.
    pub sent_packets: Option<u64>,
    /// Current congestion window in bytes.  `None` if no active QUIC connection is pooled.
    pub congestion_window: Option<u64>,
}

/// Network path information for a single transport address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathInfo {
    /// Whether this path goes through a relay server.
    pub relay: bool,
    /// The relay URL (if relay) or `ip:port` (if direct).
    pub addr: String,
    /// Whether this is the currently selected/active path.
    pub active: bool,
}

/// Parse an optional list of socket address strings into `SocketAddr` values.
///
/// Returns `Err` if any string cannot be parsed as a `host:port` address so
/// that callers can surface misconfiguration rather than silently ignoring it.
pub fn parse_direct_addrs(
    addrs: &Option<Vec<String>>,
) -> Result<Option<Vec<std::net::SocketAddr>>, String> {
    match addrs {
        None => Ok(None),
        Some(v) => {
            let mut out = Vec::with_capacity(v.len());
            for s in v {
                let addr = s
                    .parse::<std::net::SocketAddr>()
                    .map_err(|e| format!("invalid direct address {s:?}: {e}"))?;
                out.push(addr);
            }
            Ok(Some(out))
        }
    }
}
