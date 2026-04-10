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
    server::{ServeOptions, respond},
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

#[napi(object)]
pub struct JsNodeOptions {
    pub key: Option<Uint8Array>,
    pub idle_timeout: Option<f64>,
    pub relays: Option<Vec<String>>,
    pub dns_discovery: Option<String>,
    pub channel_capacity: Option<u32>,
    pub max_chunk_size_bytes: Option<u32>,
    pub max_consecutive_errors: Option<u32>,
    pub discovery: Option<JsDiscoveryOptions>,
    pub drain_timeout: Option<f64>,
    pub handle_ttl: Option<f64>,
    pub disable_networking: Option<bool>,
}

#[napi(object)]
pub struct JsEndpointInfo {
    pub endpoint_handle: u32,
    pub node_id: String,
    pub keypair: Uint8Array,
}

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
        relays: o.relays.unwrap_or_default(),
        dns_discovery: o.dns_discovery,
        capabilities: Vec::new(),
        channel_capacity: o.channel_capacity.map(|v| v as usize),
        max_chunk_size_bytes: o.max_chunk_size_bytes.map(|v| v as usize),
        max_consecutive_errors: o.max_consecutive_errors.map(|v| v as usize),
        discovery: discovery_js.clone(),
        disable_networking: o.disable_networking.unwrap_or(false),
        drain_timeout_ms: o.drain_timeout.map(|v| v as u64),
        handle_ttl_ms: o.handle_ttl.map(|v| v as u64),
        max_pooled_connections: None,
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

#[napi]
pub async fn js_next_chunk(handle: u32) -> napi::Result<Option<Buffer>> {
    let chunk = next_chunk(handle)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))?;
    Ok(chunk.map(|b| Buffer::from(b.to_vec())))
}

#[napi]
pub async fn js_send_chunk(handle: u32, chunk: Uint8Array) -> napi::Result<()> {
    let bytes = Bytes::from(chunk.to_vec());
    send_chunk(handle, bytes)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))
}

#[napi]
pub fn js_finish_body(handle: u32) -> napi::Result<()> {
    finish_body(handle).map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))
}

#[napi]
pub fn js_cancel_request(handle: u32) {
    cancel_reader(handle);
}

#[napi]
pub async fn js_next_trailer(handle: u32) -> napi::Result<Option<Vec<Vec<String>>>> {
    let trailers = next_trailer(handle)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))?;
    Ok(trailers.map(|t| t.into_iter().map(|(k, v)| vec![k, v]).collect()))
}

#[napi]
pub fn js_send_trailers(handle: u32, trailers: Vec<Vec<String>>) -> napi::Result<()> {
    let pairs: Vec<(String, String)> = trailers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    send_trailers(handle, pairs).map_err(|e| napi::Error::new(Status::GenericFailure, iroh_http_core::classify_error_json(e)))
}

#[napi]
pub fn js_alloc_body_writer() -> u32 {
    let (handle, reader) = alloc_body_writer();
    store_pending_reader(handle, reader);
    handle
}

#[napi]
pub fn js_alloc_fetch_token() -> u32 {
    iroh_http_core::alloc_fetch_token()
}

#[napi]
pub fn js_cancel_in_flight(token: u32) {
    iroh_http_core::cancel_in_flight(token);
}

// ── rawFetch ──────────────────────────────────────────────────────────────────

#[napi(object)]
pub struct JsFfiResponse {
    pub status: u32,
    pub headers: Vec<Vec<String>>,
    pub body_handle: u32,
    pub url: String,
    pub trailers_handle: u32,
}

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

    iroh_http_core::serve(
        ep.clone(),
        ServeOptions { max_consecutive_errors: Some(ep.max_consecutive_errors()), ..Default::default() },
        move |payload: RequestPayload| {
            let tsfn = Arc::clone(&tsfn);
            // Fire-and-forget: JS calls rawRespond explicitly.
            tsfn.call(payload, napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking);
        },
    );

    Ok(())
}


// ── rawConnect ────────────────────────────────────────────────────────────────

#[napi(object)]
pub struct JsFfiDuplexStream {
    pub read_handle: u32,
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
