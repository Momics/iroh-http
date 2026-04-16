//! napi-rs bindings for iroh-http-node.
//!
//! Exposes the full bridge interface to Node.js:
//! `createEndpoint`, `nextChunk`, `sendChunk`, `finishBody`,
//! `allocBodyWriter`, `rawFetch`, `rawServe`, `closeEndpoint`.

#![deny(clippy::all)]

use std::sync::Arc;
#[cfg(feature = "discovery")]
use std::sync::{Mutex, OnceLock};

use bytes::Bytes;
use iroh_http_core::{
    endpoint::{IrohEndpoint, NodeOptions},
    parse_direct_addrs, registry,
    server::respond,
    ConnectionEvent, DiscoveryOptions, NetworkingOptions, PoolOptions, RequestPayload,
    StreamingOptions,
};
use napi::{
    bindgen_prelude::{BigInt, *},
    threadsafe_function::{ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction},
    JsFunction,
};
use napi_derive::napi;

#[cfg(feature = "discovery")]
use slab::Slab;
#[cfg(feature = "discovery")]
use tokio::sync::Mutex as TokioMutex;

use iroh_http_adapter::{core_error_to_json, format_error_json};

// ── Endpoint helpers ──────────────────────────────────────────────────────────

fn get_endpoint(handle: u32) -> napi::Result<IrohEndpoint> {
    registry::get_endpoint(handle as u64).ok_or_else(|| {
        napi::Error::new(
            Status::InvalidArg,
            format_error_json(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {handle})"),
            ),
        )
    })
}

// ── Discovery slabs ───────────────────────────────────────────────────────────

#[cfg(feature = "discovery")]
type BrowseHandle = Arc<TokioMutex<iroh_http_discovery::BrowseSession>>;

#[cfg(feature = "discovery")]
fn browse_slab() -> &'static Mutex<Slab<BrowseHandle>> {
    static S: OnceLock<Mutex<Slab<BrowseHandle>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

#[cfg(feature = "discovery")]
fn advertise_slab() -> &'static Mutex<Slab<iroh_http_discovery::AdvertiseSession>> {
    static S: OnceLock<Mutex<Slab<iroh_http_discovery::AdvertiseSession>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

// ── Endpoint lifecycle ────────────────────────────────────────────────────────

/// Validate and convert an f64 option to a non-negative integer type.
fn safe_f64_to_u64(value: f64, field: &str) -> napi::Result<u64> {
    if value.is_nan() || value.is_infinite() || value < 0.0 {
        return Err(napi::Error::new(
            Status::InvalidArg,
            format!("{field}: expected a non-negative finite number, got {value}"),
        ));
    }
    Ok(value as u64)
}

fn safe_f64_to_usize(value: f64, field: &str) -> napi::Result<usize> {
    if value.is_nan() || value.is_infinite() || value < 0.0 {
        return Err(napi::Error::new(
            Status::InvalidArg,
            format!("{field}: expected a non-negative finite number, got {value}"),
        ));
    }
    Ok(value as usize)
}

#[napi(object)]
pub struct JsNodeOptions {
    pub key: Option<Uint8Array>,
    pub idle_timeout: Option<f64>,
    pub relay_mode: Option<String>,
    pub relays: Option<Vec<String>>,
    pub bind_addrs: Option<Vec<String>>,
    pub dns_discovery: Option<String>,
    pub dns_discovery_enabled: Option<bool>,
    pub channel_capacity: Option<u32>,
    pub max_chunk_size_bytes: Option<u32>,
    pub max_consecutive_errors: Option<u32>,
    pub drain_timeout: Option<f64>,
    pub handle_ttl: Option<f64>,
    pub max_pooled_connections: Option<u32>,
    pub pool_idle_timeout_ms: Option<f64>,
    pub disable_networking: Option<bool>,
    pub proxy_url: Option<String>,
    pub proxy_from_env: Option<bool>,
    pub keylog: Option<bool>,
    pub compression_level: Option<i32>,
    pub compression_min_body_bytes: Option<u32>,
    /// Maximum simultaneous in-flight requests.  Default: 64.
    pub max_concurrency: Option<u32>,
    /// Maximum connections from a single peer.  Default: 8.
    pub max_connections_per_peer: Option<u32>,
    /// Per-request timeout in milliseconds.  Default: 60 000.  0 = disabled.
    pub request_timeout: Option<f64>,
    /// Reject request bodies larger than this many bytes.  Default: unlimited.
    pub max_request_body_bytes: Option<f64>,
    /// Maximum header block size in bytes.  Default: 65536.
    pub max_header_bytes: Option<f64>,
    /// Maximum total QUIC connections the server will accept.  Default: unlimited.
    pub max_total_connections: Option<f64>,
}

/// Info returned after a successful `createEndpoint` call.
#[napi(object)]
pub struct JsEndpointInfo {
    /// Opaque handle for the endpoint — pass to all bridge functions.
    pub endpoint_handle: u32,
    /// Base32-encoded public key (stable node identity).
    pub node_id: String,
    /// 32-byte Ed25519 secret key — store to restore the same identity.
    pub keypair: Uint8Array,
}

