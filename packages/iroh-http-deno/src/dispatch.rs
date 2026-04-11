//! JSON-over-FFI dispatch.  Translates `(method, payload)` pairs into calls on
//! `iroh-http-core` and returns a JSON-encoded `{"ok": T} | {"err": string}`.
//!
//! Every method listed in the patch spec is handled here.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use bytes::Bytes;
use iroh_http_core::{
    endpoint::{IrohEndpoint, NodeOptions},
    server::respond,
    stream::{
        alloc_body_writer, cancel_reader, claim_pending_reader, finish_body,
        next_chunk, next_trailer, send_chunk, send_trailers, store_pending_reader,
    },
    RequestPayload,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

#[cfg(feature = "discovery")]
use iroh_http_discovery;#[cfg(feature = "discovery")]
use std::sync::Arc;
#[cfg(feature = "discovery")]
use tokio::sync::Mutex as TokioMutex;
use crate::serve_registry;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_direct_addrs(addrs: &Option<Vec<String>>) -> Option<Vec<std::net::SocketAddr>> {
    addrs.as_ref().map(|v| {
        v.iter()
            .filter_map(|s| s.parse::<std::net::SocketAddr>().ok())
            .collect()
    })
}

// ── Endpoint slab (replicates the napi / tauri pattern) ──────────────────────

use slab::Slab;

fn endpoint_slab() -> &'static Mutex<Slab<IrohEndpoint>> {
    static S: OnceLock<Mutex<Slab<IrohEndpoint>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

fn insert_endpoint(ep: IrohEndpoint) -> u32 {
    endpoint_slab().lock().unwrap().insert(ep) as u32
}

fn get_endpoint(handle: u32) -> Option<IrohEndpoint> {
    endpoint_slab()
        .lock()
        .unwrap()
        .get(handle as usize)
        .cloned()
}

fn remove_endpoint(handle: u32) -> Option<IrohEndpoint> {
    let mut slab = endpoint_slab().lock().unwrap();
    if slab.contains(handle as usize) {
        Some(slab.remove(handle as usize))
    } else {
        None
    }
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn ok(v: impl Serialize) -> Value {
    json!({ "ok": v })
}

fn err(s: impl std::fmt::Display) -> Value {
    json!({ "err": iroh_http_core::classify_error_json(s) })
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

/// Entry point called from `lib.rs`.  Parses the JSON payload and routes to the
/// appropriate handler.
pub async fn dispatch(method: &str, payload: &[u8]) -> Value {
    let p: Value = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(e) => return err(format!("invalid JSON payload: {e}")),
    };

    match method {
        "createEndpoint" => create_endpoint(p).await,
        "closeEndpoint" => close_endpoint(p).await,
        "nodeAddr" => node_addr_dispatch(p),
        "nodeTicket" => node_ticket_dispatch(p),
        "homeRelay" => home_relay_dispatch(p),
        "peerInfo" => peer_info_dispatch(p).await,
        "peerStats" => peer_stats_dispatch(p).await,
        "allocBodyWriter" => alloc_body_writer_dispatch(),
        "allocFetchToken" => alloc_fetch_token_dispatch(),
        "cancelInFlight" => cancel_in_flight_dispatch(p),
        "nextChunk" => next_chunk_dispatch(p).await,
        "sendChunk" => send_chunk_dispatch(p).await,
        "finishBody" => finish_body_dispatch(p),
        "cancelRequest" => cancel_request_dispatch(p),
        "nextTrailer" => next_trailer_dispatch(p).await,
        "sendTrailers" => send_trailers_dispatch(p),
        "rawFetch" => raw_fetch(p).await,
        "rawConnect" => raw_connect_dispatch(p).await,
        "serveStart" => serve_start(p).await,
        "stopServe" => stop_serve(p).await,
        "nextRequest" => next_request(p).await,
        "respond" => respond_dispatch(p),
        "secretKeySign" => secret_key_sign_dispatch(p),
        "publicKeyVerify" => public_key_verify_dispatch(p),
        "generateSecretKey" => generate_secret_key_dispatch(),
        "mdnsBrowse" => mdns_browse_dispatch(p).await,
        "mdnsNextEvent" => mdns_next_event_dispatch(p).await,
        "mdnsBrowseClose" => mdns_browse_close_dispatch(p),
        "mdnsAdvertise" => mdns_advertise_dispatch(p),
        "mdnsAdvertiseClose" => mdns_advertise_close_dispatch(p),
        _ => err(format!("unknown method: {method}")),
    }
}

// ── Endpoint ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateEndpointPayload {
    key: Option<String>,
    idle_timeout: Option<u64>,
    relay_mode: Option<String>,
    relays: Option<Vec<String>>,
    bind_addrs: Option<Vec<String>>,
    dns_discovery: Option<String>,
    dns_discovery_enabled: Option<bool>,
    channel_capacity: Option<usize>,
    max_chunk_size_bytes: Option<usize>,
    max_consecutive_errors: Option<usize>,
    drain_timeout: Option<u64>,
    handle_ttl: Option<u64>,
    disable_networking: Option<bool>,
    proxy_url: Option<String>,
    proxy_from_env: Option<bool>,
    keylog: Option<bool>,
    compression_level: Option<i32>,
    compression_min_body_bytes: Option<usize>,
    max_concurrency: Option<usize>,
    max_connections_per_peer: Option<usize>,
    request_timeout: Option<u64>,
    max_request_body_bytes: Option<usize>,
}

async fn create_endpoint(p: Value) -> Value {
    let args: CreateEndpointPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };

    let opts = NodeOptions {
        key: args.key.and_then(|k| B64.decode(k).ok()?.try_into().ok()),
        idle_timeout_ms: args.idle_timeout,
        relay_mode: args.relay_mode,
        relays: args.relays.unwrap_or_default(),
        bind_addrs: args.bind_addrs.unwrap_or_default(),
        dns_discovery: args.dns_discovery,
        dns_discovery_enabled: args.dns_discovery_enabled.unwrap_or(true),
        capabilities: Vec::new(),
        channel_capacity: args.channel_capacity,
        max_chunk_size_bytes: args.max_chunk_size_bytes,
        max_consecutive_errors: args.max_consecutive_errors,
        disable_networking: args.disable_networking.unwrap_or(false),
        drain_timeout_ms: args.drain_timeout,
        handle_ttl_ms: args.handle_ttl,
        max_pooled_connections: None,
        max_header_size: None,
        proxy_url: args.proxy_url,
        proxy_from_env: args.proxy_from_env.unwrap_or(false),
        keylog: args.keylog.unwrap_or(false),
        max_concurrency: args.max_concurrency,
        max_connections_per_peer: args.max_connections_per_peer,
        request_timeout_ms: args.request_timeout,
        max_request_body_bytes: args.max_request_body_bytes,
        drain_timeout_secs: None,
        #[cfg(feature = "compression")]
        compression: if args.compression_level.is_some() || args.compression_min_body_bytes.is_some() {
            Some(iroh_http_core::CompressionOptions {
                level: args.compression_level.unwrap_or(3),
                min_body_bytes: args.compression_min_body_bytes.unwrap_or(512),
            })
        } else {
            None
        },
    };
    match IrohEndpoint::bind(opts).await {
        Err(e) => err(e),
        Ok(ep) => {
            let node_id = ep.node_id().to_string();
            let keypair: Vec<u8> = ep.secret_key_bytes().to_vec();
            let handle = insert_endpoint(ep);
            ok(json!({ "endpointHandle": handle, "nodeId": node_id, "keypair": keypair }))
        }
    }
}

