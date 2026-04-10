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
        insert_trailer_sender, insert_trailer_receiver, remove_trailer_sender,
    }, IrohEndpoint, RequestPayload,
};
use iroh_http_framing::{parse_trailers, FramingError};

const READ_BUF: usize = 16 * 1024;
const DEFAULT_CONCURRENCY: usize = 64;
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;
const DEFAULT_MAX_CONNECTIONS_PER_PEER: usize = 8;

/// Options controlling the serve loop.
#[derive(Debug, Clone, Default)]
pub struct ServeOptions {
    /// Maximum number of concurrent in-flight requests.  `None` uses the default.
    pub max_concurrency: Option<usize>,
    /// Number of consecutive accept errors before the loop gives up.
    /// `None` uses the default (5).
    pub max_consecutive_errors: Option<usize>,
    /// Per-request timeout in seconds.  `None` disables the timeout.
    /// Default: 60.
    pub request_timeout_secs: Option<u64>,
    /// Maximum number of connections from a single peer.  Default: 8.
    pub max_connections_per_peer: Option<usize>,
    /// Maximum request body size in bytes.  `None` means unlimited.
    /// When exceeded, the stream is reset.  Default: None.
    pub max_request_body_bytes: Option<usize>,
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
    let max_errors = options.max_consecutive_errors.unwrap_or(5);
    let request_timeout = options
        .request_timeout_secs
        .map(std::time::Duration::from_secs)
        .unwrap_or(std::time::Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS));
    let max_conns_per_peer = options
        .max_connections_per_peer
        .unwrap_or(DEFAULT_MAX_CONNECTIONS_PER_PEER);
    let max_header_size = endpoint.max_header_size();
    let max_request_body_bytes = options.max_request_body_bytes;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max));
    let on_request = std::sync::Arc::new(on_request);

    // Per-peer active connection counts.
    let peer_counts: std::sync::Arc<Mutex<HashMap<iroh::PublicKey, usize>>> =
        std::sync::Arc::new(Mutex::new(HashMap::new()));

    tokio::spawn(async move {
        let ep = endpoint.raw().clone();
        let mut consecutive_errors: usize = 0;

        loop {
            let incoming = match ep.accept().await {
                Some(i) => i,
                None => break, // endpoint closed
            };

            let conn = match incoming.await {
                Ok(c) => {
                    consecutive_errors = 0;
                    c
                }
                Err(e) => {
                    consecutive_errors += 1;
                    tracing::warn!(
                        "iroh-http: accept error ({consecutive_errors}/{max_errors}): {e}"
                    );
                    if consecutive_errors >= max_errors {
                        tracing::error!(
                            "iroh-http: too many consecutive accept errors — shutting down serve loop"
                        );
                        break;
                    }
                    continue;
                }
            };

            // Per-peer connection limit.
            let remote_id = conn.remote_id();
            {
                let mut counts = peer_counts.lock().unwrap();
                let count = counts.entry(remote_id).or_insert(0);
                if *count >= max_conns_per_peer {
                    tracing::warn!("iroh-http: peer {} exceeded connection limit ({max_conns_per_peer})", crate::base32_encode(remote_id.as_bytes()));
                    conn.close(0u32.into(), b"too many connections");
                    continue;
                }
                *count += 1;
            }

            let sem = semaphore.clone();
            let on_req = on_request.clone();
            let ep_id = endpoint.node_id().to_string();
            let pc = peer_counts.clone();

            tokio::spawn(async move {
                handle_connection(
                    conn,
                    sem,
                    on_req,
                    ep_id,
                    request_timeout,
                    max_header_size,
                    max_request_body_bytes,
                )
                .await;
                // Decrement peer count.
                let mut counts = pc.lock().unwrap();
                if let Some(c) = counts.get_mut(&remote_id) {
                    *c = c.saturating_sub(1);
                    if *c == 0 {
                        counts.remove(&remote_id);
                    }
                }
            });
        }
    })
}

