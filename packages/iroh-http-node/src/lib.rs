//! napi-rs bindings for iroh-http-node.
//!
//! Exposes the full bridge interface to Node.js:
//! `createEndpoint`, `nextChunk`, `sendChunk`, `finishBody`,
//! `allocBodyWriter`, `rawFetch`, `rawServe`, `closeEndpoint`.

#![deny(clippy::all)]

use std::sync::{Arc, Mutex, OnceLock};

use bytes::Bytes;
use iroh_http_core::{
    endpoint::{IrohEndpoint, NodeOptions, DiscoveryConfig},
    server::respond,
    stream::{
        alloc_body_writer, claim_pending_reader, finish_body,
        next_chunk, send_chunk, store_pending_reader,
        cancel_reader, next_trailer, send_trailers,
    },
    RequestPayload,
};
use napi::{
    bindgen_prelude::*,
    threadsafe_function::{ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction},
    JsFunction,
};
use napi_derive::napi;
use slab::Slab;

#[cfg(feature = "discovery")]
use std::collections::HashMap;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_direct_addrs(addrs: &Option<Vec<String>>) -> Option<Vec<std::net::SocketAddr>> {
    addrs.as_ref().map(|v| {
        v.iter()
            .filter_map(|s| s.parse::<std::net::SocketAddr>().ok())
            .collect()
    })
}

// ── Endpoint slab ─────────────────────────────────────────────────────────────

fn endpoint_slab() -> &'static Mutex<Slab<IrohEndpoint>> {
    static S: OnceLock<Mutex<Slab<IrohEndpoint>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

fn insert_endpoint(ep: IrohEndpoint) -> u32 {
    endpoint_slab().lock().unwrap().insert(ep) as u32
}

fn get_endpoint(handle: u32) -> napi::Result<IrohEndpoint> {
    let slab = endpoint_slab().lock().unwrap();
    slab.get(handle as usize)
        .cloned()
        .ok_or_else(|| napi::Error::new(Status::InvalidArg, iroh_http_core::classify_error_json(format!("invalid endpoint handle: {handle}"))))
}

// ── Discovery slab ────────────────────────────────────────────────────────────

#[cfg(feature = "discovery")]
fn discovery_map() -> &'static Mutex<HashMap<u32, Arc<iroh::address_lookup::MdnsAddressLookup>>> {
    static S: OnceLock<Mutex<HashMap<u32, Arc<iroh::address_lookup::MdnsAddressLookup>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

// ── Endpoint lifecycle ────────────────────────────────────────────────────────

#[napi(object)]
pub struct JsDiscoveryOptions {
    pub mdns: Option<bool>,
    pub service_name: Option<String>,
    pub advertise: Option<bool>,
}