async fn close_endpoint(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    serve_registry::remove(handle);
    match remove_endpoint(handle) {
        None => err(format!("invalid endpoint handle: {handle}")),
        Some(ep) => {
            ep.close().await;
            ok(json!({}))
        }
    }
}

// ── Address introspection ─────────────────────────────────────────────────────

fn node_addr_dispatch(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    match get_endpoint(handle) {
        None => err(format!("invalid endpoint handle: {handle}")),
        Some(ep) => {
            let info = ep.node_addr();
            ok(json!({ "id": info.id, "addrs": info.addrs }))
        }
    }
}

fn node_ticket_dispatch(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    match get_endpoint(handle) {
        None => err(format!("invalid endpoint handle: {handle}")),
        Some(ep) => ok(iroh_http_core::node_ticket(&ep)),
    }
}

fn home_relay_dispatch(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    match get_endpoint(handle) {
        None => err(format!("invalid endpoint handle: {handle}")),
        Some(ep) => ok(ep.home_relay()),
    }
}

async fn peer_info_dispatch(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    let node_id = match p["nodeId"].as_str() {
        Some(s) => s,
        None => return err("missing nodeId"),
    };
    match get_endpoint(handle) {
        None => err(format!("invalid endpoint handle: {handle}")),
        Some(ep) => {
            ok(ep.peer_info(node_id).await.map(|info| json!({ "id": info.id, "addrs": info.addrs })))
        }
    }
}