async fn handle_connection<F>(
    conn: Connection,
    semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    on_request: std::sync::Arc<F>,
    own_node_id: String,
    request_timeout: std::time::Duration,
    max_header_size: usize,
    max_request_body_bytes: Option<usize>,
) where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    let remote_id = base32_encode(conn.remote_id().as_bytes());

    let codec = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::qpack_bridge::QpackCodec::new(),
    ));

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
        let codec_clone = codec.clone();

        tokio::spawn(async move {
            let _permit = permit; // held for duration of request
            let fut = handle_stream(
                send,
                recv,
                on_req,
                remote,
                own,
                codec_clone,
                max_header_size,
                max_request_body_bytes,
            );
            if request_timeout.is_zero() {
                // Timeout disabled.
                if let Err(e) = fut.await {
                    tracing::warn!("iroh-http: stream error: {e}");
                }
            } else {
                match tokio::time::timeout(request_timeout, fut).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => tracing::warn!("iroh-http: stream error: {e}"),
                    Err(_) => tracing::warn!("iroh-http: request timed out"),
                }
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
    codec: std::sync::Arc<tokio::sync::Mutex<crate::qpack_bridge::QpackCodec>>,
    max_header_size: usize,
    max_request_body_bytes: Option<usize>,
) -> Result<(), String>
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    // 1. Read and parse request head.
    let (method, path, req_headers, leftover) =
        read_request_head_qpack(&mut recv, &codec, max_header_size).await?;

    // 2. Detect duplex upgrade.
    let is_bidi = req_headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("upgrade") && v.eq_ignore_ascii_case("iroh-duplex")
    });

    // 3. Allocate request body channel.
    let (req_writer, req_reader) = make_body_channel();
    let req_body_handle = insert_reader(req_reader);

    // 4. Allocate response body channel.
    let (res_writer, res_reader) = make_body_channel();
    let res_body_handle = insert_writer(res_writer);

    // 5. Allocate oneshot for response head.
    let (tx, rx) = oneshot::channel::<ResponseHead>();
    let req_handle = allocate_req_handle(tx);

    // 6. Allocate trailer channels (skipped for duplex — raw bytes only).
    let (opt_req_trailer_tx, opt_res_trailer_rx, req_trailers_handle, res_trailers_handle) =
        if !is_bidi {
            let (rq_tx, rq_rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
            let rq_h = insert_trailer_receiver(rq_rx);

            let (rs_tx, rs_rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
            let rs_h = insert_trailer_sender(rs_tx);

            (Some(rq_tx), Some(rs_rx), rq_h, rs_h)
        } else {
            (None, None, 0u32, 0u32)
        };

    // 7. Construct the full URL.
    let url = format!("httpi://{own_node_id}{path}");

    // 8. Spawn recv pump task.
    if is_bidi {
        tokio::spawn(pump_recv_raw_to_body(recv, req_writer, leftover));
    } else {
        let rq_tx = opt_req_trailer_tx.expect("non-duplex req_trailer_tx");
        tokio::spawn(pump_recv_to_body(recv, req_writer, leftover, rq_tx, max_request_body_bytes));
    }

    // 9. Notify JS.
    on_request(RequestPayload {
        req_handle,
        req_body_handle,
        res_body_handle,
        req_trailers_handle,
        res_trailers_handle,
        method: method.clone(),
        url,
        headers: req_headers,
        remote_node_id,
        is_bidi,
    });

    // 10. Await JS response head.
    let response_head = rx
        .await
        .map_err(|_| "JS handler dropped without responding")?;

    // 11. Write response head.
    let pairs: Vec<(&str, &str)> = response_head
        .headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    // Only use chunked encoding when the response does NOT carry a Content-Length.
    // If Content-Length is set, send the raw body bytes as-is so the framing
    // matches what the client expects from the headers.
    let res_chunked = !response_head
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));

    // When chunked encoding is needed, inject Transfer-Encoding: chunked
    // into the QPACK-encoded headers so the client decodes correctly.
    let mut qpack_pairs = pairs.clone();
    if res_chunked && !is_bidi {
        qpack_pairs.push(("transfer-encoding", "chunked"));
    }
    let head_bytes = {
        let mut guard = codec.lock().await;
        guard.encode_response(response_head.status, &qpack_pairs)
            .map_err(|e| format!("qpack encode response: {e}"))?
    };
    send.write_all(&head_bytes)
        .await
        .map_err(|e| format!("write response head: {e}"))?;

    // 12. Pump response body.
    if is_bidi {
        // Duplex: raw bytes, no chunked encoding, no trailers.
        pump_body_raw_to_stream(res_reader, &mut send).await?;
    } else {
        let has_trailer_header = response_head
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("trailer"));
        let trailer_rx_for_pump = if has_trailer_header {
            opt_res_trailer_rx
        } else {
            // Handler did not declare trailers — drop the receiver and
            // clean up the sender from the slab so it doesn't leak.
            drop(opt_res_trailer_rx);
            remove_trailer_sender(res_trailers_handle);
            None
        };
        pump_body_to_stream(res_reader, &mut send, res_chunked, trailer_rx_for_pump).await?;
    }

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