/// Bind an Iroh QUIC endpoint and return a handle for subsequent operations.
///
/// This is the entry point for creating an iroh-http node. Pass `None` for
/// all-default configuration or supply a `JsNodeOptions` to customise.
#[napi]
pub async fn create_endpoint(options: Option<JsNodeOptions>) -> napi::Result<JsEndpointInfo> {
    let opts = options
        .map(|o| -> napi::Result<NodeOptions> {
            Ok(NodeOptions {
                key: match o.key {
                    Some(k) => {
                        let slice = k.as_ref();
                        let arr: [u8; 32] = slice.try_into().map_err(|_| {
                            napi::Error::new(
                                Status::InvalidArg,
                                format!("secret key must be exactly 32 bytes, got {}", slice.len()),
                            )
                        })?;
                        Some(arr)
                    }
                    None => None,
                },
                networking: NetworkingOptions {
                    relay_mode: o.relay_mode,
                    relays: o.relays.unwrap_or_default(),
                    bind_addrs: o.bind_addrs.unwrap_or_default(),
                    idle_timeout_ms: o.idle_timeout.map(|t| safe_f64_to_u64(t, "idleTimeout")).transpose()?,
                    proxy_url: o.proxy_url,
                    proxy_from_env: o.proxy_from_env.unwrap_or(false),
                    disabled: o.disable_networking.unwrap_or(false),
                },
                discovery: DiscoveryOptions {
                    dns_server: o.dns_discovery,
                    enabled: o.dns_discovery_enabled.unwrap_or(true),
                },
                pool: PoolOptions {
                    max_connections: o.max_pooled_connections.map(|v| v as usize),
                    idle_timeout_ms: o.pool_idle_timeout_ms.map(|v| safe_f64_to_u64(v, "poolIdleTimeoutMs")).transpose()?,
                },
                streaming: StreamingOptions {
                    channel_capacity: o.channel_capacity.map(|v| v as usize),
                    max_chunk_size_bytes: o.max_chunk_size_bytes.map(|v| v as usize),
                    drain_timeout_ms: o.drain_timeout.map(|v| safe_f64_to_u64(v, "drainTimeout")).transpose()?,
                    handle_ttl_ms: o.handle_ttl.map(|v| safe_f64_to_u64(v, "handleTtl")).transpose()?,
                },
                capabilities: Vec::new(),
                keylog: o.keylog.unwrap_or(false),
                max_header_size: o.max_header_bytes.map(|v| safe_f64_to_usize(v, "maxHeaderBytes")).transpose()?,
                server_limits: iroh_http_core::server::ServerLimits {
                    max_concurrency: o.max_concurrency.map(|v| v as usize),
                    max_connections_per_peer: o.max_connections_per_peer.map(|v| v as usize),
                    request_timeout_ms: o.request_timeout.map(|v| safe_f64_to_u64(v, "requestTimeout")).transpose()?,
                    max_request_body_bytes: o.max_request_body_bytes.map(|v| safe_f64_to_usize(v, "maxRequestBodyBytes")).transpose()?,
                    max_consecutive_errors: o.max_consecutive_errors.map(|v| v as usize),
                    drain_timeout_secs: None,
                    max_total_connections: o.max_total_connections.map(|v| safe_f64_to_usize(v, "maxTotalConnections")).transpose()?,
                },
                #[cfg(feature = "compression")]
                // NODE-003: enable compression when level or minBodyBytes is provided.
                compression: if o.compression_min_body_bytes.is_some()
                    || o.compression_level.is_some()
                {
                    // ISS-020: validate compression level range before cast.
                    if let Some(level) = o.compression_level {
                        if level < 0 {
                            return Err(napi::Error::new(
                                Status::InvalidArg,
                                format!("compressionLevel must be non-negative, got {level}"),
                            ));
                        }
                    }
                    Some(iroh_http_core::CompressionOptions {
                        min_body_bytes: o
                            .compression_min_body_bytes
                            .map(|v| v as usize)
                            .unwrap_or(512),
                        level: o.compression_level.map(|v| v as u32),
                    })
                } else {
                    None
                },
            })
        })
        .transpose()?
        .unwrap_or_default();

    let ep = IrohEndpoint::bind(opts)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;

    let node_id = ep.node_id().to_string();
    let keypair = ep.secret_key_bytes().to_vec();
    let handle = registry::insert_endpoint(ep) as u32;

    Ok(JsEndpointInfo {
        endpoint_handle: handle,
        node_id,
        keypair: Uint8Array::new(keypair),
    })
}

/// Close an Iroh endpoint.
///
/// If `force` is `true`, aborts immediately without draining in-flight
/// requests.  Otherwise performs a graceful shutdown.
#[napi]
pub async fn close_endpoint(endpoint_handle: u32, force: Option<bool>) -> napi::Result<()> {
    let ep = registry::remove_endpoint(endpoint_handle as u64).ok_or_else(|| {
        napi::Error::new(
            Status::InvalidArg,
            format_error_json("INVALID_HANDLE", "node closed or not found"),
        )
    })?;
    if force.unwrap_or(false) {
        ep.close_force().await;
    } else {
        ep.close().await;
    }
    Ok(())
}

