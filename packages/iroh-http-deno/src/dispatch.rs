//! JSON-over-FFI dispatch.  Translates `(method, payload)` pairs into calls on
//! `iroh-http-core` and returns a JSON-encoded `{"ok": T} | {"err": string}`.
//!
//! ## Why this file is large
//!
//! Deno FFI (`Deno.dlopen`) exposes a single C-ABI symbol (`iroh_http_call`).
//! Every bridge method arrives as a UTF-8 method name + JSON payload, and the
//! dispatch table below routes each to the appropriate `iroh-http-core` call.
//! Unlike Node.js (napi-rs macros) or Tauri (typed commands), Deno has no
//! code-generation layer that can auto-produce bindings — the match arms,
//! JSON deserialization, and response serialization are all hand-maintained.
//!
//! The file is organised into logical sections (endpoint lifecycle, streaming,
//! fetch/serve, keys, mDNS, sessions).  Each section is a thin shim that
//! deserializes JSON, calls core, and re-serializes the result.  Endpoint
//! slab management is centralised in `iroh_http_core::registry` (A-ISS-041).
//!
//! If a generated binding approach becomes viable for Deno FFI, this file
//! should be replaced.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use bytes::Bytes;
use iroh_http_core::{
    endpoint::{IrohEndpoint, NodeOptions},
    parse_direct_addrs, registry,
    server::respond,
    ConnectionEvent, DiscoveryOptions, NetworkingOptions, PoolOptions, RequestPayload,
    StreamingOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::serve_registry;
#[cfg(feature = "discovery")]
use iroh_http_discovery as _;
#[cfg(feature = "discovery")]
use slab::Slab;
#[cfg(feature = "discovery")]
use std::sync::Arc;
#[cfg(feature = "discovery")]
use std::sync::{Mutex, OnceLock};
#[cfg(feature = "discovery")]
use tokio::sync::Mutex as TokioMutex;

// ── Endpoint helpers ─────────────────────────────────────────────────────────

fn get_endpoint(handle: u32) -> Option<IrohEndpoint> {
    registry::get_endpoint(handle as u64)
}

fn remove_endpoint(handle: u32) -> Option<IrohEndpoint> {
    registry::remove_endpoint(handle as u64)
}

fn insert_endpoint(ep: IrohEndpoint) -> u32 {
    registry::insert_endpoint(ep) as u32
}

use iroh_http_adapter::{core_error_to_json, format_error_json};

// ── Helper ────────────────────────────────────────────────────────────────────

fn ok(v: impl Serialize) -> Value {
    json!({ "ok": v })
}

fn err(s: impl std::fmt::Display) -> Value {
    json!({ "err": format_error_json("UNKNOWN", s) })
}

fn err_code(code: &str, s: impl std::fmt::Display) -> Value {
    json!({ "err": format_error_json(code, s) })
}

fn err_core(e: iroh_http_core::CoreError) -> Value {
    json!({ "err": core_error_to_json(&e) })
}

/// Extract and look up the endpoint from a JSON payload's `endpointHandle`.
fn require_endpoint(p: &Value) -> Result<IrohEndpoint, Value> {
    let handle = p["endpointHandle"]
        .as_u64()
        .ok_or_else(|| err("missing endpointHandle"))? as u32;
    get_endpoint(handle).ok_or_else(|| {
        err_code(
            "INVALID_HANDLE",
            format!("node closed or not found (handle {handle})"),
        )
    })
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

/// Expands one arm of the dispatch table.
///
/// - `async` — calls `handler(p).await`
/// - `sync`  — calls `handler(p)`
/// - `sync0` — calls `handler()` (no payload argument; e.g. `generateSecretKey`)
macro_rules! dispatch_arm {
    (async, $handler:path, $p:expr) => {
        $handler($p).await
    };
    (sync,  $handler:path, $p:expr) => {
        $handler($p)
    };
    (sync0, $handler:path, $_p:expr) => {
        $handler()
    };
}

/// Generates the dispatch `match` from a compact method registry.  Adding a
/// new method is one line: `async "methodName" => handler_fn`.
macro_rules! dispatch_table {
    ($method:expr, $p:expr; $( $kind:ident $name:literal => $handler:path ),+ $(,)?) => {
        match $method {
            $( $name => dispatch_arm!($kind, $handler, $p), )+
            other => err(format!("unknown method: {other}")),
        }
    };
}

/// Entry point called from `lib.rs`.  Parses the JSON payload and routes to the
/// appropriate handler.
pub async fn dispatch(method: &str, payload: &[u8]) -> Value {
    let p: Value = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(e) => return err(format!("invalid JSON payload: {e}")),
    };

    dispatch_table!(method, p;
        // ── Endpoint lifecycle ───────────────────────────────────────────────
        async "createEndpoint"          => create_endpoint,
        async "closeEndpoint"           => close_endpoint,
        sync  "nodeAddr"                => node_addr_dispatch,
        sync  "nodeTicket"              => node_ticket_dispatch,
        sync  "homeRelay"               => home_relay_dispatch,
        async "peerInfo"                => peer_info_dispatch,
        async "peerStats"               => peer_stats_dispatch,
        sync  "endpointStats"           => endpoint_stats_dispatch,
        // ── Handle allocation ────────────────────────────────────────────────
        sync  "allocBodyWriter"         => alloc_body_writer_dispatch,
        sync  "allocTrailerSender"      => alloc_trailer_sender_dispatch,
        sync  "allocFetchToken"         => alloc_fetch_token_dispatch,
        sync  "cancelInFlight"          => cancel_in_flight_dispatch,
        // ── Streaming (JSON fallbacks — hot path uses raw FFI symbols) ───────
        async "nextChunk"               => next_chunk_dispatch,
        async "sendChunk"               => send_chunk_dispatch,
        sync  "finishBody"              => finish_body_dispatch,
        sync  "cancelRequest"           => cancel_request_dispatch,
        async "nextTrailer"             => next_trailer_dispatch,
        sync  "sendTrailers"            => send_trailers_dispatch,
        // ── HTTP ─────────────────────────────────────────────────────────────
        async "rawFetch"                => raw_fetch,
        async "rawConnect"              => raw_connect_dispatch,
        // ── Serve loop ───────────────────────────────────────────────────────
        async "serveStart"              => serve_start,
        async "stopServe"               => stop_serve,
        async "waitEndpointClosed"      => wait_endpoint_closed,
        async "nextRequest"             => next_request,
        async "nextConnectionEvent"     => next_connection_event,
        sync  "respond"                 => respond_dispatch,
        // ── Crypto ───────────────────────────────────────────────────────────
        sync  "secretKeySign"           => secret_key_sign_dispatch,
        sync  "publicKeyVerify"         => public_key_verify_dispatch,
        sync0 "generateSecretKey"       => generate_secret_key_dispatch,
        // ── mDNS ─────────────────────────────────────────────────────────────
        async "mdnsBrowse"              => mdns_browse_dispatch,
        async "mdnsNextEvent"           => mdns_next_event_dispatch,
        sync  "mdnsBrowseClose"         => mdns_browse_close_dispatch,
        sync  "mdnsAdvertise"           => mdns_advertise_dispatch,
        sync  "mdnsAdvertiseClose"      => mdns_advertise_close_dispatch,
        // ── Sessions ─────────────────────────────────────────────────────────
        async "sessionConnect"          => session_connect_dispatch,
        async "sessionCreateBidiStream" => session_create_bidi_stream_dispatch,
        async "sessionNextBidiStream"   => session_next_bidi_stream_dispatch,
        sync  "sessionClose"            => session_close_dispatch,
        async "sessionClosed"           => session_closed_dispatch,
        async "sessionCreateUniStream"  => session_create_uni_stream_dispatch,
        async "sessionNextUniStream"    => session_next_uni_stream_dispatch,
        sync  "sessionSendDatagram"     => session_send_datagram_dispatch,
        async "sessionRecvDatagram"     => session_recv_datagram_dispatch,
        sync  "sessionMaxDatagramSize"  => session_max_datagram_size_dispatch,
    )
}

// ── Endpoint ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // compression fields only read under #[cfg(feature = "compression")]
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
    max_pooled_connections: Option<usize>,
    pool_idle_timeout_ms: Option<u64>,
    disable_networking: Option<bool>,
    proxy_url: Option<String>,
    proxy_from_env: Option<bool>,
    keylog: Option<bool>,
    #[allow(dead_code)] // only read when `compression` feature is enabled
    compression_level: Option<i32>,
    #[allow(dead_code)] // only read when `compression` feature is enabled
    compression_min_body_bytes: Option<usize>,
    max_concurrency: Option<usize>,
    max_connections_per_peer: Option<usize>,
    request_timeout: Option<u64>,
    max_request_body_bytes: Option<usize>,
    max_header_bytes: Option<usize>,
    max_total_connections: Option<usize>,
}