/// Read a QPACK-encoded request head from the stream.
///
/// Wire format: `[2-byte big-endian length][QPACK block]`.
/// Returns `(method, path, headers, leftover_bytes)`.
///
/// The buffer is bounded to `max_header_size` bytes.  If the peer sends a
/// head larger than this, the stream is rejected with an error.
async fn read_request_head_qpack(
    recv: &mut iroh::endpoint::RecvStream,
    codec: &std::sync::Arc<tokio::sync::Mutex<crate::qpack_bridge::QpackCodec>>,
    max_header_size: usize,
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

        if buf.len() > max_header_size {
            return Err(format!("request head too large ({} bytes, limit {max_header_size})", buf.len()));
        }

        let mut guard = codec.lock().await;
        match guard.decode_request(&buf) {
            Ok((method, path, headers, consumed)) => {
                let leftover = buf[consumed..].to_vec();
                return Ok((method, path, headers, leftover));
            }
            Err(crate::qpack_bridge::DecodeError::Incomplete) => continue,
            Err(e) => return Err(format!("parse request head: {e}")),
        }
    }
}

/// Pump a `RecvStream` into a `BodyWriter` channel, handling chunked encoding.
/// `already_read` is bytes already consumed during head parsing.
/// Trailer bytes after the terminal chunk are parsed and delivered via `trailer_tx`.
/// When `max_body_bytes` is `Some(n)`, the stream is abandoned after `n` bytes.
async fn pump_recv_to_body(
    mut recv: iroh::endpoint::RecvStream,
    writer: BodyWriter,
    already_read: Vec<u8>,
    trailer_tx: tokio::sync::oneshot::Sender<Vec<(String, String)>>,
    max_body_bytes: Option<usize>,
) {
    let mut buf = already_read;
    let mut total_body_bytes: usize = 0;

    loop {
        // Drain chunked data from buffer first.
        loop {
            match iroh_http_framing::parse_chunk_header(&buf) {
                None => break, // need more bytes
                Some((0, header_consumed)) => {
                    let after_header = buf[header_consumed..].to_vec();
                    let trailers = read_trailers_from_buf(&mut recv, after_header).await;
                    let _ = trailer_tx.send(trailers);
                    return; // terminal chunk → EOF
                }
                Some((size, header_len)) => {
                    let data_end = header_len + size;
                    let trailer_end = data_end + 2;
                    if buf.len() < trailer_end {
                        break;
                    }
                    total_body_bytes += size;
                    if let Some(limit) = max_body_bytes {
                        if total_body_bytes > limit {
                            tracing::warn!("iroh-http: request body exceeded {limit} bytes, resetting stream");
                            let _ = recv.stop(0u32.into());
                            return;
                        }
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

/// Pump raw (unchunked) bytes from a `RecvStream` into a `BodyWriter` channel.
/// Used for duplex connections where no HTTP framing is applied after headers.
async fn pump_recv_raw_to_body(
    mut recv: iroh::endpoint::RecvStream,
    writer: BodyWriter,
    already_read: Vec<u8>,
) {
    if !already_read.is_empty() {
        let data = Bytes::copy_from_slice(&already_read);
        if writer.send_chunk(data).await.is_err() {
            return;
        }
    }
    loop {
        match recv.read_chunk(READ_BUF).await {
            Ok(Some(chunk)) => {
                let data = Bytes::copy_from_slice(&chunk.bytes);
                if writer.send_chunk(data).await.is_err() {
                    break;
                }
            }
            _ => break,
        }
    }
}

/// Pump raw bytes from a `BodyReader` channel to a `SendStream` without chunked encoding.
/// Used for duplex connections.
async fn pump_body_raw_to_stream(
    reader: crate::stream::BodyReader,
    send: &mut iroh::endpoint::SendStream,
) -> Result<(), String> {
    loop {
        match reader.next_chunk().await {
            None => break,
            Some(data) => {
                send.write_all(&data)
                    .await
                    .map_err(|e| format!("write duplex chunk: {e}"))?;
            }
        }
    }
    Ok(())
}

/// Read a complete trailer block from a stream, starting with `buf`.
/// Returns the parsed trailers, or an empty `Vec` on parse failure or early EOF.
async fn read_trailers_from_buf(
    recv: &mut iroh::endpoint::RecvStream,
    mut buf: Vec<u8>,
) -> Vec<(String, String)> {
    loop {
        match parse_trailers(&buf) {
            Ok((trailers, _)) => return trailers,
            Err(FramingError::Incomplete) => match recv.read_chunk(READ_BUF).await {
                Ok(Some(chunk)) => buf.extend_from_slice(&chunk.bytes),
                _ => return Vec::new(),
            },
            Err(_) => return Vec::new(),
        }
    }
}