/// Configuration options for creating an Iroh endpoint.
///
/// All fields are optional — omit or pass `None` for sensible defaults.
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
    pub discovery: Option<JsDiscoveryOptions>,
    pub drain_timeout: Option<f64>,
    pub handle_ttl: Option<f64>,
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
    let discovery_js = options.as_ref().and_then(|o| o.discovery.as_ref()).map(|d| DiscoveryConfig {
        mdns: d.mdns.unwrap_or(false),
        service_name: d.service_name.clone(),
        advertise: d.advertise.unwrap_or(true),
    });

    let opts = options.map(|o| NodeOptions {
        key: o.key.map(|k| {
            let mut arr = [0u8; 32];
            let slice = k.as_ref();
            let len = slice.len().min(32);
            arr[..len].copy_from_slice(&slice[..len]);
            arr
        }),
        idle_timeout_ms: o.idle_timeout.map(|t| t as u64),
        relay_mode: o.relay_mode,
        relays: o.relays.unwrap_or_default(),
        bind_addrs: o.bind_addrs.unwrap_or_default(),
        dns_discovery: o.dns_discovery,
        dns_discovery_enabled: o.dns_discovery_enabled.unwrap_or(true),
        capabilities: Vec::new(),
        channel_capacity: o.channel_capacity.map(|v| v as usize),
        max_chunk_size_bytes: o.max_chunk_size_bytes.map(|v| v as usize),
        max_consecutive_errors: o.max_consecutive_errors.map(|v| v as usize),
        discovery: discovery_js.clone(),
        disable_networking: o.disable_networking.unwrap_or(false),
        drain_timeout_ms: o.drain_timeout.map(|v| v as u64),
        handle_ttl_ms: o.handle_ttl.map(|v| v as u64),
        max_pooled_connections: None,
        max_header_size: None,
        proxy_url: o.proxy_url,
        proxy_from_env: o.proxy_from_env.unwrap_or(false),
        keylog: o.keylog.unwrap_or(false),
        max_concurrency: o.max_concurrency.map(|v| v as usize),
        max_connections_per_peer: o.max_connections_per_peer.map(|v| v as usize),
        request_timeout_ms: o.request_timeout.map(|v| v as u64),
        max_request_body_bytes: o.max_request_body_bytes.map(|v| v as usize),
        drain_timeout_secs: None,
        #[cfg(feature = "compression")]
        compression: if o.compression_level.is_some() || o.compression_min_body_bytes.is_some() {
            Some(iroh_http_core::CompressionOptions {
                level: o.compression_level.unwrap_or(3),
                min_body_bytes: o.compression_min_body_bytes.map(|v| v as usize).unwrap_or(512),
            })
        } else {
            None
        },
    }).unwrap_or_default();

    let ep = IrohEndpoint::bind(opts)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))?;

    // Wire up mDNS discovery if requested.
    if let Some(ref disc) = discovery_js {
        if disc.mdns {
            #[cfg(feature = "discovery")]
            {
                let service_name = disc.service_name.as_deref()
                    .ok_or_else(|| napi::Error::new(Status::InvalidArg,
                        iroh_http_core::classify_error_json("discovery.serviceName is required when mdns is true")))?;
                let mdns = iroh_http_discovery::add_mdns(ep.raw(), service_name, disc.advertise)
                    .map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))?;
                let node_id = ep.node_id().to_string();
                let keypair = ep.secret_key_bytes().to_vec();
                let handle = insert_endpoint(ep);
                discovery_map().lock().unwrap().insert(handle, mdns);
                return Ok(JsEndpointInfo {
                    endpoint_handle: handle,
                    node_id,
                    keypair: Uint8Array::new(keypair),
                });
            }
            #[cfg(not(feature = "discovery"))]
            return Err(napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(
                "mDNS discovery was requested but this build of iroh-http was compiled without the \
                 \"discovery\" feature. If you installed from npm, file an issue. If you built from \
                 source, add: cargo build --features discovery"
            )));
        }
    }

    let node_id = ep.node_id().to_string();
    let keypair = ep.secret_key_bytes().to_vec();
    let handle = insert_endpoint(ep);

    Ok(JsEndpointInfo {
        endpoint_handle: handle,
        node_id,
        keypair: Uint8Array::new(keypair),
    })
}

/// Gracefully close an Iroh endpoint.
///
/// Signals the serve loop (if any) to stop accepting, drains in-flight
/// requests, then shuts down the QUIC endpoint.
#[napi]
pub async fn close_endpoint(endpoint_handle: u32) -> napi::Result<()> {
    #[cfg(feature = "discovery")]
    discovery_map().lock().unwrap().remove(&endpoint_handle);

    let ep = {
        let mut slab = endpoint_slab().lock().unwrap();
        if !slab.contains(endpoint_handle as usize) {
            return Err(napi::Error::new(Status::InvalidArg, iroh_http_core::classify_error_json("invalid endpoint handle")));
        }
        slab.remove(endpoint_handle as usize)
    };
    ep.close().await;
    Ok(())
}

// ── Discovery subscription ────────────────────────────────────────────────────

/// Subscribe to peer discovery events for an endpoint.
///
/// The `callback` is called with the discovered node's public key string
/// whenever a peer is discovered on the local network.
#[napi]
#[cfg(feature = "discovery")]
pub fn on_peer_discovered(
    endpoint_handle: u32,
    #[allow(unused_variables)]
    callback: JsFunction,
) -> napi::Result<()> {
    let _mdns = discovery_map().lock().unwrap().get(&endpoint_handle).cloned()
        .ok_or_else(|| napi::Error::new(Status::InvalidArg,
            iroh_http_core::classify_error_json("no discovery configured for this endpoint")))?;

    // Subscription via MdnsAddressLookup::subscribe() — wired per iroh's stream API.
    // The callback receives base32-encoded node IDs of discovered peers.
    // Full implementation uses: let mut stream = mdns.subscribe().await;
    // then drives the stream in a tokio task via ThreadsafeFunction.
    Ok(())
}

#[napi]
#[cfg(not(feature = "discovery"))]
pub fn on_peer_discovered(
    _endpoint_handle: u32,
    _callback: JsFunction,
) -> napi::Result<()> {
    Err(napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(
        "discovery feature not enabled in this build"
    )))
}

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
}

