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
        BodyReader, BodyWriter, TrailerRx,
        insert_reader, insert_writer, make_body_channel,
        insert_trailer_sender, insert_trailer_receiver, remove_trailer_sender,
    },
    IrohEndpoint, RequestPayload,
};
use iroh_http_framing::{parse_trailers, FramingError};

const READ_BUF: usize = 16 * 1024;
const DEFAULT_CONCURRENCY: usize = 64;
const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_MAX_CONNECTIONS_PER_PEER: usize = 8;
const DEFAULT_DRAIN_TIMEOUT_SECS: u64 = 30;

/// Options controlling the serve loop.
#[derive(Debug, Clone, Default)]
pub struct ServeOptions {
    /// Maximum number of concurrent in-flight requests.  `None` uses the default.
    pub max_concurrency: Option<usize>,
    /// Number of consecutive accept errors before the loop gives up.
    /// `None` uses the default (5).
    pub max_consecutive_errors: Option<usize>,
    /// Per-request timeout in milliseconds.  `None` disables the timeout.
    /// Default: 60000 (60 seconds).
    pub request_timeout_ms: Option<u64>,
    /// Maximum number of connections from a single peer.  Default: 8.
    pub max_connections_per_peer: Option<usize>,
    /// Maximum request body size in bytes.  `None` means unlimited.
    /// When exceeded, the stream is reset.  Default: None.
    pub max_request_body_bytes: Option<usize>,
    /// Drain timeout in seconds for graceful shutdown.  When `shutdown()` is
    /// called, the serve loop stops accepting new connections and waits up to
    /// this long for in-flight requests to complete.  Default: 30.
    pub drain_timeout_secs: Option<u64>,
}

/// Handle returned by [`serve`].
///
/// Dropping the handle does **not** stop the serve loop — the background task
/// continues independently.  Use [`shutdown`](ServeHandle::shutdown) for
/// graceful shutdown, or [`abort`](ServeHandle::abort) for immediate stop.
pub struct ServeHandle {
    join: tokio::task::JoinHandle<()>,
    shutdown_notify: std::sync::Arc<tokio::sync::Notify>,
    drain_timeout: std::time::Duration,
}

impl ServeHandle {
    /// Signal the serve loop to stop accepting new connections and drain
    /// in-flight requests.  Returns immediately — call [`drain`](ServeHandle::drain)
    /// to wait for completion.
    pub fn shutdown(&self) {
        self.shutdown_notify.notify_one();
    }

    /// Wait for the serve loop to finish draining (up to the configured
    /// drain timeout).  If the loop has not been shut down yet, this calls
    /// `shutdown()` first.
    pub async fn drain(self) {
        self.shutdown();
        let _ = self.join.await;
    }

    /// Immediately abort the serve loop without draining.
    pub fn abort(&self) {
        self.join.abort();
    }