async fn create_endpoint(p: Value) -> Value {
    let args: CreateEndpointPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };

    let key: Option<[u8; 32]> = match args.key {
        None => None,
        Some(k) => {
            let decoded = match B64.decode(&k) {
                Ok(d) => d,
                Err(_) => return err("secret key: invalid base64"),
            };
            match <[u8; 32]>::try_from(decoded) {
                Ok(arr) => Some(arr),
                Err(v) => {
                    return err(format!(
                        "secret key must be exactly 32 bytes, got {}",
                        v.len()
                    ))
                }
            }
        }
    };

    let opts = NodeOptions {
        key,
        networking: NetworkingOptions {
            relay_mode: args.relay_mode,
            relays: args.relays.unwrap_or_default(),
            bind_addrs: args.bind_addrs.unwrap_or_default(),
            idle_timeout_ms: args.idle_timeout,
            proxy_url: args.proxy_url,
            proxy_from_env: args.proxy_from_env.unwrap_or(false),
            disabled: args.disable_networking.unwrap_or(false),
        },
        discovery: DiscoveryOptions {
            dns_server: args.dns_discovery,
            enabled: args.dns_discovery_enabled.unwrap_or(true),
        },
        pool: PoolOptions {
            max_connections: args.max_pooled_connections,
            idle_timeout_ms: args.pool_idle_timeout_ms,
        },
        streaming: StreamingOptions {
            channel_capacity: args.channel_capacity,
            max_chunk_size_bytes: args.max_chunk_size_bytes,
            drain_timeout_ms: args.drain_timeout,
            handle_ttl_ms: args.handle_ttl,
        },
        capabilities: Vec::new(),
        keylog: args.keylog.unwrap_or(false),
        max_header_size: args.max_header_bytes,
        server_limits: iroh_http_core::server::ServerLimits {
            max_concurrency: args.max_concurrency,
            max_connections_per_peer: args.max_connections_per_peer,
            request_timeout_ms: args.request_timeout,
            max_request_body_bytes: args.max_request_body_bytes,
            max_consecutive_errors: args.max_consecutive_errors,
            drain_timeout_secs: None,
            max_total_connections: args.max_total_connections,
            load_shed: None,
        },
        #[cfg(feature = "compression")]
        compression: if args.compression_min_body_bytes.is_some()
            || args.compression_level.is_some()
        {
            // ISS-020: validate compression level range before cast.
            if let Some(level) = args.compression_level {
                if level < 0 {
                    return err(format!(
                        "compressionLevel must be non-negative, got {level}"
                    ));
                }
            }
            Some(iroh_http_core::CompressionOptions {
                min_body_bytes: args.compression_min_body_bytes.unwrap_or(512),
                level: args.compression_level.map(|v| v as u32),
            })
        } else {
            None
        },
    };
    match IrohEndpoint::bind(opts).await {
        Err(e) => err_core(e),
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
    let force = p["force"].as_bool().unwrap_or(false);
    serve_registry::remove(handle);
    match remove_endpoint(handle) {
        None => err_code(
            "INVALID_HANDLE",
            format!("node closed or not found (handle {handle})"),
        ),
        Some(ep) => {
            if force {
                ep.close_force().await;
            } else {
                ep.close().await;
            }
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
        None => err_code(
            "INVALID_HANDLE",
            format!("node closed or not found (handle {handle})"),
        ),
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
        None => err_code(
            "INVALID_HANDLE",
            format!("node closed or not found (handle {handle})"),
        ),
        Some(ep) => match iroh_http_core::node_ticket(&ep) {
            Ok(ticket) => ok(ticket),
            Err(e) => err_core(e),
        },
    }
}

fn home_relay_dispatch(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    match get_endpoint(handle) {
        None => err_code(
            "INVALID_HANDLE",
            format!("node closed or not found (handle {handle})"),
        ),
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
        None => err_code(
            "INVALID_HANDLE",
            format!("node closed or not found (handle {handle})"),
        ),
        Some(ep) => ok(ep
            .peer_info(node_id)
            .await
            .map(|info| json!({ "id": info.id, "addrs": info.addrs }))),
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
        None => err_code(
            "INVALID_HANDLE",
            format!("node closed or not found (handle {handle})"),
        ),
        Some(ep) => ok(ep.peer_stats(node_id).await),
    }
}

fn endpoint_stats_dispatch(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    match get_endpoint(handle) {
        None => err_code(
            "INVALID_HANDLE",
            format!("node closed or not found (handle {handle})"),
        ),
        Some(ep) => ok(ep.endpoint_stats()),
    }
}

// ── Body writer allocation ────────────────────────────────────────────────────

fn alloc_body_writer_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    let (handle, reader) = match ep.handles().alloc_body_writer() {
        Ok(v) => v,
        Err(e) => return err_core(e),
    };
    ep.handles().store_pending_reader(handle, reader);
    ok(json!({ "handle": handle }))
}