async fn peer_stats_dispatch(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    let node_id = match p["nodeId"].as_str() {
        Some(s) => s,
        None => return err("missing nodeId"),
    };
    match get_endpoint(handle) {
        None => err(format!("invalid endpoint handle: {handle}")),
        Some(ep) => {
            ok(ep.peer_stats(node_id).await)
        }
    }
}

// ── Body writer allocation ────────────────────────────────────────────────────

fn alloc_body_writer_dispatch() -> Value {
    let (handle, reader) = alloc_body_writer();
    store_pending_reader(handle, reader);
    ok(json!({ "handle": handle }))
}

fn alloc_fetch_token_dispatch() -> Value {
    ok(json!({ "token": iroh_http_core::alloc_fetch_token() }))
}

fn cancel_in_flight_dispatch(p: Value) -> Value {
    let token = match p["token"].as_u64() {
        Some(t) => t as u32,
        None => return err("missing token"),
    };
    iroh_http_core::cancel_in_flight(token);
    ok(json!({}))
}

// ── Streaming bridge ──────────────────────────────────────────────────────────

async fn next_chunk_dispatch(p: Value) -> Value {
    let handle = match p["handle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing handle"),
    };
    match next_chunk(handle).await {
        Err(e) => err(e),
        Ok(None) => ok(json!({ "chunk": null })),
        Ok(Some(b)) => ok(json!({ "chunk": B64.encode(&b[..]) })),
    }
}

async fn send_chunk_dispatch(p: Value) -> Value {
    let handle = match p["handle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing handle"),
    };
    let b64: String = match serde_json::from_value(p["chunk"].clone()) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let bytes = match B64.decode(&b64) {
        Ok(b) => Bytes::from(b),
        Err(e) => return err(format!("base64 decode: {e}")),
    };
    match send_chunk(handle, bytes).await {
        Ok(()) => ok(json!({})),
        Err(e) => err(e),
    }
}

fn finish_body_dispatch(p: Value) -> Value {
    let handle = match p["handle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing handle"),
    };
    match finish_body(handle) {
        Ok(()) => ok(json!({})),
        Err(e) => err(e),
    }
}

fn cancel_request_dispatch(p: Value) -> Value {
    let handle = match p["handle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing handle"),
    };
    cancel_reader(handle);
    ok(json!({}))
}

async fn next_trailer_dispatch(p: Value) -> Value {
    let handle = match p["handle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing handle"),
    };
    match next_trailer(handle).await {
        Err(e) => err(e),
        Ok(None) => ok(json!({ "trailers": null })),
        Ok(Some(t)) => ok(json!({ "trailers": t })),
    }
}

fn send_trailers_dispatch(p: Value) -> Value {
    let handle = match p["handle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing handle"),
    };
    let raw: Vec<Vec<String>> = match serde_json::from_value(p["trailers"].clone()) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let pairs: Vec<(String, String)> = raw
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    match send_trailers(handle, pairs) {
        Ok(()) => ok(json!({})),
        Err(e) => err(e),
    }
}

// ── rawFetch ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFetchPayload {
    endpoint_handle: u32,
    node_id: String,
    url: String,
    method: String,
    headers: Vec<Vec<String>>,
    req_body_handle: Option<u32>,
    fetch_token: Option<u32>,
    direct_addrs: Option<Vec<String>>,
}

