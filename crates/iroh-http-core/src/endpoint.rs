//! Iroh endpoint lifecycle — create, share, and close.

use iroh::address_lookup::{DnsAddressLookup, PkarrPublisher};
use iroh::endpoint::{IdleTimeout, QuicTransportConfig, TransportAddrUsage};
use iroh::{Endpoint, RelayMode, SecretKey};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::pool::ConnectionPool;
use crate::server::ServeHandle;
use crate::stream::{HandleStore, StoreConfig};
use crate::{ALPN, ALPN_DUPLEX};

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
    fn default() -> Self { Self { dns_server: None, enabled: true } }
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
}

/// Configuration passed to [`IrohEndpoint::bind`].
#[derive(Debug, Clone)]
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
    /// ALPN capabilities to advertise. Empty = advertise iroh-http/2 and iroh-http/2-duplex.
    pub capabilities: Vec<String>,
    /// Write TLS session keys to $SSLKEYLOGFILE. Dev/debug only.
    pub keylog: bool,
    /// Maximum byte size of the HTTP/1.1 request or response head. `None` or `0` = 65536.
    pub max_header_size: Option<usize>,
    /// Server-side limits forwarded to the serve loop.
    pub server_limits: crate::server::ServerLimits,
    #[cfg(feature = "compression")]
    pub compression: Option<CompressionOptions>,
}

impl Default for NodeOptions {
    fn default() -> Self {
        Self {
            key: None,
            networking: NetworkingOptions::default(),
            discovery: DiscoveryOptions::default(),
            pool: PoolOptions::default(),
            streaming: StreamingOptions::default(),
            capabilities: Vec::new(),
            keylog: false,
            max_header_size: None,
            server_limits: crate::server::ServerLimits::default(),
            #[cfg(feature = "compression")]
            compression: None,
        }
    }
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
    /// Per-endpoint handle store — owns all body readers, writers, trailers,
    /// sessions, request-head channels, and fetch-cancel tokens.
    pub handles: HandleStore,
    /// Active serve handle, if `serve()` has been called.
    pub serve_handle: std::sync::Mutex<Option<ServeHandle>>,
    /// Done-signal receiver from the active serve task.
    /// Stored separately so `wait_serve_stop()` can await it without holding
    /// the `serve_handle` lock for the duration of the wait.
    pub serve_done_rx: std::sync::Mutex<Option<tokio::sync::watch::Receiver<bool>>>,
    /// Signals `true` when the endpoint has fully closed (either explicitly or
    /// because the serve loop exited due to native shutdown).
    pub closed_tx: tokio::sync::watch::Sender<bool>,
    pub closed_rx: tokio::sync::watch::Receiver<bool>,
    /// Number of currently active QUIC connections (incremented by serve loop,
    /// decremented via RAII guard when each connection task exits).
    pub active_connections: Arc<AtomicUsize>,
    /// Number of currently in-flight HTTP requests (incremented when a
    /// bi-stream is accepted, decremented when the request task exits).
    pub active_requests: Arc<AtomicUsize>,
    /// Body compression options, if the feature is enabled.
    #[cfg(feature = "compression")]
    pub compression: Option<CompressionOptions>,
}