fn alloc_fetch_token_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    match ep.handles().alloc_fetch_token() {
        Ok(token) => ok(json!({ "token": token })),
        Err(e) => err_core(e),
    }
}

fn alloc_trailer_sender_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    match ep.handles().alloc_trailer_sender() {
        Ok(handle) => ok(json!({ "handle": handle })),
        Err(e) => err_core(e),
    }
}

fn cancel_in_flight_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    let token = match p["token"].as_u64() {
        Some(t) => t,
        None => return err("missing token"),
    };
    ep.handles().cancel_in_flight(token);
    ok(json!({}))
}

// ── Streaming bridge ──────────────────────────────────────────────────────────

async fn next_chunk_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    let handle = match p["handle"].as_u64() {
        Some(h) => h,
        None => return err("missing handle"),
    };
    match ep.handles().next_chunk(handle).await {
        Err(e) => err_core(e),
        Ok(None) => ok(json!({ "chunk": null })),
        Ok(Some(b)) => ok(json!({ "chunk": B64.encode(&b[..]) })),
    }
}

async fn send_chunk_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    let handle = match p["handle"].as_u64() {
        Some(h) => h,
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
    match ep.handles().send_chunk(handle, bytes).await {
        Ok(()) => ok(json!({})),
        Err(e) => err_core(e),
    }
}