// ── mDNS browse / advertise ──────────────────────────────────────────────────

/// Discovery event returned by `mdnsNextEvent`.
#[napi(object)]
pub struct JsPeerDiscoveryEvent {
    /// `true` = peer appeared; `false` = peer expired.
    pub is_active: bool,
    /// Base32 public key of the discovered peer.
    pub node_id: String,
    /// Known addresses: relay URLs and/or `ip:port` strings.
    pub addrs: Vec<String>,
}

/// Start a browse session: discover peers on the local network via mDNS.
/// Returns a browse handle for polling events.
#[napi]
#[cfg(feature = "discovery")]
pub async fn mdns_browse(endpoint_handle: u32, service_name: String) -> napi::Result<u32> {
    let ep = get_endpoint(endpoint_handle)?;
    let session = iroh_http_discovery::start_browse(ep.raw(), &service_name)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, format_error_json("REFUSED", e)))?;
    let handle = browse_slab()
        .lock()
        .unwrap()
        .insert(Arc::new(TokioMutex::new(session))) as u32;
    Ok(handle)
}

#[napi]
#[cfg(not(feature = "discovery"))]
pub async fn mdns_browse(_endpoint_handle: u32, _service_name: String) -> napi::Result<u32> {
    Err(napi::Error::new(
        Status::GenericFailure,
        format_error_json("UNKNOWN", "discovery feature not enabled in this build"),
    ))
}

/// Poll the next discovery event from a browse session.
/// Returns `null` when the session is closed.
#[napi]
#[cfg(feature = "discovery")]
pub async fn mdns_next_event(browse_handle: u32) -> napi::Result<Option<JsPeerDiscoveryEvent>> {
    let session = {
        browse_slab()
            .lock()
            .unwrap()
            .get(browse_handle as usize)
            .cloned()
    }
    .ok_or_else(|| {
        napi::Error::new(
            Status::InvalidArg,
            format_error_json(
                "INVALID_HANDLE",
                format!("invalid browse handle: {browse_handle}"),
            ),
        )
    })?;
    let event = session.lock().await.next_event().await;
    Ok(event.map(|ev| JsPeerDiscoveryEvent {
        is_active: ev.is_active,
        node_id: ev.node_id,
        addrs: ev.addrs,
    }))
}

#[napi]
#[cfg(not(feature = "discovery"))]
pub async fn mdns_next_event(_browse_handle: u32) -> napi::Result<Option<JsPeerDiscoveryEvent>> {
    Err(napi::Error::new(
        Status::GenericFailure,
        format_error_json("UNKNOWN", "discovery feature not enabled in this build"),
    ))
}

/// Close a browse session, stopping mDNS discovery.
#[napi]
#[cfg(feature = "discovery")]
pub fn mdns_browse_close(browse_handle: u32) {
    let mut slab = browse_slab().lock().unwrap();
    if slab.contains(browse_handle as usize) {
        slab.remove(browse_handle as usize);
    }
}

#[napi]
#[cfg(not(feature = "discovery"))]
pub fn mdns_browse_close(_browse_handle: u32) {}

/// Start advertising this node on the local network via mDNS.
/// Returns an advertise handle.
#[napi]
#[cfg(feature = "discovery")]
pub fn mdns_advertise(endpoint_handle: u32, service_name: String) -> napi::Result<u32> {
    let ep = get_endpoint(endpoint_handle)?;
    let session = iroh_http_discovery::start_advertise(ep.raw(), &service_name)
        .map_err(|e| napi::Error::new(Status::GenericFailure, format_error_json("REFUSED", e)))?;
    let handle = advertise_slab().lock().unwrap().insert(session) as u32;
    Ok(handle)
}

#[napi]
#[cfg(not(feature = "discovery"))]
pub fn mdns_advertise(_endpoint_handle: u32, _service_name: String) -> napi::Result<u32> {
    Err(napi::Error::new(
        Status::GenericFailure,
        format_error_json("UNKNOWN", "discovery feature not enabled in this build"),
    ))
}

/// Stop advertising this node on the local network.
#[napi]
#[cfg(feature = "discovery")]
pub fn mdns_advertise_close(advertise_handle: u32) {
    let mut slab = advertise_slab().lock().unwrap();
    if slab.contains(advertise_handle as usize) {
        slab.remove(advertise_handle as usize);
    }
}

#[napi]
#[cfg(not(feature = "discovery"))]
pub fn mdns_advertise_close(_advertise_handle: u32) {}

// ── Bridge methods ────────────────────────────────────────────────────────────

// ── Address introspection ─────────────────────────────────────────────────────

#[napi(object)]
pub struct JsNodeAddrInfo {
    pub id: String,
    pub addrs: Vec<String>,
}

