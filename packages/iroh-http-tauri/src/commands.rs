//! Tauri commands for the iroh-http plugin.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use bytes::Bytes;
use iroh_http_core::{
    endpoint::NodeOptions,
    server::respond,
    RequestPayload,
    parse_direct_addrs,
};
use serde::{Deserialize, Serialize};
use tauri::{command, ipc::Channel};

use crate::state;

// ── Endpoint ──────────────────────────────────────────────────────────────────

/// Options for creating an Iroh endpoint from the Tauri frontend.
///
/// All fields are optional — omit for sensible defaults.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEndpointArgs {
    pub key: Option<String>,
    pub idle_timeout: Option<u64>,
    pub relay_mode: Option<String>,
    pub relays: Option<Vec<String>>,
    pub bind_addrs: Option<Vec<String>>,
    pub dns_discovery: Option<String>,
    pub dns_discovery_enabled: Option<bool>,
    pub channel_capacity: Option<usize>,
    pub max_chunk_size_bytes: Option<usize>,
    pub max_consecutive_errors: Option<usize>,
    pub drain_timeout: Option<u64>,
    pub handle_ttl: Option<u64>,
    pub disable_networking: Option<bool>,
    pub proxy_url: Option<String>,
    pub proxy_from_env: Option<bool>,
    pub keylog: Option<bool>,
    #[cfg(feature = "compression")]
    pub compression_min_body_bytes: Option<usize>,
    pub max_concurrency: Option<usize>,
    pub max_connections_per_peer: Option<usize>,
    pub request_timeout: Option<u64>,
    pub max_request_body_bytes: Option<usize>,
    pub max_header_bytes: Option<usize>,
}

/// Info returned after a successful endpoint bind.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointInfoPayload {
    pub endpoint_handle: u64,
    pub node_id: String,
    pub keypair: Vec<u8>,
}

/// Bind an Iroh endpoint and return a handle + identity info.
#[command]
pub async fn create_endpoint(
    args: Option<CreateEndpointArgs>,
) -> Result<EndpointInfoPayload, String> {
    let opts = args
        .map(|a| -> Result<NodeOptions, String> { Ok(NodeOptions {
            key: match a.key {
                Some(k) => {
                    let decoded = B64.decode(&k)
                        .map_err(|_| "secret key: invalid base64".to_string())?;
                    let arr: [u8; 32] = decoded.try_into()
                        .map_err(|v: Vec<u8>| format!("secret key must be exactly 32 bytes, got {}", v.len()))?;
                    Some(arr)
                }
                None => None,
            },
            idle_timeout_ms: a.idle_timeout,
            relay_mode: a.relay_mode,
            relays: a.relays.unwrap_or_default(),
            bind_addrs: a.bind_addrs.unwrap_or_default(),
            dns_discovery: a.dns_discovery,
            dns_discovery_enabled: a.dns_discovery_enabled.unwrap_or(true),
            capabilities: Vec::new(),
            channel_capacity: a.channel_capacity,
            max_chunk_size_bytes: a.max_chunk_size_bytes,
            max_consecutive_errors: a.max_consecutive_errors,
            disable_networking: a.disable_networking.unwrap_or(false),
            drain_timeout_ms: a.drain_timeout,
            handle_ttl_ms: a.handle_ttl,
            max_pooled_connections: None,
            pool_idle_timeout_ms: None,
            max_header_size: a.max_header_bytes,
            proxy_url: a.proxy_url,
            proxy_from_env: a.proxy_from_env.unwrap_or(false),
            keylog: a.keylog.unwrap_or(false),
            max_concurrency: a.max_concurrency,
            max_connections_per_peer: a.max_connections_per_peer,
            request_timeout_ms: a.request_timeout,
            max_request_body_bytes: a.max_request_body_bytes,
            drain_timeout_secs: None,
            #[cfg(feature = "compression")]
            compression: if a.compression_min_body_bytes.is_some() {
                Some(iroh_http_core::CompressionOptions {
                    min_body_bytes: a.compression_min_body_bytes.unwrap_or(512),
                })
            } else {
                None
            },
        }) })
        .transpose()?
        .unwrap_or_default();

    let ep = iroh_http_core::endpoint::IrohEndpoint::bind(opts)
        .await
        .map_err(|e| iroh_http_core::classify_error_json(e))?;

    let node_id = ep.node_id().to_string();
    let keypair = ep.secret_key_bytes().to_vec();
    let handle = state::insert_endpoint(ep);

    Ok(EndpointInfoPayload {
        endpoint_handle: handle,
        node_id,
        keypair,
    })
}

