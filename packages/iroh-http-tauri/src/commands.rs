//! Tauri commands for the iroh-http plugin.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use bytes::Bytes;
use iroh_http_core::{
    endpoint::NodeOptions,
    server::{ServeOptions, respond},
    RequestPayload,
};
use serde::{Deserialize, Serialize};
use tauri::{command, ipc::Channel};

use crate::state;

// ── Endpoint ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEndpointArgs {
    pub key: Option<Vec<u8>>,
    pub idle_timeout: Option<u64>,
    pub relays: Option<Vec<String>>,
    pub dns_discovery: Option<String>,
    pub channel_capacity: Option<usize>,
    pub max_chunk_size_bytes: Option<usize>,
    pub max_consecutive_errors: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointInfoPayload {
    pub endpoint_handle: u32,
    pub node_id: String,
    pub keypair: Vec<u8>,
}

#[command]
pub async fn create_endpoint(
    args: Option<CreateEndpointArgs>,
) -> Result<EndpointInfoPayload, String> {
    let opts = args
        .map(|a| NodeOptions {
            key: a.key.and_then(|k| k.try_into().ok()),
            idle_timeout_ms: a.idle_timeout,
            relays: a.relays.unwrap_or_default(),
            dns_discovery: a.dns_discovery,
            capabilities: Vec::new(), // advertise all by default
            channel_capacity: a.channel_capacity,
            max_chunk_size_bytes: a.max_chunk_size_bytes,
            max_consecutive_errors: a.max_consecutive_errors,
        })
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

#[command]
pub async fn close_endpoint(endpoint_handle: u32) -> Result<(), String> {
    let ep = state::remove_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;
    ep.close().await;
    Ok(())
}

// ── Bridge methods ────────────────────────────────────────────────────────────

#[command]
pub async fn next_chunk(handle: u32) -> Result<Option<String>, String> {
    let chunk = iroh_http_core::stream::next_chunk(handle).await.map_err(|e| iroh_http_core::classify_error_json(e))?;
    Ok(chunk.map(|b| B64.encode(&b[..])))
}

#[command]
pub async fn send_chunk(handle: u32, chunk: String) -> Result<(), String> {
    let bytes = B64.decode(&chunk).map_err(|e| iroh_http_core::classify_error_json(format!("base64 decode: {e}")))?;
    iroh_http_core::stream::send_chunk(handle, Bytes::from(bytes)).await.map_err(|e| iroh_http_core::classify_error_json(e))
}

#[command]
pub fn finish_body(handle: u32) -> Result<(), String> {
    iroh_http_core::stream::finish_body(handle).map_err(|e| iroh_http_core::classify_error_json(e))
}

#[command]
pub fn cancel_request(handle: u32) {
    iroh_http_core::stream::cancel_reader(handle);
}

#[command]
pub async fn next_trailer(handle: u32) -> Result<Option<Vec<Vec<String>>>, String> {
    let trailers = iroh_http_core::stream::next_trailer(handle).await.map_err(|e| iroh_http_core::classify_error_json(e))?;
    Ok(trailers.map(|t| t.into_iter().map(|(k, v)| vec![k, v]).collect()))
}

#[command]
pub fn send_trailers(handle: u32, trailers: Vec<Vec<String>>) -> Result<(), String> {
    let pairs: Vec<(String, String)> = trailers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    iroh_http_core::stream::send_trailers(handle, pairs).map_err(|e| iroh_http_core::classify_error_json(e))
}

#[command]
pub fn alloc_body_writer() -> u32 {
    state::js_alloc_body_writer()
}

#[command]
pub fn alloc_fetch_token() -> u32 {
    iroh_http_core::alloc_fetch_token()
}

#[command]
pub fn cancel_in_flight(token: u32) {
    iroh_http_core::cancel_in_flight(token);
}

// ── rawFetch ──────────────────────────────────────────────────────────────────

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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiResponsePayload {
    pub status: u16,
    pub headers: Vec<Vec<String>>,
    pub body_handle: u32,
    pub url: String,
    pub trailers_handle: u32,
}

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

    let res = iroh_http_core::fetch(&ep, &args.node_id, &args.url, &args.method, &pairs, req_body_reader, args.fetch_token)
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

/// Start the serve accept loop.
///
/// Incoming requests are pushed through `channel` as `ServeEventPayload`
/// objects.  JS processes each request and calls `respond_to_request` to
/// send the response head back.
#[command]
pub async fn serve(
    endpoint_handle: u32,
    channel: Channel<ServeEventPayload>,
) -> Result<(), String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| iroh_http_core::classify_error_json(format!("invalid endpoint handle: {endpoint_handle}")))?;

    iroh_http_core::serve(
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

    Ok(())
}

// ── respond_to_request ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RespondArgs {
    pub req_handle: u32,
    pub status: u16,
    pub headers: Vec<Vec<String>>,
}

/// Send the response head for a pending request.
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawConnectArgs {
    pub endpoint_handle: u32,
    pub node_id: String,
    pub path: String,
    pub headers: Vec<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfiDuplexStreamPayload {
    pub read_handle: u32,
    pub write_handle: u32,
}

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