#[napi(object)]
pub struct JsPathInfo {
    pub relay: bool,
    pub addr: String,
    pub active: bool,
}

#[napi(object)]
pub struct JsPeerStats {
    pub relay: bool,
    pub relay_url: Option<String>,
    pub paths: Vec<JsPathInfo>,
    pub rtt_ms: Option<f64>,
    pub bytes_sent: Option<i64>,
    pub bytes_received: Option<i64>,
    pub lost_packets: Option<i64>,
    pub sent_packets: Option<i64>,
    pub congestion_window: Option<i64>,
}

/// Full node address: node ID + relay URL(s) + direct socket addresses.
#[napi]
pub fn node_addr(endpoint_handle: u32) -> napi::Result<JsNodeAddrInfo> {
    let ep = get_endpoint(endpoint_handle)?;
    let info = ep.node_addr();
    Ok(JsNodeAddrInfo {
        id: info.id,
        addrs: info.addrs,
    })
}

/// Generate a ticket string for the given endpoint.
///
/// The ticket encodes the node ID and all known addresses (relay URLs + direct IPs).
/// Share with peers so they can connect directly.
#[napi]
pub fn node_ticket(endpoint_handle: u32) -> napi::Result<String> {
    let ep = get_endpoint(endpoint_handle)?;
    iroh_http_core::node_ticket(&ep)
        .map_err(|e| napi::Error::new(Status::GenericFailure, e.message))
}

/// Home relay URL, or null if not connected to a relay.
#[napi]
pub fn home_relay(endpoint_handle: u32) -> napi::Result<Option<String>> {
    let ep = get_endpoint(endpoint_handle)?;
    Ok(ep.home_relay())
}

/// Known addresses for a remote peer, or null if unknown.
#[napi]
pub async fn peer_info(
    endpoint_handle: u32,
    node_id: String,
) -> napi::Result<Option<JsNodeAddrInfo>> {
    let ep = get_endpoint(endpoint_handle)?;
    Ok(ep.peer_info(&node_id).await.map(|info| JsNodeAddrInfo {
        id: info.id,
        addrs: info.addrs,
    }))
}

/// Per-peer connection statistics with path information.
#[napi]
pub async fn peer_stats(
    endpoint_handle: u32,
    node_id: String,
) -> napi::Result<Option<JsPeerStats>> {
    let ep = get_endpoint(endpoint_handle)?;
    Ok(ep.peer_stats(&node_id).await.map(|s| JsPeerStats {
        relay: s.relay,
        relay_url: s.relay_url,
        paths: s
            .paths
            .into_iter()
            .map(|p| JsPathInfo {
                relay: p.relay,
                addr: p.addr,
                active: p.active,
            })
            .collect(),
        rtt_ms: s.rtt_ms,
        bytes_sent: s.bytes_sent.map(|v| v as i64),
        bytes_received: s.bytes_received.map(|v| v as i64),
        lost_packets: s.lost_packets.map(|v| v as i64),
        sent_packets: s.sent_packets.map(|v| v as i64),
        congestion_window: s.congestion_window.map(|v| v as i64),
    }))
}

/// Endpoint-level observability snapshot.
#[napi(object)]
pub struct JsEndpointStats {
    pub active_readers: i64,
    pub active_writers: i64,
    pub active_sessions: i64,
    pub total_handles: i64,
    pub pool_size: i64,
    pub active_connections: i64,
    pub active_requests: i64,
}

/// Snapshot of current endpoint statistics (point-in-time).
#[napi]
pub fn endpoint_stats(endpoint_handle: u32) -> napi::Result<JsEndpointStats> {
    let ep = get_endpoint(endpoint_handle)?;
    let s = ep.endpoint_stats();
    Ok(JsEndpointStats {
        active_readers: s.active_readers as i64,
        active_writers: s.active_writers as i64,
        active_sessions: s.active_sessions as i64,
        total_handles: s.total_handles as i64,
        pool_size: s.pool_size as i64,
        active_connections: s.active_connections as i64,
        active_requests: s.active_requests as i64,
    })
}

// ── Body streaming ────────────────────────────────────────────────────────────

/// Read the next chunk from a body reader handle.
///
/// Returns `null` at EOF. The handle is automatically cleaned up after EOF.
#[napi]
pub async fn js_next_chunk(endpoint_handle: u32, handle: BigInt) -> napi::Result<Option<Buffer>> {
    let ep = get_endpoint(endpoint_handle)?;
    let chunk = ep
        .handles()
        .next_chunk(handle.get_u64().1)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;
    Ok(chunk.map(|b| Buffer::from(b.to_vec())))
}

/// Push a chunk into a body writer handle.
///
/// Large chunks are automatically split to stay within backpressure limits.
#[napi]
pub async fn js_send_chunk(
    endpoint_handle: u32,
    handle: BigInt,
    chunk: Uint8Array,
) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    let bytes = Bytes::from(chunk.to_vec());
    ep.handles()
        .send_chunk(handle.get_u64().1, bytes)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))
}