/// Close an Iroh endpoint.
///
/// If `force` is `true`, aborts immediately.  Otherwise drains in-flight requests.
#[command]
pub async fn close_endpoint(endpoint_handle: u64, force: Option<bool>) -> Result<(), String> {
    let ep = state::remove_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    if force.unwrap_or(false) {
        ep.close_force().await;
    } else {
        ep.close().await;
    }
    Ok(())
}

// ── Ping (mobile lifecycle) ───────────────────────────────────────────────────

/// Trivial liveness probe — returns `true` when the endpoint exists.
#[command]
pub async fn ping(endpoint_handle: u64) -> Result<bool, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("endpoint not found: {endpoint_handle}")))?;
    // If the endpoint exists, it's alive.
    let _ = ep.raw().id();
    Ok(true)
}

// ── Address introspection ─────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeAddrPayload {
    pub id: String,
    pub addrs: Vec<String>,
}

/// Full node address: node ID + relay URL(s) + direct socket addresses.
#[command]
pub fn node_addr(endpoint_handle: u64) -> Result<NodeAddrPayload, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    let info = ep.node_addr();
    Ok(NodeAddrPayload { id: info.id, addrs: info.addrs })
}

/// Generate a ticket string for the given endpoint.
#[command]
pub fn node_ticket(endpoint_handle: u64) -> Result<String, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    Ok(iroh_http_core::node_ticket(&ep))
}

/// Home relay URL, or null if not connected to a relay.
#[command]
pub fn home_relay(endpoint_handle: u64) -> Result<Option<String>, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    Ok(ep.home_relay())
}

/// Known addresses for a remote peer, or null if unknown.
#[command]
pub async fn peer_info(endpoint_handle: u64, node_id: String) -> Result<Option<NodeAddrPayload>, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    Ok(ep.peer_info(&node_id).await.map(|info| NodeAddrPayload { id: info.id, addrs: info.addrs }))
}

/// Per-peer connection statistics with path information.
#[command]
pub async fn peer_stats(endpoint_handle: u64, node_id: String) -> Result<Option<iroh_http_core::PeerStats>, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    Ok(ep.peer_stats(&node_id).await)
}

// ── Bridge methods ────────────────────────────────────────────────────────────

/// Read the next chunk from a body reader handle (base64-encoded).
#[command]
pub async fn next_chunk(handle: u64) -> Result<Option<String>, String> {
    let chunk = iroh_http_core::stream::next_chunk(handle).await.map_err(|e| iroh_http_core::classify_error_json(e))?;
    Ok(chunk.map(|b| B64.encode(&b[..])))
}

/// Push a base64-encoded chunk into a body writer handle.
#[command]
pub async fn send_chunk(handle: u64, chunk: String) -> Result<(), String> {
    let bytes = B64.decode(&chunk).map_err(|e| iroh_http_core::classify_error_json(format!("base64 decode: {e}")))?;
    iroh_http_core::stream::send_chunk(handle, Bytes::from(bytes)).await.map_err(|e| iroh_http_core::classify_error_json(e))
}

/// Signal end-of-body for a writer handle.
#[command]
pub fn finish_body(handle: u64) -> Result<(), String> {
    iroh_http_core::stream::finish_body(handle).map_err(|e| iroh_http_core::classify_error_json(e))
}

/// Cancel a body reader, signalling EOF.
#[command]
pub fn cancel_request(handle: u64) {
    iroh_http_core::stream::cancel_reader(handle);
}

/// Await and retrieve trailer headers from a completed request/response.
#[command]
pub async fn next_trailer(handle: u64) -> Result<Option<Vec<Vec<String>>>, String> {
    let trailers = iroh_http_core::stream::next_trailer(handle).await.map_err(|e| iroh_http_core::classify_error_json(e))?;
    Ok(trailers.map(|t| t.into_iter().map(|(k, v)| vec![k, v]).collect()))
}