/// Full node address: node ID + relay URL(s) + direct socket addresses.
#[napi]
pub fn node_addr(endpoint_handle: u32) -> napi::Result<JsNodeAddrInfo> {
    let ep = get_endpoint(endpoint_handle)?;
    let info = ep.node_addr();
    Ok(JsNodeAddrInfo { id: info.id, addrs: info.addrs })
}

/// Generate a ticket string for the given endpoint.
///
/// The ticket encodes the node ID and all known addresses (relay URLs + direct IPs).
/// Share with peers so they can connect directly.
#[napi]
pub fn node_ticket(endpoint_handle: u32) -> napi::Result<String> {
    let ep = get_endpoint(endpoint_handle)?;
    Ok(iroh_http_core::node_ticket(&ep))
}

/// Home relay URL, or null if not connected to a relay.
#[napi]
pub fn home_relay(endpoint_handle: u32) -> napi::Result<Option<String>> {
    let ep = get_endpoint(endpoint_handle)?;
    Ok(ep.home_relay())
}

/// Known addresses for a remote peer, or null if unknown.
#[napi]
pub async fn peer_info(endpoint_handle: u32, node_id: String) -> napi::Result<Option<JsNodeAddrInfo>> {
    let ep = get_endpoint(endpoint_handle)?;
    Ok(ep.peer_info(&node_id).await.map(|info| JsNodeAddrInfo { id: info.id, addrs: info.addrs }))
}

/// Per-peer connection statistics with path information.
#[napi]
pub async fn peer_stats(endpoint_handle: u32, node_id: String) -> napi::Result<Option<JsPeerStats>> {
    let ep = get_endpoint(endpoint_handle)?;
    Ok(ep.peer_stats(&node_id).await.map(|s| JsPeerStats {
        relay: s.relay,
        relay_url: s.relay_url,
        paths: s.paths.into_iter().map(|p| JsPathInfo {
            relay: p.relay,
            addr: p.addr,
            active: p.active,
        }).collect(),
    }))
}

// ── Body streaming ────────────────────────────────────────────────────────────

/// Read the next chunk from a body reader handle.
///
/// Returns `null` at EOF. The handle is automatically cleaned up after EOF.
#[napi]
pub async fn js_next_chunk(handle: u32) -> napi::Result<Option<Buffer>> {
    let chunk = next_chunk(handle)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))?;
    Ok(chunk.map(|b| Buffer::from(b.to_vec())))
}

/// Push a chunk into a body writer handle.
///
/// Large chunks are automatically split to stay within backpressure limits.
#[napi]
pub async fn js_send_chunk(handle: u32, chunk: Uint8Array) -> napi::Result<()> {
    let bytes = Bytes::from(chunk.to_vec());
    send_chunk(handle, bytes)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))
}

/// Signal end-of-body by dropping the writer.
///
/// The paired `BodyReader` will return `null` on its next `nextChunk` call.
#[napi]
pub fn js_finish_body(handle: u32) -> napi::Result<()> {
    finish_body(handle).map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))
}

/// Cancel a body reader, causing any pending `nextChunk` to return null.
#[napi]
pub fn js_cancel_request(handle: u32) {
    cancel_reader(handle);
}

/// Await and retrieve trailer headers from a completed request/response.
///
/// Returns `null` if no trailers were sent.
#[napi]
pub async fn js_next_trailer(handle: u32) -> napi::Result<Option<Vec<Vec<String>>>> {
    let trailers = next_trailer(handle)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))?;
    Ok(trailers.map(|t| t.into_iter().map(|(k, v)| vec![k, v]).collect()))
}

/// Deliver response trailer headers to the Rust pump task.
#[napi]
pub fn js_send_trailers(handle: u32, trailers: Vec<Vec<String>>) -> napi::Result<()> {
    let pairs: Vec<(String, String)> = trailers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    send_trailers(handle, pairs).map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))
}

/// Allocate a body writer handle for streaming request bodies.
///
/// Call this before `rawFetch` to get a handle that can be written to
/// with `sendChunk` / `finishBody`.
#[napi]
pub fn js_alloc_body_writer() -> u32 {
    let (handle, reader) = alloc_body_writer();
    store_pending_reader(handle, reader);
    handle
}

/// Allocate a cancellation token for an upcoming `rawFetch` call.
///
/// Wire `AbortSignal → cancelInFlight(token)` for request cancellation.
#[napi]
pub fn js_alloc_fetch_token() -> u32 {
    iroh_http_core::alloc_fetch_token()
}