/// Signal end-of-body by dropping the writer.
///
/// The paired `BodyReader` will return `null` on its next `nextChunk` call.
#[napi]
pub fn js_finish_body(endpoint_handle: u32, handle: BigInt) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    ep.handles()
        .finish_body(handle.get_u64().1)
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))
}

/// Cancel a body reader, causing any pending `nextChunk` to return null.
#[napi]
pub fn js_cancel_request(endpoint_handle: u32, handle: BigInt) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    ep.handles().cancel_reader(handle.get_u64().1);
    Ok(())
}

/// Await and retrieve trailer headers from a completed request/response.
///
/// Returns `null` if no trailers were sent.
#[napi]
pub async fn js_next_trailer(
    endpoint_handle: u32,
    handle: BigInt,
) -> napi::Result<Option<Vec<Vec<String>>>> {
    let ep = get_endpoint(endpoint_handle)?;
    let trailers = ep
        .handles()
        .next_trailer(handle.get_u64().1)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;
    Ok(trailers.map(|t| t.into_iter().map(|(k, v)| vec![k, v]).collect()))
}

/// Deliver response trailer headers to the Rust pump task.
#[napi]
pub fn js_send_trailers(
    endpoint_handle: u32,
    handle: BigInt,
    trailers: Vec<Vec<String>>,
) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    let pairs: Vec<(String, String)> = trailers
        .into_iter()
        .filter_map(|p| {
            if p.len() == 2 {
                Some((p[0].clone(), p[1].clone()))
            } else {
                None
            }
        })
        .collect();
    ep.handles()
        .send_trailers(handle.get_u64().1, pairs)
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))
}

/// Allocate a body writer handle for streaming request bodies.
///
/// Call this before `rawFetch` to get a handle that can be written to
/// with `sendChunk` / `finishBody`.
#[napi]
pub fn js_alloc_body_writer(endpoint_handle: u32) -> napi::Result<u64> {
    let ep = get_endpoint(endpoint_handle)?;
    let (handle, reader) = ep
        .handles()
        .alloc_body_writer()
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;
    ep.handles().store_pending_reader(handle, reader);
    Ok(handle)
}

/// Allocate a request trailer sender handle for use with `rawFetch`.
///
/// Call this before `rawFetch` when you want to send request trailers.
/// Pass the returned handle as `reqTrailersHandle` to `rawFetch`, then
/// call `sendTrailers(handle, trailers)` after the request body is finished.
#[napi]
pub fn js_alloc_trailer_sender(endpoint_handle: u32) -> napi::Result<u64> {
    let ep = get_endpoint(endpoint_handle)?;
    ep.handles()
        .alloc_trailer_sender()
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))
}

/// Allocate a cancellation token for an upcoming `rawFetch` call.
///
/// Wire `AbortSignal → cancelInFlight(token)` for request cancellation.
#[napi]
pub fn js_alloc_fetch_token(endpoint_handle: u32) -> napi::Result<u64> {
    let ep = get_endpoint(endpoint_handle)?;
    ep.handles()
        .alloc_fetch_token()
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))
}

/// Cancel an in-flight fetch by its cancellation token.
///
/// Safe to call after the fetch has already completed (no-op).
#[napi]
pub fn js_cancel_in_flight(endpoint_handle: u32, token: BigInt) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    ep.handles().cancel_in_flight(token.get_u64().1);
    Ok(())
}

// ── rawFetch ──────────────────────────────────────────────────────────────────

/// Raw response returned by `rawFetch`.
///
/// The shared TS layer wraps this into a web-standard `Response`.
#[napi(object)]
pub struct JsFfiResponse {
    /// HTTP status code.
    pub status: u32,
    /// Response headers as `[[key, value], ...]`.
    pub headers: Vec<Vec<String>>,
    /// Handle to the response body reader (`nextChunk`).
    pub body_handle: BigInt,
    /// Full `httpi://` URL of the responding peer.
    pub url: String,
    /// Handle to await response trailer headers.
    pub trailers_handle: BigInt,
}

/// Send an HTTP request to a remote Iroh peer.
///
/// Low-level function — the shared TS layer wraps this in `makeFetch`.
#[napi]
#[allow(clippy::too_many_arguments)]
pub async fn raw_fetch(
    endpoint_handle: u32,
    node_id: String,
    url: String,
    method: String,
    headers: Vec<Vec<String>>,
    req_body_handle: Option<BigInt>,
    req_trailers_handle: Option<BigInt>,
    fetch_token: BigInt,
    direct_addrs: Option<Vec<String>>,
) -> napi::Result<JsFfiResponse> {
    let ep = get_endpoint(endpoint_handle)?;

    let pairs: Vec<(String, String)> = headers
        .into_iter()
        .filter_map(|pair| {
            if pair.len() == 2 {
                Some((pair[0].clone(), pair[1].clone()))
            } else {
                None
            }
        })
        .collect();

    let req_body_reader =
        req_body_handle.and_then(|h| ep.handles().claim_pending_reader(h.get_u64().1));

    let req_trailer_sender_handle = req_trailers_handle.map(|h| h.get_u64().1);

    let addrs =
        parse_direct_addrs(&direct_addrs).map_err(|e| napi::Error::new(Status::InvalidArg, e))?;
    let res = iroh_http_core::fetch(
        &ep,
        &node_id,
        &url,
        &method,
        &pairs,
        req_body_reader,
        req_trailer_sender_handle,
        Some(fetch_token.get_u64().1),
        addrs.as_deref(),
    )
    .await
    .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;

    let resp_headers: Vec<Vec<String>> = res.headers.into_iter().map(|(k, v)| vec![k, v]).collect();

    Ok(JsFfiResponse {
        status: res.status as u32,
        headers: resp_headers,
        body_handle: BigInt::from(res.body_handle),
        url: res.url,
        trailers_handle: BigInt::from(res.trailers_handle),
    })
}

