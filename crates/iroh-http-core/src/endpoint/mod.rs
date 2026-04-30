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
//! public API surface.

pub(crate) mod ffi_bridge;
pub(crate) mod http_runtime;
pub(crate) mod session_runtime;
pub(crate) mod transport;

use iroh::address_lookup::{DnsAddressLookup, PkarrPublisher};
use iroh::endpoint::{Builder, IdleTimeout, QuicTransportConfig, TransportAddrUsage};
use iroh::{Endpoint, RelayMode, SecretKey};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use self::ffi_bridge::FfiBridge;
use self::http_runtime::HttpRuntime;
use self::session_runtime::SessionRuntime;
use self::transport::Transport;
use crate::ffi::handles::{HandleStore, StoreConfig};
use crate::http::server::ServeHandle;
use crate::http::transport::pool::ConnectionPool;
use crate::{ALPN, ALPN_DUPLEX};

pub use crate::config::{
    DiscoveryOptions, NetworkingOptions, NodeOptions, PoolOptions, StreamingOptions,
};
pub use crate::http::server::stack::CompressionOptions;
pub use crate::stats::{ConnectionEvent, EndpointStats, NodeAddrInfo, PathInfo, PeerStats};

/// A shared Iroh endpoint.
///
/// Clone-able (cheap Arc clone).  All fetch and serve calls on the same node
/// share one endpoint and therefore one stable QUIC identity.
#[derive(Clone)]
pub struct IrohEndpoint {
    pub(crate) inner: Arc<EndpointInner>,
}

/// Composition of the four ADR-014 D1 subsystems. No business logic; the
/// public API on [`IrohEndpoint`] reaches into the appropriate subsystem.
pub(crate) struct EndpointInner {
    pub transport: Transport,
    pub http: HttpRuntime,
    pub session: SessionRuntime,
    pub ffi: FfiBridge,
}

impl IrohEndpoint {
    /// Bind an Iroh endpoint with the supplied options.
    pub async fn bind(opts: NodeOptions) -> Result<Self, crate::CoreError> {
        // Validate: if networking is disabled, relay_mode should not be explicitly set to a
        // network-requiring mode.
        if opts.networking.disabled
            && opts
                .networking
                .relay_mode
                .as_deref()
                .is_some_and(|m| !matches!(m, "disabled"))
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

        // Validate capabilities against known ALPN values.
        for cap in &opts.capabilities {
            if !crate::KNOWN_ALPNS.contains(&cap.as_str()) {
                return Err(crate::CoreError::invalid_input(format!(
                    "unknown ALPN capability: \"{cap}\"; valid values are {:?}",
                    crate::KNOWN_ALPNS,
                )));
            }
        }

        // Validate compression level range (zstd accepts 1–22).
        if let Some(level) = opts.compression.as_ref().and_then(|c| c.level) {
            if !(1..=22).contains(&level) {
                return Err(crate::CoreError::invalid_input(format!(
                    "compression level must be 1–22, got {level}"
                )));
            }
        }

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

        let mut builder = Builder::new(iroh::endpoint::presets::Minimal)
            .relay_mode(relay_mode)
            .alpns(alpns);

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
                // Limit inbound bidirectional streams per connection to bound
                // slowloris-style resource exhaustion (P2-4).
                .max_concurrent_bidi_streams(128u32.into())
                .build();
            builder = builder.transport_config(transport);
        } else {
            // Even without a custom idle timeout, cap concurrent bidi streams.
            let transport = QuicTransportConfig::builder()
                .max_concurrent_bidi_streams(128u32.into())
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
                .unwrap_or(crate::ffi::handles::DEFAULT_CHANNEL_CAPACITY)
                .max(1),
            max_chunk_size: opts
                .streaming
                .max_chunk_size_bytes
                .unwrap_or(crate::ffi::handles::DEFAULT_MAX_CHUNK_SIZE)
                .max(1),
            drain_timeout: Duration::from_millis(
                opts.streaming
                    .drain_timeout_ms
                    .unwrap_or(crate::ffi::handles::DEFAULT_DRAIN_TIMEOUT_MS),
            ),
            max_handles: crate::ffi::handles::DEFAULT_MAX_HANDLES,
            ttl: Duration::from_millis(
                opts.streaming
                    .handle_ttl_ms
                    .unwrap_or(crate::ffi::handles::DEFAULT_SLAB_TTL_MS),
            ),
        };
        let sweep_ttl = store_config.ttl;
        let sweep_interval = Duration::from_millis(
            opts.streaming
                .sweep_interval_ms
                .unwrap_or(crate::ffi::handles::DEFAULT_SWEEP_INTERVAL_MS),
        );
        let (closed_tx, closed_rx) = tokio::sync::watch::channel(false);
        let (event_tx, event_rx) = tokio::sync::mpsc::channel::<crate::events::TransportEvent>(256);