impl IrohEndpoint {
    /// Bind an Iroh endpoint with the supplied options.
    pub async fn bind(opts: NodeOptions) -> Result<Self, crate::CoreError> {
        // Validate: if networking is disabled, relay_mode should not be explicitly set to a
        // network-requiring mode.
        if opts.networking.disabled
            && opts.networking.relay_mode.as_deref()
                .map_or(false, |m| !matches!(m, "disabled"))
        {
            return Err(crate::CoreError::invalid_input(
                "networking.disabled is true but relay_mode is set to a non-disabled value; \
                 set relay_mode to \"disabled\" or omit it when networking.disabled is true",
            ));
        }

        let relay_mode = if opts.networking.disabled {
            RelayMode::Disabled
        } else {
            match opts.networking.relay_mode.as_deref() {
                None | Some("default") => RelayMode::Default,
                Some("staging") => RelayMode::Staging,
                Some("disabled") => RelayMode::Disabled,
                Some("custom") => {
                    if opts.networking.relays.is_empty() {
                        return Err(crate::CoreError::invalid_input(
                            "relay_mode \"custom\" requires at least one URL in `relays`",
                        ));
                    }
                    let urls = opts
                        .networking
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

        // DNS discovery (enabled by default unless networking.disabled).
        if !opts.networking.disabled && opts.discovery.enabled {
            if let Some(ref url_str) = opts.discovery.dns_server {
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

        if let Some(ms) = opts.networking.idle_timeout_ms {
            let timeout = IdleTimeout::try_from(Duration::from_millis(ms)).map_err(|e| {
                crate::CoreError::invalid_input(format!("idle_timeout_ms out of range: {e}"))
            })?;
            let transport = QuicTransportConfig::builder()
                .max_idle_timeout(Some(timeout))
                .build();
            builder = builder.transport_config(transport);
        }

        // Bind address(es).
        for addr_str in &opts.networking.bind_addrs {
            let sock: std::net::SocketAddr = addr_str.parse().map_err(|e| {
                crate::CoreError::invalid_input(format!("invalid bind address \"{addr_str}\": {e}"))
            })?;
            builder = builder.bind_addr(sock).map_err(|e| {
                crate::CoreError::invalid_input(format!("bind address \"{addr_str}\": {e}"))
            })?;
        }

        // Proxy configuration.
        if let Some(ref proxy) = opts.networking.proxy_url {
            let url: url::Url = proxy
                .parse()
                .map_err(|e| crate::CoreError::invalid_input(format!("invalid proxy URL: {e}")))?;
            builder = builder.proxy_url(url);
        } else if opts.networking.proxy_from_env {
            builder = builder.proxy_from_env();
        }

        // TLS keylog for Wireshark debugging.
        if opts.keylog {
            builder = builder.keylog(true);
        }

        let ep = builder.bind().await.map_err(classify_bind_error)?;

        let node_id_str = crate::base32_encode(ep.id().as_bytes());

        let store_config = StoreConfig {
            channel_capacity: opts
                .streaming
                .channel_capacity
                .unwrap_or(crate::stream::DEFAULT_CHANNEL_CAPACITY)
                .max(1),
            max_chunk_size: opts
                .streaming
                .max_chunk_size_bytes
                .unwrap_or(crate::stream::DEFAULT_MAX_CHUNK_SIZE)
                .max(1),
            drain_timeout: Duration::from_millis(
                opts.streaming
                    .drain_timeout_ms
                    .unwrap_or(crate::stream::DEFAULT_DRAIN_TIMEOUT_MS),
            ),
            max_handles: crate::stream::DEFAULT_MAX_HANDLES,
            ttl: Duration::from_millis(
                opts.streaming
                    .handle_ttl_ms
                    .unwrap_or(crate::stream::DEFAULT_SLAB_TTL_MS),
            ),
        };
        let sweep_ttl = store_config.ttl;
        let (closed_tx, closed_rx) = tokio::sync::watch::channel(false);

        let inner = Arc::new(EndpointInner {
            ep,
            node_id_str,
            pool: ConnectionPool::new(
                opts.pool.max_connections,
                opts.pool.idle_timeout_ms
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
                if sl.max_consecutive_errors.is_none() {
                    sl.max_consecutive_errors = Some(5);
                }
                sl
            },
            handles: HandleStore::new(store_config),
            serve_handle: std::sync::Mutex::new(None),
            serve_done_rx: std::sync::Mutex::new(None),
            closed_tx,
            closed_rx,
            active_connections: Arc::new(AtomicUsize::new(0)),
            active_requests: Arc::new(AtomicUsize::new(0)),
            #[cfg(feature = "compression")]
            compression: opts.compression,
        });

        // Start per-endpoint sweep task (held alive via Weak reference).
        if !sweep_ttl.is_zero() {
            let weak = Arc::downgrade(&inner);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(60));
                loop {
                    ticker.tick().await;
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    inner.handles.sweep(sweep_ttl);
                    drop(inner); // release strong ref between ticks
                }
            });
        }

        Ok(Self { inner })
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
    /// The handle store (all registries) is freed when the last `IrohEndpoint`
    /// clone is dropped — no explicit unregister is needed.
    pub async fn close(&self) {
        // ISS-027: drain in-flight requests *before* dropping so that request
        // handlers can still access their reader/writer/trailer handles
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
        self.inner.ep.close().await;
        let _ = self.inner.closed_tx.send(true);
    }

    /// Immediate close: abort the serve loop and close the endpoint with
    /// no drain period.
    pub async fn close_force(&self) {
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
        let _ = self.inner.closed_tx.send(true);
    }

    /// Wait until this endpoint has been closed (either explicitly via `close()` /
    /// `close_force()`, or because the native QUIC stack shut down).
    ///
    /// Returns immediately if already closed.
    pub async fn wait_closed(&self) {
        let mut rx = self.inner.closed_rx.clone();
        let _ = rx.wait_for(|v| *v).await;
    }

    /// Store a serve handle so that `close()` can drain it.
    pub fn set_serve_handle(&self, handle: ServeHandle) {
        *self
            .inner
            .serve_done_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(handle.subscribe_done());
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

    /// Wait until the serve loop has fully exited (serve task drained and finished).
    ///
    /// Returns immediately if `serve()` was never called.
    pub async fn wait_serve_stop(&self) {
        let rx = self
            .inner
            .serve_done_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(mut rx) = rx {
            // wait_for returns Err only if the sender is dropped; being dropped
            // also means the task has exited, so treat both outcomes as "done".
            let _ = rx.wait_for(|v| *v).await;
        }
    }

    pub fn raw(&self) -> &Endpoint {
        &self.inner.ep
    }

    /// Per-endpoint handle store.
    pub fn handles(&self) -> &HandleStore {
        &self.inner.handles
    }

    /// Maximum byte size of an HTTP/1.1 head.
    pub fn max_header_size(&self) -> usize {
        self.inner.max_header_size
    }

    /// Access the connection pool.
    pub(crate) fn pool(&self) -> &ConnectionPool {
        &self.inner.pool
    }

    /// Snapshot of current endpoint statistics.
    ///
    /// All fields are point-in-time reads and may change between calls.
    pub fn endpoint_stats(&self) -> EndpointStats {
        let (active_readers, active_writers, active_sessions, total_handles) =
            self.inner.handles.count_handles();
        let pool_size = self.inner.pool.entry_count_approx();
        let active_connections = self.inner.active_connections.load(Ordering::Relaxed);
        let active_requests = self.inner.active_requests.load(Ordering::Relaxed);
        EndpointStats {
            active_readers,
            active_writers,
            active_sessions,
            total_handles,
            pool_size,
            active_connections,
            active_requests,
        }
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

/// Endpoint-level observability snapshot.
///
/// Returned by [`IrohEndpoint::endpoint_stats`].  All counts are point-in-time reads
/// and may change between calls.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EndpointStats {
    /// Number of currently open body reader handles.
    pub active_readers: usize,
    /// Number of currently open body writer handles.
    pub active_writers: usize,
    /// Number of live QUIC sessions (WebTransport connections).
    pub active_sessions: usize,
    /// Total number of allocated (reader + writer + trailer + session + other) handles.
    pub total_handles: usize,
    /// Number of QUIC connections currently cached in the connection pool.
    pub pool_size: u64,
    /// Number of live QUIC connections accepted by the serve loop.
    pub active_connections: usize,
    /// Number of HTTP requests currently being processed.
    pub active_requests: usize,
}

/// A connection lifecycle event fired when a QUIC peer connection opens or closes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionEvent {
    /// Base32-encoded public key of the peer.
    pub peer_id: String,
    /// `true` when this is the first connection from the peer (0→1), `false` when the last one closes (1→0).
    pub connected: bool,
}

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