// ── rawServe / rawRespond ──────────────────────────────────────────────────────

/// Call once per request from the JS handler to send the response head.
///
/// This is the Node.js equivalent of Tauri's `respond_to_request` command.
/// The handler callback in `rawServe` is fire-and-forget (napi-rs does not
/// support awaiting Promise return values from ThreadsafeFunction callbacks),
/// so JS must call `rawRespond` explicitly after computing the response head.
#[napi]
pub fn raw_respond(
    endpoint_handle: u32,
    req_handle: BigInt,
    status: u32,
    headers: Vec<Vec<String>>,
) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    let header_pairs: Vec<(String, String)> = headers
        .into_iter()
        .filter_map(|p| {
            if p.len() == 2 {
                Some((p[0].clone(), p[1].clone()))
            } else {
                None
            }
        })
        .collect();
    respond(
        ep.handles(),
        req_handle.get_u64().1,
        status as u16,
        header_pairs,
    )
    .map_err(|e| napi::Error::new(Status::GenericFailure, e))
}

#[napi]
pub fn raw_serve(
    endpoint_handle: u32,
    handler: JsFunction,
    on_connection_event: Option<JsFunction>,
) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;

    type CallArgs = RequestPayload;
    // Use ErrorStrategy::Fatal but do NOT rely on the return value — the JS
    // handler is async and calls `rawRespond` explicitly when ready.
    let tsfn: ThreadsafeFunction<CallArgs, ErrorStrategy::Fatal> = handler
        .create_threadsafe_function(0, |ctx: ThreadSafeCallContext<CallArgs>| {
            let env = ctx.env;
            let p = ctx.value;

            let mut obj = env.create_object()?;
            obj.set("reqHandle", env.create_bigint_from_u64(p.req_handle)?)?;
            obj.set(
                "reqBodyHandle",
                env.create_bigint_from_u64(p.req_body_handle)?,
            )?;
            obj.set(
                "resBodyHandle",
                env.create_bigint_from_u64(p.res_body_handle)?,
            )?;
            obj.set(
                "reqTrailersHandle",
                env.create_bigint_from_u64(p.req_trailers_handle)?,
            )?;
            obj.set(
                "resTrailersHandle",
                env.create_bigint_from_u64(p.res_trailers_handle)?,
            )?;
            obj.set("isBidi", env.get_boolean(p.is_bidi)?)?;
            obj.set("method", env.create_string(&p.method)?)?;
            obj.set("url", env.create_string(&p.url)?)?;
            obj.set("remoteNodeId", env.create_string(&p.remote_node_id)?)?;

            let mut headers_arr = env.create_array(p.headers.len() as u32)?;
            for (i, (k, v)) in p.headers.iter().enumerate() {
                let mut pair = env.create_array(2)?;
                pair.set(0, env.create_string(k)?)?;
                pair.set(1, env.create_string(v)?)?;
                headers_arr.set(i as u32, pair)?;
            }
            obj.set("headers", headers_arr)?;

            Ok(vec![obj.into_unknown()])
        })?;

    let tsfn = Arc::new(tsfn);

    // Build an optional TSFN for connection events (peer connect/disconnect).
    let conn_event_fn: Option<Arc<dyn Fn(ConnectionEvent) + Send + Sync>> =
        if let Some(cb) = on_connection_event {
            let conn_tsfn: ThreadsafeFunction<ConnectionEvent, ErrorStrategy::Fatal> = cb
                .create_threadsafe_function(0, |ctx: ThreadSafeCallContext<ConnectionEvent>| {
                    let env = ctx.env;
                    let ev = ctx.value;
                    let mut obj = env.create_object()?;
                    obj.set("peerId", env.create_string(&ev.peer_id)?)?;
                    obj.set("connected", env.get_boolean(ev.connected)?)?;
                    Ok(vec![obj.into_unknown()])
                })?;
            Some(Arc::new(move |ev: ConnectionEvent| {
                conn_tsfn.call(
                    ev,
                    napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking,
                );
            }))
        } else {
            None
        };

    let ep_clone = ep.clone();
    let handle = iroh_http_core::serve_with_events(
        ep.clone(),
        ep.serve_options(),
        move |payload: RequestPayload| {
            let tsfn = Arc::clone(&tsfn);
            let ep_ref = ep_clone.clone();
            let req_handle = payload.req_handle;
            // ISS-019: check TSFN enqueue status; on failure respond with 503
            // immediately so the request doesn't stall until timeout.
            let status = tsfn.call(
                payload,
                napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking,
            );
            if status != napi::Status::Ok {
                tracing::warn!("iroh-http-node: TSFN enqueue failed ({status:?}), responding 503");
                let _ = ep_ref.handles().take_req_sender(req_handle).map(|tx| {
                    let _ = tx.send(iroh_http_core::stream::ResponseHeadEntry {
                        status: 503,
                        headers: vec![("content-length".to_string(), "0".to_string())],
                    });
                });
            }
        },
        conn_event_fn,
    );
    ep.set_serve_handle(handle);

    Ok(())
}