fn finish_body_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    let handle = match p["handle"].as_u64() {
        Some(h) => h,
        None => return err("missing handle"),
    };
    match ep.handles().finish_body(handle) {
        Ok(()) => ok(json!({})),
        Err(e) => err_core(e),
    }
}

fn cancel_request_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    let handle = match p["handle"].as_u64() {
        Some(h) => h,
        None => return err("missing handle"),
    };
    ep.handles().cancel_reader(handle);
    ok(json!({}))
}

async fn next_trailer_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    let handle = match p["handle"].as_u64() {
        Some(h) => h,
        None => return err("missing handle"),
    };
    match ep.handles().next_trailer(handle).await {
        Err(e) => err_core(e),
        Ok(None) => ok(json!({ "trailers": null })),
        Ok(Some(t)) => ok(json!({ "trailers": t })),
    }
}

fn send_trailers_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    let handle = match p["handle"].as_u64() {
        Some(h) => h,
        None => return err("missing handle"),
    };
    let raw: Vec<Vec<String>> = match serde_json::from_value(p["trailers"].clone()) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let pairs: Vec<(String, String)> = raw
        .into_iter()
        .filter_map(|p| {
            if p.len() == 2 {
                Some((p[0].clone(), p[1].clone()))
            } else {
                None
            }
        })
        .collect();
    match ep.handles().send_trailers(handle, pairs) {
        Ok(()) => ok(json!({})),
        Err(e) => err_core(e),
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
    req_body_handle: Option<u64>,
    req_trailers_handle: Option<u64>,
    fetch_token: Option<u64>,
    direct_addrs: Option<Vec<String>>,
}