async fn raw_fetch(p: Value) -> Value {
    let args: RawFetchPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => return err(format!("invalid endpoint handle: {}", args.endpoint_handle)),
    };
    let pairs: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    let reader = args.req_body_handle.and_then(claim_pending_reader);
    let addrs = parse_direct_addrs(&args.direct_addrs);
    match iroh_http_core::fetch(&ep, &args.node_id, &args.url, &args.method, &pairs, reader, args.fetch_token, addrs.as_deref()).await {
        Err(e) => err(e),
        Ok(res) => {
            let headers: Vec<Vec<String>> = res.headers.into_iter().map(|(k, v)| vec![k, v]).collect();
            ok(json!({
                "status": res.status,
                "headers": headers,
                "bodyHandle": res.body_handle,
                "url": res.url,
                "trailersHandle": res.trailers_handle,
            }))
        }
    }
}

// ── rawConnect ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConnectPayload {
    endpoint_handle: u32,
    node_id: String,
    path: String,
    headers: Vec<Vec<String>>,
}

async fn raw_connect_dispatch(p: Value) -> Value {
    let args: RawConnectPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => return err(format!("invalid endpoint handle: {}", args.endpoint_handle)),
    };
    let pairs: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    match iroh_http_core::raw_connect(&ep, &args.node_id, &args.path, &pairs).await {
        Err(e) => err(e),
        Ok(d) => ok(json!({ "readHandle": d.read_handle, "writeHandle": d.write_handle })),
    }
}

// ── serve ─────────────────────────────────────────────────────────────────────

async fn serve_start(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    let ep = match get_endpoint(handle) {
        Some(e) => e,
        None => return err(format!("invalid endpoint handle: {handle}")),
    };

    let queue = serve_registry::register(handle);

    let serve_handle = iroh_http_core::serve(
        ep.clone(),
        ep.serve_options(),
        move |payload: RequestPayload| {
            let q = std::sync::Arc::clone(&queue);
            let headers: Vec<Vec<String>> = payload
                .headers
                .into_iter()
                .map(|(k, v)| vec![k, v])
                .collect();
            let event = json!({
                "reqHandle":         payload.req_handle,
                "reqBodyHandle":     payload.req_body_handle,
                "resBodyHandle":     payload.res_body_handle,
                "reqTrailersHandle": payload.req_trailers_handle,
                "resTrailersHandle": payload.res_trailers_handle,
                "isBidi":          payload.is_bidi,
                "method":            payload.method,
                "url":               payload.url,
                "headers":           headers,
                "remoteNodeId":      payload.remote_node_id,
            });
            let tx = q.tx.clone();
            tokio::spawn(async move {
                // try_send: if queue is full, reject with a 503 immediately
                // rather than stalling the accept loop or growing memory unboundedly.
                if tx.try_send(event).is_err() {
                    tracing::warn!("iroh-http-deno: serve queue full — dropping request with 503");
                    let _ = respond(payload.req_handle, 503,
                        vec![("content-length".to_string(), "0".to_string())]);
                }
            });
        },
    );
    ep.set_serve_handle(serve_handle);

    ok(json!({}))
}

async fn stop_serve(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    let ep = match get_endpoint(handle) {
        Some(e) => e,
        None => return err(format!("invalid endpoint handle: {handle}")),
    };
    ep.stop_serve();
    ok(json!({}))
}

async fn next_request(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    let queue = match serve_registry::get(handle) {
        Some(q) => q,
        None => return err(format!("no serve queue for handle: {handle}")),
    };
    let item = queue.rx.lock().await.recv().await;
    ok(item)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RespondPayload {
    req_handle: u32,
    status: u16,
    headers: Vec<Vec<String>>,
}

fn respond_dispatch(p: Value) -> Value {
    let args: RespondPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let headers: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    match respond(args.req_handle, args.status, headers) {
        Ok(()) => ok(json!({})),
        Err(e) => err(e),
    }
}

// ── Key operations ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignPayload {
    secret_key: String,
    data: String,
}