/// Stop the serve loop for the given endpoint (graceful shutdown).
///
/// This signals the accept loop to stop but does NOT close the endpoint or
/// drain in-flight requests.  Call `closeEndpoint` afterwards if you want
/// a full teardown.
#[napi]
pub fn stop_serve(endpoint_handle: u32) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    ep.stop_serve();
    Ok(())
}

/// Wait until the serve loop has fully exited (all in-flight requests drained).
///
/// Resolves immediately if `rawServe` was never called on this endpoint.
/// Call this after `stopServe` to confirm the loop has actually terminated.
#[napi]
pub async fn wait_serve_stop(endpoint_handle: u32) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    ep.wait_serve_stop().await;
    Ok(())
}

/// Wait until this endpoint has been fully closed — either because `closeEndpoint()`
/// was called or because the QUIC stack shut down natively.
///
/// This is used to surface `node.closed` reliably even without an explicit `close()`.
#[napi]
pub async fn wait_endpoint_closed(endpoint_handle: u32) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    ep.wait_closed().await;
    Ok(())
}

// ── rawConnect ────────────────────────────────────────────────────────────────

/// Handles for a full-duplex QUIC stream.
#[napi(object)]
pub struct JsFfiDuplexStream {
    /// Body reader handle — call `nextChunk(readHandle)` to receive data.
    pub read_handle: BigInt,
    /// Body writer handle — call `sendChunk(writeHandle, …)` to send data.
    pub write_handle: BigInt,
}

/// Open a full-duplex connection to a remote node.
#[napi]
pub async fn raw_connect(
    endpoint_handle: u32,
    node_id: String,
    path: String,
    headers: Vec<Vec<String>>,
) -> napi::Result<JsFfiDuplexStream> {
    let ep = get_endpoint(endpoint_handle)?;

    let pairs: Vec<(String, String)> = headers
        .into_iter()
        .filter_map(|p| {
            if p.len() == 2 {
                Some((p[0].clone(), p[1].clone()))
            } else {
                None
            }
        })
        .collect();

    let duplex = iroh_http_core::raw_connect(&ep, &node_id, &path, &pairs)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;

    Ok(JsFfiDuplexStream {
        read_handle: BigInt::from(duplex.read_handle),
        write_handle: BigInt::from(duplex.write_handle),
    })
}

// ── Session ───────────────────────────────────────────────────────────────────

/// Establish a session (QUIC connection) to a remote peer.
/// Returns an opaque session handle.
#[napi]
pub async fn session_connect(
    endpoint_handle: u32,
    node_id: String,
    direct_addrs: Option<Vec<String>>,
) -> napi::Result<u64> {
    let ep = get_endpoint(endpoint_handle)?;
    let addrs =
        parse_direct_addrs(&direct_addrs).map_err(|e| napi::Error::new(Status::InvalidArg, e))?;
    let handle = iroh_http_core::session_connect(&ep, &node_id, addrs.as_deref())
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;
    Ok(handle)
}

/// Open a new bidirectional stream on an existing session.
#[napi(object)]
pub struct JsSessionBidiStream {
    pub read_handle: BigInt,
    pub write_handle: BigInt,
}

#[napi]
pub async fn session_create_bidi_stream(
    endpoint_handle: u32,
    session_handle: BigInt,
) -> napi::Result<JsSessionBidiStream> {
    let ep = get_endpoint(endpoint_handle)?;
    let duplex = iroh_http_core::session_create_bidi_stream(&ep, session_handle.get_u64().1)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;
    Ok(JsSessionBidiStream {
        read_handle: BigInt::from(duplex.read_handle),
        write_handle: BigInt::from(duplex.write_handle),
    })
}

/// Accept the next incoming bidirectional stream on a session.
/// Returns null when the session is closed.
#[napi]
pub async fn session_next_bidi_stream(
    endpoint_handle: u32,
    session_handle: BigInt,
) -> napi::Result<Option<JsSessionBidiStream>> {
    let ep = get_endpoint(endpoint_handle)?;
    let result = iroh_http_core::session_next_bidi_stream(&ep, session_handle.get_u64().1)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;
    Ok(result.map(|d| JsSessionBidiStream {
        read_handle: BigInt::from(d.read_handle),
        write_handle: BigInt::from(d.write_handle),
    }))
}