    /// The configured drain timeout.
    pub fn drain_timeout(&self) -> std::time::Duration {
        self.drain_timeout
    }
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

/// Drop guard that removes a `pending_responses` entry if the serving task
/// is cancelled (timeout) or hits an error before `respond()` is called.
/// Call `.defuse()` after `rx.await` succeeds to prevent cleanup.
struct PendingGuard {
    handle: u32,
    active: bool,
}

impl PendingGuard {
    fn new(handle: u32) -> Self {
        Self { handle, active: true }
    }
    /// Prevent the guard from removing the entry on drop.
    fn defuse(&mut self) {
        self.active = false;
    }
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if self.active {
            pending_responses().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.handle);
        }
    }
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
/// Returns a [`ServeHandle`] that can be used to gracefully shut down the
/// serve loop.  Dropping the handle lets the loop run indefinitely.
pub fn serve<F>(
    endpoint: IrohEndpoint,
    options: ServeOptions,
    on_request: F,
) -> ServeHandle
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    let max = options.max_concurrency.unwrap_or(DEFAULT_CONCURRENCY);
    let max_errors = options.max_consecutive_errors.unwrap_or(5);
    let request_timeout = options
        .request_timeout_ms
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS));
    let max_conns_per_peer = options
        .max_connections_per_peer
        .unwrap_or(DEFAULT_MAX_CONNECTIONS_PER_PEER);
    let max_header_size = endpoint.max_header_size();
    let max_request_body_bytes = options.max_request_body_bytes;
    #[cfg(feature = "compression")]
    let compression = endpoint.compression().cloned();
    let drain_timeout = std::time::Duration::from_secs(
        options.drain_timeout_secs.unwrap_or(DEFAULT_DRAIN_TIMEOUT_SECS),
    );
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max));
    let on_request = std::sync::Arc::new(on_request);

    // Per-peer active connection counts.
    let peer_counts: std::sync::Arc<Mutex<HashMap<iroh::PublicKey, usize>>> =
        std::sync::Arc::new(Mutex::new(HashMap::new()));

    // Shutdown signal.
    let shutdown_notify = std::sync::Arc::new(tokio::sync::Notify::new());
    let shutdown_listen = shutdown_notify.clone();
    let drain_sem = semaphore.clone();
    let drain_max = max;
    let drain_dur = drain_timeout;

    let join = tokio::spawn(async move {
        let ep = endpoint.raw().clone();
        let mut consecutive_errors: usize = 0;

        loop {
            let incoming = tokio::select! {
                biased;
                _ = shutdown_listen.notified() => {
                    tracing::info!("iroh-http: serve loop shutting down (drain requested)");
                    break;
                }
                inc = ep.accept() => {
                    match inc {
                        Some(i) => i,
                        None => break, // endpoint closed
                    }
                }
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
                let mut counts = peer_counts.lock().unwrap_or_else(|e| e.into_inner());
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
            #[cfg(feature = "compression")]
            let comp = compression.clone();

            tokio::spawn(async move {
                handle_connection(
                    conn,
                    sem,
                    on_req,
                    ep_id,
                    request_timeout,
                    max_header_size,
                    max_request_body_bytes,
                    #[cfg(feature = "compression")]
                    comp,
                )
                .await;
                // Decrement peer count.
                let mut counts = pc.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(c) = counts.get_mut(&remote_id) {
                    *c = c.saturating_sub(1);
                    if *c == 0 {
                        counts.remove(&remote_id);
                    }
                }
            });
        }

        // Drain: wait for all in-flight requests to finish (all permits returned).
        // acquire_many(max) succeeds only when every permit is free.
        let drain_result = tokio::time::timeout(
            drain_dur,
            drain_sem.acquire_many(drain_max as u32),
        )
        .await;
        match drain_result {
            Ok(Ok(_permits)) => {
                tracing::info!("iroh-http: all in-flight requests drained");
            }
            Ok(Err(_)) => {
                tracing::warn!("iroh-http: semaphore closed during drain");
            }
            Err(_) => {
                tracing::warn!(
                    "iroh-http: drain timed out after {}s, force-closing remaining requests",
                    drain_dur.as_secs()
                );
            }
        }
    });

    ServeHandle {
        join,
        shutdown_notify,
        drain_timeout: drain_dur,
    }
}