async fn raw_fetch(p: Value) -> Value {
    let args: RawFetchPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    let pairs: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| {
            if p.len() == 2 {
                Some((p[0].clone(), p[1].clone()))
            } else {
                None
            }
        })
        .collect();
    let reader = args
        .req_body_handle
        .and_then(|h| ep.handles().claim_pending_reader(h));
    let req_trailer_sender_handle = args.req_trailers_handle;
    let addrs = match parse_direct_addrs(&args.direct_addrs) {
        Ok(a) => a,
        Err(e) => return err(e),
    };
    match iroh_http_core::fetch(
        &ep,
        &args.node_id,
        &args.url,
        &args.method,
        &pairs,
        reader,
        req_trailer_sender_handle,
        args.fetch_token,
        addrs.as_deref(),
    )
    .await
    {
        Err(e) => err_core(e),
        Ok(res) => {
            let headers: Vec<Vec<String>> =
                res.headers.into_iter().map(|(k, v)| vec![k, v]).collect();
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
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    let pairs: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| {
            if p.len() == 2 {
                Some((p[0].clone(), p[1].clone()))
            } else {
                None
            }
        })
        .collect();
    match iroh_http_core::raw_connect(&ep, &args.node_id, &args.path, &pairs).await {
        Err(e) => err_core(e),
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
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {handle})"),
            )
        }
    };

    let queue = serve_registry::register(handle);

    let ep_clone = ep.clone();
    let conn_tx = queue.conn_tx.clone();
    let conn_event_fn: Option<std::sync::Arc<dyn Fn(ConnectionEvent) + Send + Sync>> =
        Some(std::sync::Arc::new(move |ev: ConnectionEvent| {
            let _ = conn_tx.try_send(serde_json::json!({
                "peerId": ev.peer_id,
                "connected": ev.connected,
            }));
        }));
    let serve_handle = iroh_http_core::serve_with_events(
        ep.clone(),
        ep.serve_options(),
        move |payload: RequestPayload| {
            let q = std::sync::Arc::clone(&queue);
            let ep_ref = ep_clone.clone();
            let headers: Vec<Vec<String>> = payload
                .headers
                .into_iter()
                .map(|(k, v)| vec![k, v])
                .collect();
            let event = serde_json::json!({
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
                    let _ = respond(
                        ep_ref.handles(),
                        payload.req_handle,
                        503,
                        vec![("content-length".to_string(), "0".to_string())],
                    );
                }
            });
        },
        conn_event_fn,
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
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {handle})"),
            )
        }
    };
    ep.stop_serve();
    // DENO-002: drop the registry entry so the tx inside ServeQueue is freed.
    // Once the serve closure also drops its cloned tx, the channel closes and
    // nextRequest's recv() returns None, allowing the polling loop to exit.
    serve_registry::remove(handle);
    ok(json!({}))
}

async fn wait_endpoint_closed(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    let ep = match get_endpoint(handle) {
        Some(e) => e,
        None => return ok(json!({})), // already removed — treat as closed
    };
    ep.wait_closed().await;
    ok(json!({}))
}