/// Close a session.
#[napi]
pub async fn session_close_handle(
    endpoint_handle: u32,
    session_handle: BigInt,
    close_code: Option<BigInt>,
    reason: Option<String>,
) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    iroh_http_core::session_close(
        &ep,
        session_handle.get_u64().1,
        close_code.map(|c| c.get_u64().1).unwrap_or(0),
        reason.as_deref().unwrap_or(""),
    )
    .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))
}

/// Wait for a session to close. Returns close info { closeCode, reason }.
#[napi(object)]
pub struct JsCloseInfo {
    pub close_code: BigInt,
    pub reason: String,
}

#[napi]
pub async fn session_closed(
    endpoint_handle: u32,
    session_handle: BigInt,
) -> napi::Result<JsCloseInfo> {
    let ep = get_endpoint(endpoint_handle)?;
    let info = iroh_http_core::session_closed(&ep, session_handle.get_u64().1)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;
    Ok(JsCloseInfo {
        close_code: BigInt::from(info.close_code),
        reason: info.reason,
    })
}

/// Open a new unidirectional (send-only) stream on a session.
/// Returns a write handle.
#[napi]
pub async fn session_create_uni_stream(
    endpoint_handle: u32,
    session_handle: BigInt,
) -> napi::Result<u64> {
    let ep = get_endpoint(endpoint_handle)?;
    iroh_http_core::session_create_uni_stream(&ep, session_handle.get_u64().1)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))
}

/// Accept the next incoming unidirectional stream on a session.
/// Returns a read handle, or null when the session is closed.
#[napi]
pub async fn session_next_uni_stream(
    endpoint_handle: u32,
    session_handle: BigInt,
) -> napi::Result<Option<u64>> {
    let ep = get_endpoint(endpoint_handle)?;
    iroh_http_core::session_next_uni_stream(&ep, session_handle.get_u64().1)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))
}

/// Send a datagram on a session.
#[napi]
pub async fn session_send_datagram(
    endpoint_handle: u32,
    session_handle: BigInt,
    data: Uint8Array,
) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;
    iroh_http_core::session_send_datagram(&ep, session_handle.get_u64().1, data.as_ref())
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))
}

/// Receive the next datagram on a session. Returns null when the session closes.
#[napi]
pub async fn session_recv_datagram(
    endpoint_handle: u32,
    session_handle: BigInt,
) -> napi::Result<Option<Buffer>> {
    let ep = get_endpoint(endpoint_handle)?;
    let result = iroh_http_core::session_recv_datagram(&ep, session_handle.get_u64().1)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;
    Ok(result.map(Buffer::from))
}

/// Get the maximum datagram payload size for a session.
/// Returns null if datagrams are not supported.
#[napi]
pub fn session_max_datagram_size(
    endpoint_handle: u32,
    session_handle: BigInt,
) -> napi::Result<Option<u32>> {
    let ep = get_endpoint(endpoint_handle)?;
    let result = iroh_http_core::session_max_datagram_size(&ep, session_handle.get_u64().1)
        .map_err(|e| napi::Error::new(Status::GenericFailure, core_error_to_json(&e)))?;
    Ok(result.map(|s| s as u32))
}

// ── Key operations ────────────────────────────────────────────────────────────

/// Sign arbitrary bytes with a 32-byte Ed25519 secret key.
/// Returns a 64-byte signature as a `Buffer`.
#[napi]
pub fn secret_key_sign(secret_key: Uint8Array, data: Uint8Array) -> napi::Result<Buffer> {
    let key_bytes: [u8; 32] = secret_key
        .as_ref()
        .try_into()
        .map_err(|_| napi::Error::new(Status::InvalidArg, "secret key must be 32 bytes"))?;
    let sig = iroh_http_core::secret_key_sign(&key_bytes, data.as_ref())
        .map_err(|e| napi::Error::new(Status::GenericFailure, e.to_string()))?;
    Ok(Buffer::from(sig.to_vec()))
}

/// Verify a 64-byte Ed25519 signature against a 32-byte public key.
/// Returns `true` on success, `false` on failure — does not throw.
#[napi]
pub fn public_key_verify(public_key: Uint8Array, data: Uint8Array, signature: Uint8Array) -> bool {
    let Ok(key_bytes) = <[u8; 32]>::try_from(public_key.as_ref()) else {
        return false;
    };
    let Ok(sig_bytes) = <[u8; 64]>::try_from(signature.as_ref()) else {
        return false;
    };
    iroh_http_core::public_key_verify(&key_bytes, data.as_ref(), &sig_bytes)
}

/// Generate a fresh Ed25519 secret key. Returns 32 raw bytes.
#[napi]
pub fn generate_secret_key() -> napi::Result<Buffer> {
    let key = iroh_http_core::generate_secret_key()
        .map_err(|e| napi::Error::new(Status::GenericFailure, e.to_string()))?;
    Ok(Buffer::from(key.to_vec()))
}