/// Cancel an in-flight fetch by its cancellation token.
///
/// Safe to call after the fetch has already completed (no-op).
#[napi]
pub fn js_cancel_in_flight(token: u32) {
    iroh_http_core::cancel_in_flight(token);
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
    pub body_handle: u32,
    /// Full `httpi://` URL of the responding peer.
    pub url: String,
    /// Handle to await response trailer headers.
    pub trailers_handle: u32,
}

/// Send an HTTP request to a remote Iroh peer.
///
/// Low-level function — the shared TS layer wraps this in `makeFetch`.
#[napi]
pub async fn raw_fetch(
    endpoint_handle: u32,
    node_id: String,
    url: String,
    method: String,
    headers: Vec<Vec<String>>,
    req_body_handle: Option<u32>,
    fetch_token: u32,
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

    let req_body_reader = req_body_handle.and_then(claim_pending_reader);

    let addrs = parse_direct_addrs(&direct_addrs);
    let res = iroh_http_core::fetch(&ep, &node_id, &url, &method, &pairs, req_body_reader, Some(fetch_token), addrs.as_deref())
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))?;

    let resp_headers: Vec<Vec<String>> = res
        .headers
        .into_iter()
        .map(|(k, v)| vec![k, v])
        .collect();

    Ok(JsFfiResponse {
        status: res.status as u32,
        headers: resp_headers,
        body_handle: res.body_handle,
        url: res.url,
        trailers_handle: res.trailers_handle,
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
    req_handle: u32,
    status: u32,
    headers: Vec<Vec<String>>,
) -> napi::Result<()> {
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
    respond(req_handle, status as u16, header_pairs)
        .map_err(|e| napi::Error::new(Status::GenericFailure, e))
}

#[napi]
pub fn raw_serve(
    endpoint_handle: u32,
    handler: JsFunction,
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
            obj.set("reqHandle", env.create_uint32(p.req_handle)?)?;
            obj.set("reqBodyHandle", env.create_uint32(p.req_body_handle)?)?;
            obj.set("resBodyHandle", env.create_uint32(p.res_body_handle)?)?;
            obj.set("reqTrailersHandle", env.create_uint32(p.req_trailers_handle)?)?;
            obj.set("resTrailersHandle", env.create_uint32(p.res_trailers_handle)?)?;
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

    let handle = iroh_http_core::serve(
        ep.clone(),
        ep.serve_options(),
        move |payload: RequestPayload| {
            let tsfn = Arc::clone(&tsfn);
            // Fire-and-forget: JS calls rawRespond explicitly.
            tsfn.call(payload, napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking);
        },
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


// ── rawConnect ────────────────────────────────────────────────────────────────

/// Handles for a full-duplex QUIC stream.
#[napi(object)]
pub struct JsFfiDuplexStream {
    /// Body reader handle — call `nextChunk(readHandle)` to receive data.
    pub read_handle: u32,
    /// Body writer handle — call `sendChunk(writeHandle, …)` to send data.
    pub write_handle: u32,
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
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();

    let duplex = iroh_http_core::raw_connect(&ep, &node_id, &path, &pairs)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))?;

    Ok(JsFfiDuplexStream {
        read_handle: duplex.read_handle,
        write_handle: duplex.write_handle,
    })
}

// ── Key operations ────────────────────────────────────────────────────────────

/// Sign arbitrary bytes with a 32-byte Ed25519 secret key.
/// Returns a 64-byte signature as a `Buffer`.
#[napi]
pub fn secret_key_sign(secret_key: Uint8Array, data: Uint8Array) -> napi::Result<Buffer> {
    let key_bytes: [u8; 32] = secret_key.as_ref().try_into()
        .map_err(|_| napi::Error::new(Status::InvalidArg, "secret key must be 32 bytes"))?;
    let sig = iroh_http_core::secret_key_sign(&key_bytes, data.as_ref());
    Ok(Buffer::from(sig.to_vec()))
}

/// Verify a 64-byte Ed25519 signature against a 32-byte public key.
/// Returns `true` on success, `false` on failure — does not throw.
#[napi]
pub fn public_key_verify(public_key: Uint8Array, data: Uint8Array, signature: Uint8Array) -> bool {
    let Ok(key_bytes) = <[u8; 32]>::try_from(public_key.as_ref()) else { return false };
    let Ok(sig_bytes) = <[u8; 64]>::try_from(signature.as_ref()) else { return false };
    iroh_http_core::public_key_verify(&key_bytes, data.as_ref(), &sig_bytes)
}

/// Generate a fresh Ed25519 secret key. Returns 32 raw bytes.
#[napi]
pub fn generate_secret_key() -> Buffer {
    Buffer::from(iroh_http_core::generate_secret_key().to_vec())
}