/// Deliver response trailer headers to the Rust pump task.
#[command]
pub fn send_trailers(handle: u64, trailers: Vec<Vec<String>>) -> Result<(), String> {
    let pairs: Vec<(String, String)> = trailers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    iroh_http_core::stream::send_trailers(handle, pairs).map_err(|e| iroh_http_core::classify_error_json(e))
}

/// Allocate a body writer handle for streaming request bodies.
#[command]
pub fn alloc_body_writer() -> u64 {
    state::js_alloc_body_writer()
}

/// Allocate a cancellation token for an upcoming fetch call.
#[command]
pub fn alloc_fetch_token() -> u64 {
    iroh_http_core::alloc_fetch_token(0)
}

/// Cancel an in-flight fetch by its token.
#[command]
pub fn cancel_in_flight(token: u64) {
    iroh_http_core::cancel_in_flight(token);
}

// ── rawFetch ──────────────────────────────────────────────────────────────────

/// Arguments for `rawFetch` — send an HTTP request to a remote peer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawFetchArgs {
    pub endpoint_handle: u64,
    pub node_id: String,
    pub url: String,
    pub method: String,
    pub headers: Vec<Vec<String>>,
    pub req_body_handle: Option<u64>,
    pub fetch_token: Option<u64>,
    pub direct_addrs: Option<Vec<String>>,
}

/// Response payload returned by `rawFetch`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiResponsePayload {
    pub status: u16,
    pub headers: Vec<Vec<String>>,
    pub body_handle: u64,
    pub url: String,
    pub trailers_handle: u64,
}

/// Send an HTTP request to a remote Iroh peer.
#[command]
pub async fn raw_fetch(args: RawFetchArgs) -> Result<FfiResponsePayload, String> {
    let ep = state::get_endpoint(args.endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {}", args.endpoint_handle)))?;

    let pairs: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();

    let req_body_reader = args.req_body_handle.and_then(iroh_http_core::stream::claim_pending_reader);

    let addrs = parse_direct_addrs(&args.direct_addrs).map_err(|e| e)?;
    let res = iroh_http_core::fetch(&ep, &args.node_id, &args.url, &args.method, &pairs, req_body_reader, args.fetch_token, addrs.as_deref())
        .await.map_err(iroh_http_core::classify_error_json)?;

    let resp_headers: Vec<Vec<String>> = res
        .headers
        .into_iter()
        .map(|(k, v)| vec![k, v])
        .collect();

    Ok(FfiResponsePayload {
        status: res.status,
        headers: resp_headers,
        body_handle: res.body_handle,
        url: res.url,
        trailers_handle: res.trailers_handle,
    })
}

// ── serve ─────────────────────────────────────────────────────────────────────

/// Serialised request payload pushed through the Tauri Channel.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServeEventPayload {
    pub req_handle: u64,
    pub req_body_handle: u64,
    pub res_body_handle: u64,
    pub req_trailers_handle: u64,
    pub res_trailers_handle: u64,
    pub is_bidi: bool,
    pub method: String,
    pub url: String,
    pub headers: Vec<Vec<String>>,
    pub remote_node_id: String,
}

/// Start the serve accept loop, streaming incoming requests via a Tauri Channel.
#[command]
pub async fn serve(
    endpoint_handle: u64,
    channel: Channel<ServeEventPayload>,
) -> Result<(), String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;

    let handle = iroh_http_core::serve(
        ep.clone(),
        ep.serve_options(),
        move |payload: RequestPayload| {
            let ch = channel.clone();
            let headers: Vec<Vec<String>> = payload
                .headers
                .into_iter()
                .map(|(k, v)| vec![k, v])
                .collect();
            let event = ServeEventPayload {
                req_handle: payload.req_handle,
                req_body_handle: payload.req_body_handle,
                res_body_handle: payload.res_body_handle,
                req_trailers_handle: payload.req_trailers_handle,
                res_trailers_handle: payload.res_trailers_handle,
                is_bidi: payload.is_bidi,
                method: payload.method,
                url: payload.url,
                headers,
                remote_node_id: payload.remote_node_id,
            };
            if let Err(e) = ch.send(event) {
                tracing::warn!("iroh-http-tauri: channel send error: {e}");
            }
        },
    );
    ep.set_serve_handle(handle);

    Ok(())
}

