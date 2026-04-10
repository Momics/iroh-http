//! Incoming HTTP request — `serve()` implementation.
//!
//! The server accept loop runs as a Tokio background task.  For each incoming
//! bidi QUIC stream it:
//!   1. Parses the HTTP/1.1 request head.
//!   2. Allocates body channels for the request and response.
//!   3. Calls the JS-supplied callback via a `oneshot`-based request registry.
//!   4. Writes the response head, then pumps the response body.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use bytes::Bytes;
use iroh::endpoint::Connection;
use tokio::sync::oneshot;

use crate::{
    base32_encode,
    client::pump_body_to_stream,
    stream::{
        insert_reader, insert_writer, make_body_channel, BodyWriter,
    }, IrohEndpoint, RequestPayload,
};
use iroh_http_framing::{parse_request_head, reason_phrase, serialize_response_head, FramingError};

const READ_BUF: usize = 16 * 1024;
const DEFAULT_CONCURRENCY: usize = 64;

/// Options controlling the serve loop.
#[derive(Debug, Clone, Default)]
pub struct ServeOptions {
    /// Maximum number of concurrent in-flight requests.  `None` uses the default.
    pub max_concurrency: Option<usize>,
}

// ── Pending response head registry ───────────────────────────────────────────

struct ResponseHead {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

fn pending_responses() -> &'static Mutex<HashMap<u32, oneshot::Sender<ResponseHead>>> {
    static S: OnceLock<Mutex<HashMap<u32, oneshot::Sender<ResponseHead>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Called from the napi/tauri layer when JS has decided on the response head.
///
/// Wakes the waiting Rust task so it can write the status line + headers to
/// the QUIC stream and start pumping the response body.
pub fn respond(req_handle: u32, status: u16, headers: Vec<(String, String)>) -> Result<(), String> {
    let sender = pending_responses()
        .lock()
        .unwrap()
        .remove(&req_handle)
        .ok_or_else(|| format!("unknown req_handle: {req_handle}"))?;
    sender
        .send(ResponseHead { status, headers })
        .map_err(|_| "serve task dropped before respond".to_string())
}

// ── Accept loop ───────────────────────────────────────────────────────────────

/// Start the serve accept loop as a Tokio background task.
///
/// `on_request` is called for every incoming request.  It receives a
/// [`RequestPayload`] and must eventually call [`respond`] with the
/// response head and write/finish `payload.res_body_handle`.
///
/// The returned handle is a `JoinHandle`; the caller can drop it to allow
/// the task to run indefinitely in the background.
pub fn serve<F>(
    endpoint: IrohEndpoint,
    options: ServeOptions,
    on_request: F,
) -> tokio::task::JoinHandle<()>
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    let max = options.max_concurrency.unwrap_or(DEFAULT_CONCURRENCY);
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max));
    let on_request = std::sync::Arc::new(on_request);

    tokio::spawn(async move {
        let ep = endpoint.raw().clone();

        while let Some(incoming) = ep.accept().await {
            let conn = match incoming.await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("iroh-http: accept error: {e}");
                    continue;
                }
            };

            let sem = semaphore.clone();
            let on_req = on_request.clone();
            let ep_id = endpoint.node_id().to_string();

            tokio::spawn(async move {
                handle_connection(conn, sem, on_req, ep_id).await;
            });
        }
    })
}

async fn handle_connection<F>(
    conn: Connection,
    semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    on_request: std::sync::Arc<F>,
    own_node_id: String,
) where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    let remote_id = base32_encode(conn.remote_id().as_bytes());

    loop {
        let (send, recv) = match conn.accept_bi().await {
            Ok(pair) => pair,
            Err(_) => break,
        };

        let permit = match semaphore.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => break,
        };

        let on_req = on_request.clone();
        let remote = remote_id.clone();
        let own = own_node_id.clone();

        tokio::spawn(async move {
            let _permit = permit; // held for duration of request
            if let Err(e) = handle_stream(send, recv, on_req, remote, own).await {
                tracing::warn!("iroh-http: stream error: {e}");
            }
        });
    }
}

