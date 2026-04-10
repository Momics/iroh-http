//! Tauri commands for the iroh-http plugin.

use bytes::Bytes;
use iroh_http_core::{
    endpoint::NodeOptions,
    server::{ServeOptions, respond},
    RequestPayload,
};
use serde::{Deserialize, Serialize};
use tauri::{command, ipc::Channel, Runtime};

use crate::state;

// ── Shared response / error type ──────────────────────────────────────────────

fn err(s: impl ToString) -> tauri::Error {
    tauri::Error::PluginInitialization("iroh-http".into(), s.to_string())
}

// ── Endpoint ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEndpointArgs {
    pub key: Option<Vec<u8>>,
    pub idle_timeout: Option<u64>,
    pub relays: Option<Vec<String>>,
    pub dns_discovery: Option<String>,
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
        })
        .unwrap_or_default();

    let ep = iroh_http_core::endpoint::IrohEndpoint::bind(opts)
        .await
        .map_err(|e| e)?;

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
        .ok_or_else(|| format!("invalid endpoint handle: {endpoint_handle}"))?;
    ep.close().await;
    Ok(())
}

// ── Bridge methods ────────────────────────────────────────────────────────────

#[command]
pub async fn next_chunk(handle: u32) -> Result<Option<Vec<u8>>, String> {
    let chunk = iroh_http_core::stream::next_chunk(handle).await?;
    Ok(chunk.map(|b| b.to_vec()))
}

#[command]
pub async fn send_chunk(handle: u32, chunk: Vec<u8>) -> Result<(), String> {
    iroh_http_core::stream::send_chunk(handle, Bytes::from(chunk)).await
}

#[command]
pub fn finish_body(handle: u32) -> Result<(), String> {
    iroh_http_core::stream::finish_body(handle)
}

#[command]
pub fn cancel_request(handle: u32) {
    iroh_http_core::stream::cancel_reader(handle);
}

#[command]
pub async fn next_trailer(handle: u32) -> Result<Option<Vec<Vec<String>>>, String> {
    let trailers = iroh_http_core::stream::next_trailer(handle).await?;
    Ok(trailers.map(|t| t.into_iter().map(|(k, v)| vec![k, v]).collect()))
}

#[command]
pub fn send_trailers(handle: u32, trailers: Vec<Vec<String>>) -> Result<(), String> {
    let pairs: Vec<(String, String)> = trailers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();
    iroh_http_core::stream::send_trailers(handle, pairs)
}

#[command]
pub fn alloc_body_writer() -> u32 {
    state::js_alloc_body_writer()
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
        .ok_or_else(|| format!("invalid endpoint handle: {}", args.endpoint_handle))?;

    let pairs: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();

    let req_body_reader = args.req_body_handle.and_then(state::claim_pending_reader);

    let res = iroh_http_core::fetch(&ep, &args.node_id, &args.url, &args.method, &pairs, req_body_reader)
        .await?;

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
pub async fn serve<R: Runtime>(
    endpoint_handle: u32,
    channel: Channel<ServeEventPayload>,
) -> Result<(), String> {
    let ep = state::get_endpoint(endpoint_handle)
        .ok_or_else(|| format!("invalid endpoint handle: {endpoint_handle}"))?;

    iroh_http_core::serve(
        ep,
        ServeOptions::default(),
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
    respond(args.req_handle, args.status, headers)
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
        .ok_or_else(|| format!("invalid endpoint handle: {}", args.endpoint_handle))?;

    let pairs: Vec<(String, String)> = args
        .headers
        .into_iter()
        .filter_map(|p| if p.len() == 2 { Some((p[0].clone(), p[1].clone())) } else { None })
        .collect();

    let duplex = iroh_http_core::raw_connect(&ep, &args.node_id, &args.path, &pairs)
        .await?;

    Ok(FfiDuplexStreamPayload {
        read_handle: duplex.read_handle,
        write_handle: duplex.write_handle,
    })
}