fn secret_key_sign_dispatch(p: Value) -> Value {
    let args: SignPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let key_bytes: [u8; 32] = match B64.decode(&args.secret_key) {
        Ok(v) => match v.try_into() {
            Ok(a) => a,
            Err(_) => return err("secret key must be 32 bytes"),
        },
        Err(e) => return err(format!("base64 decode key: {e}")),
    };
    let data_bytes = match B64.decode(&args.data) {
        Ok(v) => v,
        Err(e) => return err(format!("base64 decode data: {e}")),
    };
    let sig = iroh_http_core::secret_key_sign(&key_bytes, &data_bytes);
    ok(json!(B64.encode(sig)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifyPayload {
    public_key: String,
    data: String,
    signature: String,
}

fn public_key_verify_dispatch(p: Value) -> Value {
    let args: VerifyPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let key_bytes: [u8; 32] = match B64.decode(&args.public_key) {
        Ok(v) => match v.try_into() {
            Ok(a) => a,
            Err(_) => return err("public key must be 32 bytes"),
        },
        Err(e) => return err(format!("base64 decode key: {e}")),
    };
    let data_bytes = match B64.decode(&args.data) {
        Ok(v) => v,
        Err(e) => return err(format!("base64 decode data: {e}")),
    };
    let sig_bytes: [u8; 64] = match B64.decode(&args.signature) {
        Ok(v) => match v.try_into() {
            Ok(a) => a,
            Err(_) => return err("signature must be 64 bytes"),
        },
        Err(e) => return err(format!("base64 decode sig: {e}")),
    };
    ok(json!(iroh_http_core::public_key_verify(&key_bytes, &data_bytes, &sig_bytes)))
}

fn generate_secret_key_dispatch() -> Value {
    ok(json!(B64.encode(iroh_http_core::generate_secret_key())))
}

// ── mDNS browse / advertise ──────────────────────────────────────────────────

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

async fn mdns_browse_dispatch(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    let service_name = match p["serviceName"].as_str() {
        Some(s) => s,
        None => return err("missing serviceName"),
    };
    #[cfg(feature = "discovery")]
    {
        let ep = match get_endpoint(handle) {
            Some(ep) => ep,
            None => return err(format!("invalid endpoint handle: {handle}")),
        };
        match iroh_http_discovery::start_browse(ep.raw(), service_name).await {
            Err(e) => err(e),
            Ok(session) => {
                let h = browse_slab().lock().unwrap().insert(Arc::new(TokioMutex::new(session))) as u32;
                ok(json!(h))
            }
        }
    }
    #[cfg(not(feature = "discovery"))]
    err("discovery feature not enabled in this build")
}

async fn mdns_next_event_dispatch(p: Value) -> Value {
    let handle = match p["browseHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing browseHandle"),
    };
    #[cfg(feature = "discovery")]
    {
        let session = match browse_slab().lock().unwrap().get(handle as usize).cloned() {
            Some(s) => s,
            None => return err(format!("invalid browse handle: {handle}")),
        };
        let event = session.lock().await.next_event().await;
        match event {
            None => ok(json!(null)),
            Some(ev) => ok(json!({
                "isActive": ev.is_active,
                "nodeId": ev.node_id,
                "addrs": ev.addrs,
            })),
        }
    }
    #[cfg(not(feature = "discovery"))]
    err("discovery feature not enabled in this build")
}

fn mdns_browse_close_dispatch(p: Value) -> Value {
    let handle = match p["browseHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing browseHandle"),
    };
    #[cfg(feature = "discovery")]
    {
        let mut slab = browse_slab().lock().unwrap();
        if slab.contains(handle as usize) {
            slab.remove(handle as usize);
        }
    }
    ok(json!({}))
}

fn mdns_advertise_dispatch(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    let service_name = match p["serviceName"].as_str() {
        Some(s) => s,
        None => return err("missing serviceName"),
    };
    #[cfg(feature = "discovery")]
    {
        let ep = match get_endpoint(handle) {
            Some(ep) => ep,
            None => return err(format!("invalid endpoint handle: {handle}")),
        };
        match iroh_http_discovery::start_advertise(ep.raw(), service_name) {
            Err(e) => err(e),
            Ok(session) => {
                let h = advertise_slab().lock().unwrap().insert(session) as u32;
                ok(json!(h))
            }
        }
    }
    #[cfg(not(feature = "discovery"))]
    err("discovery feature not enabled in this build")
}

fn mdns_advertise_close_dispatch(p: Value) -> Value {
    let handle = match p["advertiseHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing advertiseHandle"),
    };
    #[cfg(feature = "discovery")]
    {
        let mut slab = advertise_slab().lock().unwrap();
        if slab.contains(handle as usize) {
            slab.remove(handle as usize);
        }
    }
    ok(json!({}))
}