async fn handle_connection<F>(
    conn: Connection,
    semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    on_request: std::sync::Arc<F>,
    own_node_id: String,
    request_timeout: std::time::Duration,
    max_header_size: usize,
    max_request_body_bytes: Option<usize>,
    #[cfg(feature = "compression")]
    compression: Option<crate::compress::CompressionOptions>,
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
        #[cfg(feature = "compression")]
        let comp = compression.clone();

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
                #[cfg(feature = "compression")]
                comp,
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

// ── Request dispatching ───────────────────────────────────────────────────────

/// Returned by [`dispatch_request`] — carries the async pieces the caller
/// needs to await and forward to [`write_response`].
struct DispatchResult {
    rx: oneshot::Receiver<ResponseHead>,
    pending_guard: PendingGuard,
    res_reader: BodyReader,
    opt_res_trailer_rx: Option<TrailerRx>,
    res_trailers_handle: u32,
}

/// Allocates all request/response body channels and trailer channels, spawns
/// the recv-to-body pump task, and fires `on_request`.
fn dispatch_request<F>(
    recv: iroh::endpoint::RecvStream,
    method: String,
    path: &str,
    own_node_id: String,
    remote_node_id: String,
    req_headers: Vec<(String, String)>,
    is_bidi: bool,
    leftover: Vec<u8>,
    max_request_body_bytes: Option<usize>,
    #[cfg(feature = "compression")]
    req_content_zstd: bool,
    on_request: std::sync::Arc<F>,
) -> DispatchResult
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    // Allocate request body channel; optionally wrap with decompressor.
    let (req_writer, req_reader) = make_body_channel();
    #[cfg(feature = "compression")]
    let req_reader = if req_content_zstd {
        crate::compress::decompress_body(req_reader)
    } else {
        req_reader
    };
    let req_body_handle = insert_reader(req_reader);

    // Allocate response body channel.
    let (res_writer, res_reader) = make_body_channel();
    let res_body_handle = insert_writer(res_writer);

    // Allocate oneshot for response head.
    let (tx, rx) = oneshot::channel::<ResponseHead>();
    let req_handle = allocate_req_handle(tx);
    let pending_guard = PendingGuard::new(req_handle);

    // Allocate trailer channels (skipped for duplex streams — raw bytes only).
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

    // Construct the full URL and spawn the recv pump.
    let url = format!("httpi://{own_node_id}{path}");
    if is_bidi {
        tokio::spawn(pump_recv_raw_to_body(recv, req_writer, leftover));
    } else {
        let req_is_chunked = req_headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("transfer-encoding")
                && v.to_ascii_lowercase().contains("chunked")
        });
        let rq_tx = opt_req_trailer_tx.expect("non-duplex req_trailer_tx");
        if req_is_chunked {
            tokio::spawn(pump_recv_to_body(
                recv, req_writer, leftover, rq_tx, max_request_body_bytes,
            ));
        } else {
            tokio::spawn(pump_recv_raw_to_body_limited(
                recv, req_writer, leftover, rq_tx,
                max_request_body_bytes.unwrap_or(usize::MAX),
            ));
        }
    }

    // Notify the handler.
    on_request(RequestPayload {
        req_handle,
        req_body_handle,
        res_body_handle,
        req_trailers_handle,
        res_trailers_handle,
        method,
        url,
        headers: req_headers,
        remote_node_id,
        is_bidi,
    });

    DispatchResult {
        rx,
        pending_guard,
        res_reader,
        opt_res_trailer_rx,
        res_trailers_handle,
    }
}

