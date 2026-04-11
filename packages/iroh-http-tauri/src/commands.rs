//! Tauri commands for the iroh-http plugin.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use bytes::Bytes;
use iroh_http_core::{
    endpoint::{DiscoveryConfig, NodeOptions},
    server::{ServeOptions, respond},
    RequestPayload,
};
use serde::{Deserialize, Serialize};
use tauri::{command, ipc::Channel};

use crate::state;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_direct_addrs(addrs: &Option<Vec<String>>) -> Option<Vec<std::net::SocketAddr>> {
    addrs.as_ref().map(|v| {
        v.iter()
            .filter_map(|s| s.parse::<std::net::SocketAddr>().ok())
            .collect()
    })
}

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
    pub discovery_mdns: Option<bool>,
    pub discovery_service_name: Option<String>,
    pub discovery_advertise: Option<bool>,
    pub drain_timeout: Option<u64>,
    pub handle_ttl: Option<u64>,
    pub disable_networking: Option<bool>,
    pub proxy_url: Option<String>,
    pub proxy_from_env: Option<bool>,
    pub keylog: Option<bool>,
    pub compression_level: Option<i32>,
    pub compression_min_body_bytes: Option<usize>,
}

/// Info returned after a successful endpoint bind.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointInfoPayload {
    pub endpoint_handle: u32,
    pub node_id: String,
    pub keypair: Vec<u8>,
}

/// Bind an Iroh endpoint and return a handle + identity info.
#[command]
pub async fn create_endpoint(
    args: Option<CreateEndpointArgs>,
) -> Result<EndpointInfoPayload, String> {
    let discovery = args.as_ref().and_then(|a| {
        if a.discovery_mdns.unwrap_or(false) {
            Some(DiscoveryConfig {
                mdns: true,
                service_name: a.discovery_service_name.clone(),
                advertise: a.discovery_advertise.unwrap_or(true),
            })
        } else {
            None
        }
    });

    let opts = args
        .map(|a| NodeOptions {
            key: a.key.and_then(|k| B64.decode(k).ok()?.try_into().ok()),
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
            discovery: discovery.clone(),
            disable_networking: a.disable_networking.unwrap_or(false),
            drain_timeout_ms: a.drain_timeout,
            handle_ttl_ms: a.handle_ttl,
            max_pooled_connections: None,
            max_header_size: None,
            proxy_url: a.proxy_url,
            proxy_from_env: a.proxy_from_env.unwrap_or(false),
            keylog: a.keylog.unwrap_or(false),
            #[cfg(feature = "compression")]
            compression: if a.compression_level.is_some() || a.compression_min_body_bytes.is_some() {
                Some(iroh_http_core::CompressionOptions {
                    level: a.compression_level.unwrap_or(3),
                    min_body_bytes: a.compression_min_body_bytes.unwrap_or(512),
                })
            } else {
                None
            },
        })
        .unwrap_or_default();

    let ep = iroh_http_core::endpoint::IrohEndpoint::bind(opts)
        .await
        .map_err(|e| iroh_http_core::classify_error_json(e))?;

    // Wire up mDNS discovery if configured.
    #[cfg(feature = "discovery")]
    if let Some(ref disc) = discovery {
        if disc.mdns {
            let service_name = disc.service_name.as_deref()
                .ok_or_else(|| iroh_http_core::classify_error_json(
                    "discovery.serviceName is required when mdns is true"))?;
            iroh_http_discovery::add_mdns(ep.raw(), service_name, disc.advertise)
                .map_err(|e| iroh_http_core::classify_error_json(e))?;
        }
    }
    #[cfg(not(feature = "discovery"))]
    if discovery.as_ref().map_or(false, |d| d.mdns) {
        return Err(iroh_http_core::classify_error_json(
            "mDNS discovery was requested but this build was compiled without the \"discovery\" feature"
        ));
    }

    let node_id = ep.node_id().to_string();
    let keypair = ep.secret_key_bytes().to_vec();
    let handle = state::insert_endpoint(ep);

    Ok(EndpointInfoPayload {
        endpoint_handle: handle,
        node_id,
        keypair,
    })
}

/// Gracefully close an Iroh endpoint, draining in-flight requests.
#[command]
pub async fn close_endpoint(endpoint_handle: u32) -> Result<(), String> {
    let ep = state::remove_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    ep.close().await;
    Ok(())
}

// ── Ping (mobile lifecycle) ───────────────────────────────────────────────────

/// Trivial liveness probe — returns `true` when the endpoint exists.
#[command]
pub async fn ping(endpoint_handle: u32) -> Result<bool, String> {
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
pub fn node_addr(endpoint_handle: u32) -> Result<NodeAddrPayload, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    let info = ep.node_addr();
    Ok(NodeAddrPayload { id: info.id, addrs: info.addrs })
}