/// Stop the serve loop for the given endpoint (graceful shutdown).
#[command]
pub fn stop_serve(endpoint_handle: u64) -> Result<(), String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    ep.stop_serve();
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RespondArgs {
    pub req_handle: u64,
    pub status: u16,
    pub headers: Vec<Vec<String>>,
}

/// Send the response head for a pending request.
///
/// Called from the Tauri frontend after computing the response status and headers.
#[command]
pub fn respond_to_request(args: RespondArgs) -> Result<(), String> {
    let headers: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    respond(args.req_handle, args.status, headers).map_err(|e| iroh_http_core::classify_error_json(e))
}

// ── rawConnect ────────────────────────────────────────────────────────────────

/// Arguments for opening a full-duplex stream.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawConnectArgs {
    pub endpoint_handle: u64,
    pub node_id: String,
    pub path: String,
    pub headers: Vec<Vec<String>>,
}

/// Handles for a full-duplex QUIC stream.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiDuplexStreamPayload {
    pub read_handle: u64,
    pub write_handle: u64,
}

/// Open a full-duplex QUIC connection to a remote peer.
#[command]
pub async fn raw_connect(args: RawConnectArgs) -> Result<FfiDuplexStreamPayload, String> {
    let ep = state::get_endpoint(args.endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {}", args.endpoint_handle)))?;

    let pairs: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();

    let duplex = iroh_http_core::raw_connect(&ep, &args.node_id, &args.path, &pairs)
        .await.map_err(iroh_http_core::classify_error_json)?;

    Ok(FfiDuplexStreamPayload {
        read_handle: duplex.read_handle,
        write_handle: duplex.write_handle,
    })
}

// ── Session ───────────────────────────────────────────────────────────────────

/// Arguments for establishing a session.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConnectArgs {
    pub endpoint_handle: u64,
    pub node_id: String,
    pub direct_addrs: Option<Vec<String>>,
}

/// Establish a session (QUIC connection) to a remote peer.
#[command]
pub async fn session_connect(args: SessionConnectArgs) -> Result<u64, String> {
    let ep = state::get_endpoint(args.endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {}", args.endpoint_handle)))?;

    let addrs: Option<Vec<std::net::SocketAddr>> = args.direct_addrs.as_ref().map(|v| {
        v.iter()
            .filter_map(|s| s.parse::<std::net::SocketAddr>().ok())
            .collect()
    });

    iroh_http_core::session_connect(&ep, &args.node_id, addrs.as_deref())
        .await.map_err(iroh_http_core::classify_error_json)
}

/// Handles for a bidirectional stream on a session.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBidiStreamPayload {
    pub read_handle: u64,
    pub write_handle: u64,
}

/// Open a new bidirectional stream on an existing session.
#[command]
pub async fn session_create_bidi_stream(session_handle: u64) -> Result<SessionBidiStreamPayload, String> {
    let duplex = iroh_http_core::session_create_bidi_stream(session_handle)
        .await.map_err(iroh_http_core::classify_error_json)?;
    Ok(SessionBidiStreamPayload {
        read_handle: duplex.read_handle,
        write_handle: duplex.write_handle,
    })
}

/// Accept the next incoming bidirectional stream on a session.
/// Returns null when the session is closed.
#[command]
pub async fn session_next_bidi_stream(session_handle: u64) -> Result<Option<SessionBidiStreamPayload>, String> {
    let result = iroh_http_core::session_next_bidi_stream(session_handle)
        .await.map_err(iroh_http_core::classify_error_json)?;
    Ok(result.map(|d| SessionBidiStreamPayload {
        read_handle: d.read_handle,
        write_handle: d.write_handle,
    }))
}

/// Close a session with optional close code and reason.
#[command]
pub async fn session_close(session_handle: u64, close_code: Option<u32>, reason: Option<String>) -> Result<(), String> {
    iroh_http_core::session_close(session_handle, close_code.unwrap_or(0), reason.as_deref().unwrap_or(""))
        .map_err(iroh_http_core::classify_error_json)
}