async fn handle_stream<F>(
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
    on_request: std::sync::Arc<F>,
    remote_node_id: String,
    own_node_id: String,
) -> Result<(), String>
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    // 1. Read and parse request head.
    let (method, path, req_headers, leftover) = read_request_head(&mut recv).await?;

    // 2. Allocate request body channel (reader in global slab, writer pumped from stream).
    let (req_writer, req_reader) = make_body_channel();
    let req_body_handle = insert_reader(req_reader);

    // 3. Allocate response body channel.
    let (res_writer, res_reader) = make_body_channel();
    let res_body_handle = insert_writer(res_writer);

    // 4. Allocate oneshot for response head.
    let (tx, rx) = oneshot::channel::<ResponseHead>();
    let req_handle = allocate_req_handle(tx);

    // 5. Construct the full URL (server side: http+iroh://<own-node-id>/path).
    let url = format!("http+iroh://{own_node_id}{path}");

    // 6. Spawn pump task: stream → reqBodyWriter channel.
    tokio::spawn(pump_recv_to_body(recv, req_writer, leftover));

    // 7. Notify JS.
    on_request(RequestPayload {
        req_handle,
        req_body_handle,
        res_body_handle,
        method: method.clone(),
        url,
        headers: req_headers,
        remote_node_id,
    });

    // 8. Await JS response head.
    let response_head = rx.await.map_err(|_| "JS handler dropped without responding")?;

    // 9. Write response head.
    let pairs: Vec<(&str, &str)> = response_head
        .headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let head_bytes = serialize_response_head(
        response_head.status,
        reason_phrase(response_head.status),
        &pairs,
        true, // chunked
    );
    send.write_all(&head_bytes)
        .await
        .map_err(|e| format!("write response head: {e}"))?;

    // 10. Pump response body (from JS's sendChunk calls) to QUIC send stream.
    pump_body_to_stream(res_reader, &mut send, true).await?;

    send.finish().map_err(|e| format!("finish stream: {e}"))?;

    Ok(())
}

// ── Helper: allocate a req_handle ─────────────────────────────────────────────

static NEXT_REQ_HANDLE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(1);

fn allocate_req_handle(sender: oneshot::Sender<ResponseHead>) -> u32 {
    let handle = NEXT_REQ_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    pending_responses()
        .lock()
        .unwrap()
        .insert(handle, sender);
    handle
}

// ── I/O helpers ───────────────────────────────────────────────────────────────

async fn read_request_head(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<(String, String, Vec<(String, String)>, Vec<u8>), String> {
    let mut buf: Vec<u8> = Vec::new();

    loop {
        match recv
            .read_chunk(READ_BUF)
            .await
            .map_err(|e| format!("read: {e}"))?
        {
            None => return Err("stream closed before complete request head".into()),
            Some(chunk) => buf.extend_from_slice(&chunk.bytes),
        }

        match parse_request_head(&buf) {
            Ok((method, path, headers, consumed)) => {
                let leftover = buf[consumed..].to_vec();
                return Ok((method, path, headers, leftover));
            }
            Err(FramingError::Incomplete) => continue,
            Err(FramingError::Parse(e)) => return Err(format!("parse request head: {e}")),
        }
    }
}

/// Pump a `RecvStream` into a `BodyWriter` channel, handling chunked encoding.
/// `already_read` is bytes already consumed during head parsing.
async fn pump_recv_to_body(
    mut recv: iroh::endpoint::RecvStream,
    writer: BodyWriter,
    already_read: Vec<u8>,
) {
    let mut buf = already_read;

    loop {
        // Drain chunked data from buffer first.
        loop {
            match iroh_http_framing::parse_chunk_header(&buf) {
                None => break, // need more bytes
                Some((0, _)) => return, // terminal chunk → EOF
                Some((size, header_len)) => {
                    let data_end = header_len + size;
                    let trailer_end = data_end + 2;
                    if buf.len() < trailer_end {
                        break;
                    }
                    let data = Bytes::copy_from_slice(&buf[header_len..data_end]);
                    if writer.send_chunk(data).await.is_err() {
                        return;
                    }
                    buf.drain(..trailer_end);
                }
            }
        }

        match recv.read_chunk(READ_BUF).await {
            Err(_) | Ok(None) => {
                // Stream finished; flush any remaining raw bytes.
                if !buf.is_empty() {
                    let data = Bytes::copy_from_slice(&buf);
                    let _ = writer.send_chunk(data).await;
                }
                return;
            }
            Ok(Some(chunk)) => buf.extend_from_slice(&chunk.bytes),
        }
    }
}
