//! napi-rs bindings for iroh-http-node.
//!
//! Exposes the full bridge interface to Node.js:
//! `createEndpoint`, `nextChunk`, `sendChunk`, `finishBody`,
//! `allocBodyWriter`, `rawFetch`, `rawServe`, `closeEndpoint`.

#![deny(clippy::all)]

use std::sync::{Arc, Mutex, OnceLock};

use bytes::Bytes;
use iroh_http_core::{
    endpoint::{IrohEndpoint, NodeOptions},
    server::{ServeOptions, respond},
    stream::{
        alloc_body_writer, claim_pending_reader, finish_body,
        next_chunk, send_chunk, store_pending_reader,
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
        .ok_or_else(|| napi::Error::new(Status::InvalidArg, format!("invalid endpoint handle: {handle}")))
}

// ── Endpoint lifecycle ────────────────────────────────────────────────────────

#[napi(object)]
pub struct JsNodeOptions {
    pub key: Option<Uint8Array>,
    pub idle_timeout: Option<f64>,
    pub relays: Option<Vec<String>>,
    pub dns_discovery: Option<String>,
}

#[napi(object)]
pub struct JsEndpointInfo {
    pub endpoint_handle: u32,
    pub node_id: String,
    pub keypair: Uint8Array,
}

#[napi]
pub async fn create_endpoint(options: Option<JsNodeOptions>) -> napi::Result<JsEndpointInfo> {
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
    }).unwrap_or_default();

    let ep = IrohEndpoint::bind(opts)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, e))?;

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
    let ep = {
        let mut slab = endpoint_slab().lock().unwrap();
        if !slab.contains(endpoint_handle as usize) {
            return Err(napi::Error::new(Status::InvalidArg, "invalid endpoint handle"));
        }
        slab.remove(endpoint_handle as usize)
    };
    ep.close().await;
    Ok(())
}

// ── Bridge methods ────────────────────────────────────────────────────────────

#[napi]
pub async fn js_next_chunk(handle: u32) -> napi::Result<Option<Uint8Array>> {
    let chunk = next_chunk(handle)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, e))?;
    Ok(chunk.map(|b| Uint8Array::new(b.to_vec())))
}

#[napi]
pub async fn js_send_chunk(handle: u32, chunk: Uint8Array) -> napi::Result<()> {
    let bytes = Bytes::copy_from_slice(chunk.as_ref());
    send_chunk(handle, bytes)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, e))
}

#[napi]
pub fn js_finish_body(handle: u32) -> napi::Result<()> {
    finish_body(handle).map_err(|e| napi::Error::new(Status::GenericFailure, e))
}

#[napi]
pub fn js_alloc_body_writer() -> u32 {
    let (handle, reader) = alloc_body_writer();
    store_pending_reader(handle, reader);
    handle
}

// ── rawFetch ──────────────────────────────────────────────────────────────────

#[napi(object)]
pub struct JsFfiResponse {
    pub status: u32,
    pub headers: Vec<Vec<String>>,
    pub body_handle: u32,
    pub url: String,
}

#[napi]
pub async fn raw_fetch(
    endpoint_handle: u32,
    node_id: String,
    url: String,
    method: String,
    headers: Vec<Vec<String>>,
    req_body_handle: Option<u32>,
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

    let res = iroh_http_core::fetch(&ep, &node_id, &url, &method, &pairs, req_body_reader)
        .await
        .map_err(|e| napi::Error::new(Status::GenericFailure, e))?;

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
    })
}

// ── rawServe ──────────────────────────────────────────────────────────────────

#[napi(object)]
pub struct JsResponseHead {
    pub status: u32,
    pub headers: Vec<Vec<String>>,
}

#[napi]
pub fn raw_serve(
    endpoint_handle: u32,
    handler: JsFunction,
) -> napi::Result<()> {
    let ep = get_endpoint(endpoint_handle)?;

    type CallArgs = RequestPayload;
    let tsfn: ThreadsafeFunction<CallArgs, ErrorStrategy::Fatal> = handler
        .create_threadsafe_function(0, |ctx: ThreadSafeCallContext<CallArgs>| {
            let env = ctx.env;
            let p = ctx.value;

            let mut obj = env.create_object()?;
            obj.set("reqHandle", env.create_uint32(p.req_handle)?)?;
            obj.set("reqBodyHandle", env.create_uint32(p.req_body_handle)?)?;
            obj.set("resBodyHandle", env.create_uint32(p.res_body_handle)?)?;
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
        ep,
        ServeOptions::default(),
        move |payload: RequestPayload| {
            let tsfn = Arc::clone(&tsfn);
            let req_handle = payload.req_handle;
            tokio::spawn(async move {
                let result: napi::Result<JsResponseHead> = tsfn.call_async(payload).await;
                match result {
                    Ok(head) => {
                        let headers: Vec<(String, String)> = head
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
                        if let Err(e) = respond(req_handle, head.status as u16, headers) {
                            tracing::warn!("iroh-http-node: respond error: {e}");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("iroh-http-node: handler error: {e}");
                        let _ = respond(req_handle, 500, vec![]);
                    }
                }
            });
        },
    );

    Ok(())
}