/// Wait for a session to close. Returns { closeCode, reason }.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseInfoPayload {
    pub close_code: u32,
    pub reason: String,
}

#[command]
pub async fn session_closed(session_handle: u64) -> Result<CloseInfoPayload, String> {
    let info = iroh_http_core::session_closed(session_handle)
        .await
        .map_err(iroh_http_core::classify_error_json)?;
    Ok(CloseInfoPayload {
        close_code: info.close_code,
        reason: info.reason,
    })
}

/// Open a new unidirectional (send-only) stream on a session.
/// Returns a write handle.
#[command]
pub async fn session_create_uni_stream(session_handle: u64) -> Result<u64, String> {
    iroh_http_core::session_create_uni_stream(session_handle)
        .await
        .map_err(iroh_http_core::classify_error_json)
}

/// Accept the next incoming unidirectional stream on a session.
/// Returns a read handle, or null when the session is closed.
#[command]
pub async fn session_next_uni_stream(session_handle: u64) -> Result<Option<u64>, String> {
    iroh_http_core::session_next_uni_stream(session_handle)
        .await
        .map_err(iroh_http_core::classify_error_json)
}

/// Send a datagram on a session. Data is base64-encoded.
#[command]
pub async fn session_send_datagram(session_handle: u64, data: String) -> Result<(), String> {
    let bytes = B64.decode(&data)
        .map_err(|e| format!("base64 decode: {e}"))?;
    iroh_http_core::session_send_datagram(session_handle, &bytes)
        .map_err(iroh_http_core::classify_error_json)
}

/// Receive the next datagram on a session. Returns base64, or null when closed.
#[command]
pub async fn session_recv_datagram(session_handle: u64) -> Result<Option<String>, String> {
    let result = iroh_http_core::session_recv_datagram(session_handle)
        .await
        .map_err(iroh_http_core::classify_error_json)?;
    Ok(result.map(|d| B64.encode(&d)))
}

/// Get the maximum datagram payload size for a session.
#[command]
pub fn session_max_datagram_size(session_handle: u64) -> Result<Option<u32>, String> {
    let result = iroh_http_core::session_max_datagram_size(session_handle)
        .map_err(iroh_http_core::classify_error_json)?;
    Ok(result.map(|s| s as u32))
}

// ── Key operations ────────────────────────────────────────────────────────────

/// Sign arbitrary bytes with a 32-byte Ed25519 secret key (base64-encoded).
/// Returns a 64-byte signature as base64.
#[command]
pub fn secret_key_sign(secret_key: String, data: String) -> Result<String, String> {
    let key_bytes: [u8; 32] = B64.decode(&secret_key)
        .map_err(|e| format!("base64 decode key: {e}"))?
        .try_into()
        .map_err(|_| "secret key must be 32 bytes".to_string())?;
    let data_bytes = B64.decode(&data)
        .map_err(|e| format!("base64 decode data: {e}"))?;
    let sig = iroh_http_core::secret_key_sign(&key_bytes, &data_bytes);
    Ok(B64.encode(sig))
}

/// Verify a 64-byte Ed25519 signature against a 32-byte public key.
/// All inputs base64-encoded.  Returns `true` on success.
#[command]
pub fn public_key_verify(public_key: String, data: String, signature: String) -> Result<bool, String> {
    let key_bytes: [u8; 32] = B64.decode(&public_key)
        .map_err(|e| format!("base64 decode key: {e}"))?
        .try_into()
        .map_err(|_| "public key must be 32 bytes".to_string())?;
    let data_bytes = B64.decode(&data)
        .map_err(|e| format!("base64 decode data: {e}"))?;
    let sig_bytes: [u8; 64] = B64.decode(&signature)
        .map_err(|e| format!("base64 decode sig: {e}"))?
        .try_into()
        .map_err(|_| "signature must be 64 bytes".to_string())?;
    Ok(iroh_http_core::public_key_verify(&key_bytes, &data_bytes, &sig_bytes))
}

/// Generate a fresh Ed25519 secret key. Returns 32 raw bytes as base64.
#[command]
pub fn generate_secret_key() -> String {
    B64.encode(iroh_http_core::generate_secret_key())
}