        let inner = Arc::new(EndpointInner {
            transport: Transport { ep, node_id_str },
            http: HttpRuntime {
                pool: ConnectionPool::new(
                    opts.pool.max_connections,
                    opts.pool
                        .idle_timeout_ms
                        .map(std::time::Duration::from_millis),
                    Some(event_tx.clone()),
                ),
                // ISS-020: treat 0 as an error — callers should use `None` for the default.
                max_header_size: match opts.max_header_size {
                    Some(0) => {
                        return Err(crate::CoreError::invalid_input(
                            "max_header_size must be > 0; use None for the default (65536)",
                        ));
                    }
                    None => 64 * 1024,
                    Some(n) => n,
                },
                max_response_body_bytes: opts
                    .max_response_body_bytes
                    .unwrap_or(crate::http::server::DEFAULT_MAX_RESPONSE_BODY_BYTES),
                active_connections: Arc::new(AtomicUsize::new(0)),
                active_requests: Arc::new(AtomicUsize::new(0)),
                compression: opts.compression,
            },
            session: SessionRuntime {
                serve_handle: std::sync::Mutex::new(None),
                serve_done_rx: std::sync::Mutex::new(None),
                closed_tx,
                closed_rx,
                event_tx,
                event_rx: std::sync::Mutex::new(Some(event_rx)),
                path_subs: dashmap::DashMap::new(),
            },
            ffi: FfiBridge {
                handles: HandleStore::new(store_config),
            },
        });