async fn next_request(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    let queue = match serve_registry::get(handle) {
        Some(q) => q,
        // Queue was already removed (stopServe completed) — signal end-of-stream.
        None => return ok(Value::Null),
    };
    // Clone the receiver so we can watch for shutdown without moving it.
    // `wait_for(|v| *v)` completes immediately if shutdown was already triggered
    // (watch persists its last value), or waits until it becomes true — both paths
    // unblock any pending recv() call (issue-12 fix).
    let mut shutdown_rx = queue.shutdown_rx.clone();
    let item = tokio::select! {
        biased;
        _ = shutdown_rx.wait_for(|v| *v) => None,
        item = async { queue.rx.lock().await.recv().await } => item,
    };
    ok(item)
}

/// Poll the next peer connection event (connect or disconnect) for an endpoint.
///
/// Returns `{"ok": {"peerId": "...", "connected": bool}}` on success,
/// or `{"ok": null}` when the serve loop has stopped and no more events will arrive.
async fn next_connection_event(p: Value) -> Value {
    let handle = match p["endpointHandle"].as_u64() {
        Some(h) => h as u32,
        None => return err("missing endpointHandle"),
    };
    let queue = match serve_registry::get(handle) {
        Some(q) => q,
        None => return ok(Value::Null),
    };
    let mut shutdown_rx = queue.shutdown_rx.clone();
    let item = tokio::select! {
        biased;
        _ = shutdown_rx.wait_for(|v| *v) => None,
        item = async { queue.conn_rx.lock().await.recv().await } => item,
    };
    ok(item)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RespondPayload {
    #[allow(dead_code)]
    endpoint_handle: u32,
    req_handle: u64,
    status: u16,
    headers: Vec<Vec<String>>,
}

fn respond_dispatch(p: Value) -> Value {
    let ep = match require_endpoint(&p) {
        Ok(ep) => ep,
        Err(e) => return e,
    };
    let args: RespondPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let headers: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| {
            if p.len() == 2 {
                Some((p[0].clone(), p[1].clone()))
            } else {
                None
            }
        })
        .collect();
    match respond(ep.handles(), args.req_handle, args.status, headers) {
        Ok(()) => ok(json!({})),
        Err(e) => err_core(e),
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
    let sig = match iroh_http_core::secret_key_sign(&key_bytes, &data_bytes) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
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
    ok(json!(iroh_http_core::public_key_verify(
        &key_bytes,
        &data_bytes,
        &sig_bytes
    )))
}

fn generate_secret_key_dispatch() -> Value {
    let key = match iroh_http_core::generate_secret_key() {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    ok(json!(B64.encode(key)))
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
            None => {
                return err_code(
                    "INVALID_HANDLE",
                    format!("node closed or not found (handle {handle})"),
                )
            }
        };
        match iroh_http_discovery::start_browse(ep.raw(), service_name).await {
            Err(e) => err_code("REFUSED", e),
            Ok(session) => {
                let h = browse_slab()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(Arc::new(TokioMutex::new(session))) as u32;
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
        let session = match browse_slab()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(handle as usize)
            .cloned()
        {
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
        let mut slab = browse_slab().lock().unwrap_or_else(|e| e.into_inner());
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
            None => {
                return err_code(
                    "INVALID_HANDLE",
                    format!("node closed or not found (handle {handle})"),
                )
            }
        };
        match iroh_http_discovery::start_advertise(ep.raw(), service_name) {
            Err(e) => err_code("REFUSED", e),
            Ok(session) => {
                let h = advertise_slab()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(session) as u32;
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
        let mut slab = advertise_slab().lock().unwrap_or_else(|e| e.into_inner());
        if slab.contains(handle as usize) {
            slab.remove(handle as usize);
        }
    }
    ok(json!({}))
}

// ── Session ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionConnectPayload {
    endpoint_handle: u32,
    node_id: String,
    direct_addrs: Option<Vec<String>>,
}

async fn session_connect_dispatch(p: Value) -> Value {
    let args: SessionConnectPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    let addrs = match parse_direct_addrs(&args.direct_addrs) {
        Ok(a) => a,
        Err(e) => return err(e),
    };
    match iroh_http_core::session_connect(&ep, &args.node_id, addrs.as_deref()).await {
        Err(e) => err_core(e),
        Ok(handle) => ok(json!({ "sessionHandle": handle })),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionEndpointPayload {
    endpoint_handle: u32,
    session_handle: u64,
}

async fn session_create_bidi_stream_dispatch(p: Value) -> Value {
    let args: SessionEndpointPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    match iroh_http_core::session_create_bidi_stream(&ep, args.session_handle).await {
        Err(e) => err_core(e),
        Ok(d) => ok(json!({ "readHandle": d.read_handle, "writeHandle": d.write_handle })),
    }
}

async fn session_next_bidi_stream_dispatch(p: Value) -> Value {
    let args: SessionEndpointPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    match iroh_http_core::session_next_bidi_stream(&ep, args.session_handle).await {
        Err(e) => err_core(e),
        Ok(None) => ok(json!(null)),
        Ok(Some(d)) => ok(json!({ "readHandle": d.read_handle, "writeHandle": d.write_handle })),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionClosePayload {
    endpoint_handle: u32,
    session_handle: u64,
    close_code: Option<u64>,
    reason: Option<String>,
}

fn session_close_dispatch(p: Value) -> Value {
    let args: SessionClosePayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    match iroh_http_core::session_close(
        &ep,
        args.session_handle,
        args.close_code.unwrap_or(0),
        args.reason.as_deref().unwrap_or(""),
    ) {
        Err(e) => err_core(e),
        Ok(()) => ok(json!({})),
    }
}

async fn session_closed_dispatch(p: Value) -> Value {
    let args: SessionEndpointPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    match iroh_http_core::session_closed(&ep, args.session_handle).await {
        Err(e) => err_core(e),
        Ok(info) => ok(json!({ "closeCode": info.close_code, "reason": info.reason })),
    }
}

async fn session_create_uni_stream_dispatch(p: Value) -> Value {
    let args: SessionEndpointPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    match iroh_http_core::session_create_uni_stream(&ep, args.session_handle).await {
        Err(e) => err_core(e),
        Ok(handle) => ok(json!({ "writeHandle": handle })),
    }
}

async fn session_next_uni_stream_dispatch(p: Value) -> Value {
    let args: SessionEndpointPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    match iroh_http_core::session_next_uni_stream(&ep, args.session_handle).await {
        Err(e) => err_core(e),
        Ok(None) => ok(json!(null)),
        Ok(Some(handle)) => ok(json!({ "readHandle": handle })),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionDatagramPayload {
    endpoint_handle: u32,
    session_handle: u64,
    data: String, // base64
}

fn session_send_datagram_dispatch(p: Value) -> Value {
    let args: SessionDatagramPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    let data = match B64.decode(&args.data) {
        Ok(d) => d,
        Err(e) => return err(format!("base64 decode: {e}")),
    };
    match iroh_http_core::session_send_datagram(&ep, args.session_handle, &data) {
        Err(e) => err_core(e),
        Ok(()) => ok(json!({})),
    }
}

async fn session_recv_datagram_dispatch(p: Value) -> Value {
    let args: SessionEndpointPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    match iroh_http_core::session_recv_datagram(&ep, args.session_handle).await {
        Err(e) => err_core(e),
        Ok(None) => ok(json!(null)),
        Ok(Some(data)) => ok(json!({ "data": B64.encode(&data) })),
    }
}

fn session_max_datagram_size_dispatch(p: Value) -> Value {
    let args: SessionEndpointPayload = match serde_json::from_value(p) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let ep = match get_endpoint(args.endpoint_handle) {
        Some(e) => e,
        None => {
            return err_code(
                "INVALID_HANDLE",
                format!("node closed or not found (handle {})", args.endpoint_handle),
            )
        }
    };
    match iroh_http_core::session_max_datagram_size(&ep, args.session_handle) {
        Err(e) => err_core(e),
        Ok(size) => ok(json!({ "maxDatagramSize": size })),
    }
}