/// Encodes and writes the response head to `send`, optionally compresses the
/// response body, then pumps it to the QUIC send stream.
async fn write_response(
    send: &mut iroh::endpoint::SendStream,
    codec: &std::sync::Arc<tokio::sync::Mutex<crate::qpack_bridge::QpackCodec>>,
    response_head: ResponseHead,
    is_bidi: bool,
    res_reader: BodyReader,
    opt_res_trailer_rx: Option<TrailerRx>,
    res_trailers_handle: u32,
    #[cfg(feature = "compression")]
    client_accepts_zstd: bool,
    #[cfg(feature = "compression")]
    compression: Option<crate::compress::CompressionOptions>,
) -> Result<(), String> {
    // Determine whether to compress the response body.
    #[cfg(feature = "compression")]
    let compress_response = client_accepts_zstd
        && compression.is_some()
        && !response_head
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-encoding"))
        && {
            let min = compression.as_ref().map_or(512, |c| c.min_body_bytes);
            let content_length: Option<usize> = response_head
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.parse().ok());
            match content_length {
                Some(len) if len < min => false,
                _ => true,
            }
        };

    // Build final response headers, injecting Content-Encoding if compressing.
    #[allow(unused_mut)]
    let mut resp_headers = response_head.headers;
    #[cfg(feature = "compression")]
    if compress_response {
        resp_headers.push(("content-encoding".to_string(), "zstd".to_string()));
        resp_headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-length"));
    }

    let pairs: Vec<(&str, &str)> = resp_headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let res_chunked = !resp_headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
    let mut qpack_pairs = pairs.clone();
    if res_chunked && !is_bidi {
        qpack_pairs.push(("transfer-encoding", "chunked"));
    }

    // Encode and write the response head.
    let head_bytes = {
        let mut guard = codec.lock().await;
        guard
            .encode_response(response_head.status, &qpack_pairs)
            .map_err(|e| format!("qpack encode response: {e}"))?
    };
    send.write_all(&head_bytes)
        .await
        .map_err(|e| format!("write response head: {e}"))?;

    // Optionally compress the response body.
    #[cfg(feature = "compression")]
    let res_reader = if compress_response {
        let level = compression.as_ref().map_or(3, |c| c.level);
        crate::compress::compress_body(res_reader, level)
    } else {
        res_reader
    };

    // Pump the response body.
    if is_bidi {
        pump_body_raw_to_stream(res_reader, send).await?;
    } else {
        let has_trailer_header = resp_headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("trailer"));
        let trailer_rx_for_pump = if has_trailer_header {
            opt_res_trailer_rx
        } else {
            drop(opt_res_trailer_rx);
            remove_trailer_sender(res_trailers_handle);
            None
        };
        pump_body_to_stream(res_reader, send, res_chunked, trailer_rx_for_pump).await?;
    }

    Ok(())
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
    #[cfg(feature = "compression")]
    compression: Option<crate::compress::CompressionOptions>,
) -> Result<(), String>
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    // 1. Read and parse request head.
    #[allow(unused_mut)]
    let (method, path, mut req_headers, leftover) =
        read_request_head_qpack(&mut recv, &codec, max_header_size).await?;

    // 2. Detect duplex upgrade and request-side compression flags.
    let is_bidi = req_headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("upgrade") && v.eq_ignore_ascii_case("iroh-duplex")
    });
    #[cfg(feature = "compression")]
    let req_content_zstd = !is_bidi && req_headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("content-encoding") && crate::compress::is_zstd(v)
    });
    #[cfg(feature = "compression")]
    let client_accepts_zstd = !is_bidi && req_headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("accept-encoding") && v.to_ascii_lowercase().contains("zstd")
    });
    #[cfg(feature = "compression")]
    if req_content_zstd {
        req_headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-encoding"));
    }

    // 3. Allocate channels, spawn recv pump, and notify the handler.
    let DispatchResult {
        rx,
        mut pending_guard,
        res_reader,
        opt_res_trailer_rx,
        res_trailers_handle,
    } = dispatch_request(
        recv,
        method,
        &path,
        own_node_id,
        remote_node_id,
        req_headers,
        is_bidi,
        leftover,
        max_request_body_bytes,
        #[cfg(feature = "compression")]
        req_content_zstd,
        on_request,
    );

    // 4. Await the response head.
    let response_head = rx
        .await
        .map_err(|_| "JS handler dropped without responding")?;
    // respond() already removed the entry — prevent double-removal on drop.
    pending_guard.defuse();

    // 5. Write response head + body.
    write_response(
        &mut send,
        &codec,
        response_head,
        is_bidi,
        res_reader,
        opt_res_trailer_rx,
        res_trailers_handle,
        #[cfg(feature = "compression")]
        client_accepts_zstd,
        #[cfg(feature = "compression")]
        compression,
    )
    .await?;

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

/// Pump raw (non-chunked) bytes from a `RecvStream` into a `BodyWriter`, enforcing
/// a maximum body size.  Used for non-duplex requests that lack `Transfer-Encoding:
/// chunked`.  Drops the `trailer_tx` on completion so the trailer reader resolves.
async fn pump_recv_raw_to_body_limited(
    mut recv: iroh::endpoint::RecvStream,
    writer: BodyWriter,
    already_read: Vec<u8>,
    _trailer_tx: tokio::sync::oneshot::Sender<Vec<(String, String)>>,
    max_bytes: usize,
) {
    let mut total = 0usize;

    if !already_read.is_empty() {
        total += already_read.len();
        if total > max_bytes {
            tracing::warn!("iroh-http: raw body exceeds {max_bytes} byte limit");
            return;
        }
        let data = Bytes::copy_from_slice(&already_read);
        if writer.send_chunk(data).await.is_err() {
            return;
        }
    }

    loop {
        match recv.read_chunk(READ_BUF).await {
            Ok(Some(chunk)) => {
                total += chunk.bytes.len();
                if total > max_bytes {
                    tracing::warn!("iroh-http: raw body exceeds {max_bytes} byte limit");
                    break;
                }
                let data = Bytes::copy_from_slice(&chunk.bytes);
                if writer.send_chunk(data).await.is_err() {
                    break;
                }
            }
            _ => break,
        }
    }
    // _trailer_tx drops here → trailer reader resolves with `RecvError`
    // (no trailers for raw bodies).
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