// ── mDNS browse / advertise ──────────────────────────────────────────────────

use std::sync::{Mutex, OnceLock};

#[cfg(feature = "discovery")]
use std::sync::Arc;
#[cfg(feature = "discovery")]
use tokio::sync::Mutex as TokioMutex;

#[cfg(feature = "discovery")]
type BrowseHandle = Arc<TokioMutex<iroh_http_discovery::BrowseSession>>;

#[cfg(feature = "discovery")]
fn browse_slab() -> &'static Mutex<slab::Slab<BrowseHandle>> {
    static S: OnceLock<Mutex<slab::Slab<BrowseHandle>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(slab::Slab::new()))
}

#[cfg(feature = "discovery")]
fn advertise_slab() -> &'static Mutex<slab::Slab<iroh_http_discovery::AdvertiseSession>> {
    static S: OnceLock<Mutex<slab::Slab<iroh_http_discovery::AdvertiseSession>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(slab::Slab::new()))
}

/// Discovery event payload for the Tauri frontend.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerDiscoveryEventPayload {
    pub is_active: bool,
    pub node_id: String,
    pub addrs: Vec<String>,
}

/// Start a browse session: discover peers on the local network via mDNS.
#[command]
#[cfg(feature = "discovery")]
pub async fn mdns_browse(endpoint_handle: u64, service_name: String) -> Result<u64, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    let session = iroh_http_discovery::start_browse(ep.raw(), &service_name)
        .await
        .map_err(|e| iroh_http_core::classify_error_json(e))?;
    let handle = browse_slab().lock().unwrap().insert(Arc::new(TokioMutex::new(session))) as u64;
    Ok(handle)
}

#[command]
#[cfg(not(feature = "discovery"))]
pub async fn mdns_browse(_endpoint_handle: u64, _service_name: String) -> Result<u64, String> {
    Err(iroh_http_core::classify_error_json("discovery feature not enabled in this build"))
}

/// Poll the next discovery event from a browse session.
#[command]
#[cfg(feature = "discovery")]
pub async fn mdns_next_event(browse_handle: u64) -> Result<Option<PeerDiscoveryEventPayload>, String> {
    let session = {
        browse_slab().lock().unwrap().get(browse_handle as usize).cloned()
    }.ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid browse handle: {browse_handle}")))?;
    let event = session.lock().await.next_event().await;
    Ok(event.map(|ev| PeerDiscoveryEventPayload {
        is_active: ev.is_active,
        node_id: ev.node_id,
        addrs: ev.addrs,
    }))
}

#[command]
#[cfg(not(feature = "discovery"))]
pub async fn mdns_next_event(_browse_handle: u64) -> Result<Option<PeerDiscoveryEventPayload>, String> {
    Err(iroh_http_core::classify_error_json("discovery feature not enabled in this build"))
}

/// Close a browse session, stopping mDNS discovery.
#[command]
pub fn mdns_browse_close(browse_handle: u64) {
    #[cfg(feature = "discovery")]
    {
        let mut slab = browse_slab().lock().unwrap();
        if slab.contains(browse_handle as usize) {
            slab.remove(browse_handle as usize);
        }
    }
}

/// Start advertising this node on the local network via mDNS.
#[command]
#[cfg(feature = "discovery")]
pub fn mdns_advertise(endpoint_handle: u64, service_name: String) -> Result<u64, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    let session = iroh_http_discovery::start_advertise(ep.raw(), &service_name)
        .map_err(|e| iroh_http_core::classify_error_json(e))?;
    let handle = advertise_slab().lock().unwrap().insert(session) as u64;
    Ok(handle)
}

#[command]
#[cfg(not(feature = "discovery"))]
pub fn mdns_advertise(_endpoint_handle: u64, _service_name: String) -> Result<u64, String> {
    Err(iroh_http_core::classify_error_json("discovery feature not enabled in this build"))
}

/// Stop advertising this node on the local network.
#[command]
pub fn mdns_advertise_close(advertise_handle: u64) {
    #[cfg(feature = "discovery")]
    {
        let mut slab = advertise_slab().lock().unwrap();
        if slab.contains(advertise_handle as usize) {
            slab.remove(advertise_handle as usize);
        }
    }
}