        // Start per-endpoint sweep task (held alive via Weak reference).
        if !sweep_ttl.is_zero() {
            let weak = Arc::downgrade(&inner);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(sweep_interval);
                loop {
                    ticker.tick().await;
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    inner.ffi.handles.sweep(sweep_ttl);
                    drop(inner); // release strong ref between ticks
                }
            });
        }

        Ok(Self { inner })
    }

    /// The node's public key as a lowercase base32 string.
    pub fn node_id(&self) -> &str {
        &self.inner.transport.node_id_str
    }

    /// Immediately run a TTL sweep on all handle registries, evicting any
    /// entries whose TTL has expired.
    ///
    /// The background sweep task already runs this automatically on its
    /// configured interval. `sweep_now()` is provided for test fixtures and
    /// short-lived endpoints that cannot wait for the next tick.
    ///
    /// Returns immediately if the endpoint was created with `handle_ttl_ms: Some(0)`
    /// (sweeping disabled).
    pub fn sweep_now(&self) {
        let ttl = self.inner.ffi.handles.config.ttl;
        if !ttl.is_zero() {
            self.inner.ffi.handles.sweep(ttl);
        }
    }

    /// The node's raw secret key bytes (32 bytes).
    ///
    /// This is the Ed25519 private key that establishes the node's cryptographic identity.
    /// Use it **only** to persist and later restore the key via `NodeOptions::secret_key`.
    ///
    /// # Security
    ///
    /// **These 32 bytes are the irrecoverable private key for this node.**
    /// Anyone who obtains them can impersonate this node permanently — there is no revocation.
    ///
    /// - **Never log, print, or include in error payloads.** Debug formatters, tracing
    ///   spans, and generic error handlers are common accidental leak vectors.
    /// - **Encrypt at rest.** Store in a secrets vault or OS keychain, not in
    ///   plaintext config files or databases.
    /// - **Zeroize after use.** Call `zeroize::Zeroize::zeroize()` on the returned
    ///   array (or use a `secrecy`/`zeroize` wrapper) once you have persisted the bytes
    ///   to an encrypted store. The returned `[u8; 32]` is NOT automatically zeroed on drop.
    /// - **Never include in network responses, crash dumps, or analytics.**
    #[must_use]
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.inner.transport.ep.secret_key().to_bytes()
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

    /// Immediate close: abort the serve loop and close the endpoint with
    /// no drain period.
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

    /// Wait until this endpoint has been closed (either explicitly via `close()` /
    /// `close_force()`, or because the native QUIC stack shut down).
    ///
    /// Returns immediately if already closed.
    pub async fn wait_closed(&self) {
        let mut rx = self.inner.session.closed_rx.clone();
        let _ = rx.wait_for(|v| *v).await;
    }

    /// Store a serve handle so that `close()` can drain it.
    pub fn set_serve_handle(&self, handle: ServeHandle) {
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
    ///
    /// Returns immediately — does NOT close the endpoint or drain in-flight
    /// requests.  The handle is preserved so `close()` can still drain later.
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

    /// Wait until the serve loop has fully exited (serve task drained and finished).
    ///
    /// Returns immediately if `serve()` was never called.
    pub async fn wait_serve_stop(&self) {
        let rx = self
            .inner
            .session
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
        &self.inner.transport.ep
    }

    /// Per-endpoint handle store.
    pub fn handles(&self) -> &HandleStore {
        &self.inner.ffi.handles
    }

    /// Maximum byte size of an HTTP/1.1 head.
    pub fn max_header_size(&self) -> usize {
        self.inner.http.max_header_size
    }

    /// Maximum decompressed response-body bytes accepted per outgoing fetch.
    pub fn max_response_body_bytes(&self) -> usize {
        self.inner.http.max_response_body_bytes
    }

    /// Access the connection pool.
    pub(crate) fn pool(&self) -> &ConnectionPool {
        &self.inner.http.pool
    }

    /// Snapshot of current endpoint statistics.
    ///
    /// All fields are point-in-time reads and may change between calls.
    pub fn endpoint_stats(&self) -> EndpointStats {
        let (active_readers, active_writers, active_sessions, total_handles) =
            self.inner.ffi.handles.count_handles();
        let pool_size = self.inner.http.pool.entry_count_approx() as usize;
        let active_connections = self.inner.http.active_connections.load(Ordering::Relaxed);
        let active_requests = self.inner.http.active_requests.load(Ordering::Relaxed);
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
    pub fn compression(&self) -> Option<&CompressionOptions> {
        self.inner.http.compression.as_ref()
    }

    /// Returns the local socket addresses this endpoint is bound to.
    pub fn bound_sockets(&self) -> Vec<std::net::SocketAddr> {
        self.inner.transport.ep.bound_sockets()
    }

    /// Full node address: node ID + relay URL(s) + direct socket addresses.
    pub fn node_addr(&self) -> NodeAddrInfo {
        let addr = self.inner.transport.ep.addr();
        let mut addrs = Vec::new();
        for relay in addr.relay_urls() {
            addrs.push(relay.to_string());
        }
        for da in addr.ip_addrs() {
            addrs.push(da.to_string());
        }
        NodeAddrInfo {
            id: self.inner.transport.node_id_str.clone(),
            addrs,
        }
    }

    /// Home relay URL, or `None` if not connected to a relay.
    pub fn home_relay(&self) -> Option<String> {
        self.inner
            .transport
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
        let info = self.inner.transport.ep.remote_info(pk).await?;
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
        let info = self.inner.transport.ep.remote_info(pk).await?;

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
            if let Some(pooled) = self.inner.http.pool.get_existing(pk, crate::ALPN).await {
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

    /// Take the transport event receiver, handing it off to a platform drain task.
    ///
    /// May only be called once per endpoint.  The drain task owns the receiver and
    /// loops until `event_tx` is dropped (i.e. the endpoint closes).  Returns `None`
    /// if the receiver was already taken (i.e. `subscribe_events` was called before).
    pub fn subscribe_events(
        &self,
    ) -> Option<tokio::sync::mpsc::Receiver<crate::events::TransportEvent>> {
        self.inner
            .session
            .event_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// Subscribe to path changes for a specific peer.
    ///
    /// Spawns a background watcher task the first time a given peer is subscribed.
    /// The watcher polls `peer_stats()` every 200 ms and emits on the returned
    /// channel whenever the active path changes.
    pub fn subscribe_path_changes(
        &self,
        node_id_str: &str,
    ) -> tokio::sync::mpsc::UnboundedReceiver<PathInfo> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        // Replace any existing sender; old watcher exits when it detects is_closed().
        self.inner
            .session
            .path_subs
            .insert(node_id_str.to_string(), tx);

        let ep = self.clone();
        let nid = node_id_str.to_string();
        let event_tx = self.inner.session.event_tx.clone();

        tokio::spawn(async move {
            let mut last_key: Option<String> = None;
            let mut closed_rx = ep.inner.session.closed_rx.clone();
            loop {
                // Exit immediately if the endpoint has been closed.
                if *closed_rx.borrow() {
                    ep.inner.session.path_subs.remove(&nid);
                    break;
                }
                let is_closed = ep
                    .inner
                    .session
                    .path_subs
                    .get(&nid)
                    .map(|s| s.is_closed())
                    .unwrap_or(true);
                if is_closed {
                    ep.inner.session.path_subs.remove(&nid);
                    break;
                }

                if let Some(stats) = ep.peer_stats(&nid).await {
                    if let Some(active) = stats.paths.iter().find(|p| p.active) {
                        let key = format!("{}:{}", active.relay, active.addr);
                        if Some(&key) != last_key.as_ref() {
                            last_key = Some(key);
                            if let Some(sender) = ep.inner.session.path_subs.get(&nid) {
                                let _ = sender.send(active.clone());
                            }
                            let _ = event_tx.try_send(crate::events::TransportEvent::path_change(
                                &nid,
                                &active.addr,
                                active.relay,
                            ));
                        }
                    }
                }

                // Sleep 200 ms, but wake early if the endpoint is being closed.
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {}
                    result = closed_rx.wait_for(|v| *v) => {
                        let _ = result;
                        ep.inner.session.path_subs.remove(&nid);
                        break;
                    }
                }
            }
        });

        rx
    }
}

// ── Bind-error classification ────────────────────────────────────────────────

/// Classify a bind error into a prefixed string for the JS error mapper.
fn classify_bind_error(e: impl std::fmt::Display) -> crate::CoreError {
    let msg = e.to_string();
    crate::CoreError::connection_failed(msg)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

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