/// Home relay URL, or null if not connected to a relay.
#[command]
pub fn home_relay(endpoint_handle: u32) -> Result<Option<String>, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    Ok(ep.home_relay())
}

/// Known addresses for a remote peer, or null if unknown.
#[command]
pub async fn peer_info(endpoint_handle: u32, node_id: String) -> Result<Option<NodeAddrPayload>, String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    Ok(ep.peer_info(&node_id).await.map(|info| NodeAddrPayload { id: info.id, addrs: info.addrs }))
}

// ── Bridge methods ────────────────────────────────────────────────────────────

/// Read the next chunk from a body reader handle (base64-encoded).
#[command]
pub async fn next_chunk(handle: u32) -> Result<Option<String>, String> {
    let chunk = iroh_http_core::stream::next_chunk(handle).await.map_err(|e| iroh_http_core::classify_error_json(e))?;
    Ok(chunk.map(|b| B64.encode(&b[..])))
}

/// Push a base64-encoded chunk into a body writer handle.
#[command]
pub async fn send_chunk(handle: u32, chunk: String) -> Result<(), String> {
    let bytes = B64.decode(&chunk).map_err(|e| iroh_http_core::classify_error_json(format!("base64 decode: {e}")))?;
    iroh_http_core::stream::send_chunk(handle, Bytes::from(bytes)).await.map_err(|e| iroh_http_core::classify_error_json(e))
}

/// Signal end-of-body for a writer handle.
#[command]
pub fn finish_body(handle: u32) -> Result<(), String> {
    iroh_http_core::stream::finish_body(handle).map_err(|e| iroh_http_core::classify_error_json(e))
}

/// Cancel a body reader, signalling EOF.
#[command]
pub fn cancel_request(handle: u32) {
    iroh_http_core::stream::cancel_reader(handle);
}

/// Await and retrieve trailer headers from a completed request/response.
#[command]
pub async fn next_trailer(handle: u32) -> Result<Option<Vec<Vec<String>>>, String> {
    let trailers = iroh_http_core::stream::next_trailer(handle).await.map_err(|e| iroh_http_core::classify_error_json(e))?;
    Ok(trailers.map(|t| t.into_iter().map(|(k, v)| vec![k, v]).collect()))
}

/// Deliver response trailer headers to the Rust pump task.
#[command]
pub fn send_trailers(handle: u32, trailers: Vec<Vec<String>>) -> Result<(), String> {
    let pairs: Vec<(String, String)> = trailers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    iroh_http_core::stream::send_trailers(handle, pairs).map_err(|e| iroh_http_core::classify_error_json(e))
}

/// Allocate a body writer handle for streaming request bodies.
#[command]
pub fn alloc_body_writer() -> u32 {
    state::js_alloc_body_writer()
}

/// Allocate a cancellation token for an upcoming fetch call.
#[command]
pub fn alloc_fetch_token() -> u32 {
    iroh_http_core::alloc_fetch_token()
}

/// Cancel an in-flight fetch by its token.
#[command]
pub fn cancel_in_flight(token: u32) {
    iroh_http_core::cancel_in_flight(token);
}

// ── rawFetch ──────────────────────────────────────────────────────────────────

/// Arguments for `rawFetch` — send an HTTP request to a remote peer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawFetchArgs {
    pub endpoint_handle: u32,
    pub node_id: String,
    pub url: String,
    pub method: String,
    pub headers: Vec<Vec<String>>,
    pub req_body_handle: Option<u32>,
    pub fetch_token: Option<u32>,
    pub direct_addrs: Option<Vec<String>>,
}

/// Response payload returned by `rawFetch`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiResponsePayload {
    pub status: u16,
    pub headers: Vec<Vec<String>>,
    pub body_handle: u32,
    pub url: String,
    pub trailers_handle: u32,
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

    let addrs = parse_direct_addrs(&args.direct_addrs);
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
    pub req_handle: u32,
    pub req_body_handle: u32,
    pub res_body_handle: u32,
    pub req_trailers_handle: u32,
    pub res_trailers_handle: u32,
    pub is_bidi: bool,
    pub method: String,
    pub url: String,
    pub headers: Vec<Vec<String>>,
    pub remote_node_id: String,
}

/// Start the serve accept loop, streaming incoming requests via a Tauri Channel.
#[command]
pub async fn serve(
    endpoint_handle: u32,
    channel: Channel<ServeEventPayload>,
) -> Result<(), String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;

    let handle = iroh_http_core::serve(
        ep.clone(),
        ServeOptions { max_consecutive_errors: Some(ep.max_consecutive_errors()), ..Default::default() },
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RespondArgs {
    pub req_handle: u32,
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
    pub endpoint_handle: u32,
    pub node_id: String,
    pub path: String,
    pub headers: Vec<Vec<String>>,
}

/// Handles for a full-duplex QUIC stream.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiDuplexStreamPayload {
    pub read_handle: u32,
    pub write_handle: u32,
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

