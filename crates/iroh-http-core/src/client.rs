//! Outgoing HTTP request — `fetch()` implementation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicU32, Ordering};

use bytes::Bytes;
use iroh::endpoint::Connection;


use crate::{
    base32_encode, parse_node_id,
    stream::{BodyReader, BodyWriter, make_body_channel, insert_reader, insert_writer},
    FfiResponse, FfiDuplexStream, IrohEndpoint, ALPN, ALPN_DUPLEX,
};
use iroh_http_framing::{
    encode_chunk, terminal_chunk,
    terminal_chunk_start, serialize_trailers, parse_trailers, FramingError,
};

const READ_BUF: usize = 16 * 1024;

// ── In-flight fetch cancellation ──────────────────────────────────────────────

static NEXT_FETCH_TOKEN: AtomicU32 = AtomicU32::new(1);

fn in_flight_map() -> &'static Mutex<HashMap<u32, Arc<tokio::sync::Notify>>> {
    static MAP: OnceLock<Mutex<HashMap<u32, Arc<tokio::sync::Notify>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Allocate a cancellation token for an upcoming `fetch` call.
///
/// Call this before `rawFetch`, wire `AbortSignal → cancel_in_flight(token)`,
/// and pass `token` to the platform's `rawFetch`/`fetch` binding.  The token
/// is automatically removed from the map when the fetch completes.
pub fn alloc_fetch_token() -> u32 {
    let id = NEXT_FETCH_TOKEN.fetch_add(1, Ordering::Relaxed);
    let notify = Arc::new(tokio::sync::Notify::new());
    in_flight_map().lock().unwrap().insert(id, notify);
    id
}

/// Signal an in-flight fetch to abort.  Safe to call after the fetch has
/// already completed — it is a no-op in that case.
pub fn cancel_in_flight(token_id: u32) {
    if let Some(notify) = in_flight_map().lock().unwrap().get(&token_id) {
        notify.notify_one();
    }
}

/// Send an HTTP/1.1 request to a remote node and return the response.
///
/// `req_body_reader` — optional body channel that the caller will pump
/// from the JS side via `sendChunk`/`finishBody`.  `None` for bodyless methods.
///
/// `fetch_token` — optional cancellation token previously allocated with
/// `alloc_fetch_token()`.  When `cancel_in_flight(token)` is called from
/// another thread/task while this future is running, the fetch is dropped
/// and the underlying QUIC streams are reset.
pub async fn fetch(
    endpoint: &IrohEndpoint,
    remote_node_id: &str,
    url: &str,
    method: &str,
    headers: &[(String, String)],
    req_body_reader: Option<BodyReader>,
    fetch_token: Option<u32>,
    direct_addrs: Option<&[std::net::SocketAddr]>,
) -> Result<FfiResponse, String> {
    // Retrieve the cancellation Notify for this token, if any.
    let cancel_notify = fetch_token.and_then(|id| {
        in_flight_map().lock().unwrap().get(&id).cloned()
    });

    let node_id = parse_node_id(remote_node_id)?;
    let mut addr = iroh::EndpointAddr::new(node_id);
    if let Some(addrs) = direct_addrs {
        for a in addrs {
            addr = addr.with_ip_addr(*a);
        }
    }

    let ep_raw = endpoint.raw().clone();
    let addr_clone = addr.clone();

    let pooled = endpoint
        .pool()
        .get_or_connect(node_id, ALPN, || async move {
            ep_raw
                .connect(addr_clone, ALPN)
                .await
                .map_err(|e| format!("connect: {e}"))
        })
        .await?;

    let conn = pooled.conn.clone();
    let codec = pooled.codec.clone();

    let result = do_request(
        conn,
        url,
        method,
        headers,
        req_body_reader,
        codec,
    );

    let out = if let Some(notify) = cancel_notify {
        tokio::select! {
            _ = notify.notified() => Err("aborted".to_string()),
            r = result => r,
        }
    } else {
        result.await
    };

    // Clean up the token regardless of outcome.
    if let Some(id) = fetch_token {
        in_flight_map().lock().unwrap().remove(&id);
    }

    out
}

async fn do_request(
    conn: Connection,
    url: &str,
    method: &str,
    headers: &[(String, String)],
    req_body_reader: Option<BodyReader>,
    codec: std::sync::Arc<tokio::sync::Mutex<crate::qpack_bridge::QpackCodec>>,
) -> Result<FfiResponse, String> {
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| format!("open_bi: {e}"))?;

    // Derive path from URL.
    let path = extract_path(url);
    let has_body = req_body_reader.is_some();

    // Build header list for serialisation (convert owned pairs to borrowed refs).
    let pairs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    // Use chunked encoding only when the caller did not supply a Content-Length.
    // When Content-Length is present, send raw bytes so the framing matches.
    let has_content_len = pairs
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
    let req_chunked = has_body && !has_content_len;

    // Encode request head via QPACK.
    // When chunked encoding is needed, inject Transfer-Encoding: chunked
    // into the QPACK-encoded headers so the server decodes correctly.
    let mut qpack_pairs = pairs.clone();
    if req_chunked {
        qpack_pairs.push(("transfer-encoding", "chunked"));
    }
    let head_bytes = {
        let mut guard = codec.lock().await;
        guard.encode_request(method, &path, &qpack_pairs)
            .map_err(|e| format!("qpack encode: {e}"))?
    };

    send.write_all(&head_bytes)
        .await
        .map_err(|e| format!("write head: {e}"))?;

    // Spawn request body pump as a background task so we can concurrently
    // read the response head (avoids deadlock when the server sends an early
    // error response before the request body is fully consumed).
    if let Some(reader) = req_body_reader {
        tokio::spawn(async move {
            let _ = pump_body_to_stream(reader, &mut send, req_chunked, None).await;
            let _ = send.finish();
        });
    } else {
        send.finish().map_err(|e| format!("finish send: {e}"))?;
    }

    // Read and parse the response head.
    let (status, resp_headers, consumed) =
        read_head_qpack(&mut recv, &codec).await?;

    let resp_is_chunked = resp_headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")
    });

    // Spawn a task to pump the response body into a channel.
    let (res_writer, res_reader) = make_body_channel();
    // Set up a trailer channel — the pump task will send trailers when found.
    let (trailer_tx, trailer_rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
    let trailer_handle = crate::stream::insert_trailer_receiver(trailer_rx);

    tokio::spawn(pump_stream_to_body(recv, res_writer, consumed, trailer_tx, resp_is_chunked));

    let body_handle = insert_reader(res_reader);

    // Build response URL: set the URL to the remote peer's address.
    let remote_str = base32_encode(conn.remote_id().as_bytes());
    let response_url = format!("httpi://{remote_str}{path}");

    Ok(FfiResponse {
        status,
        headers: resp_headers,
        body_handle,
        url: response_url,
        trailers_handle: trailer_handle,
    })
}

// ── I/O helpers ──────────────────────────────────────────────────────────────

/// Write a `BodyReader`'s data to an Iroh `SendStream`.
///
/// If `chunked`, wraps each chunk in HTTP/1.1 chunked encoding.
/// If `trailer_rx` is `Some`, awaits trailers from JS after the body ends and
/// writes them before the stream-level finish.
/// If `trailer_rx` is `None`, writes the plain terminal chunk `0\r\n\r\n`.
pub(crate) async fn pump_body_to_stream(
    reader: BodyReader,
    send: &mut iroh::endpoint::SendStream,
    chunked: bool,
    trailer_rx: Option<tokio::sync::oneshot::Receiver<Vec<(String, String)>>>,
) -> Result<(), String> {
    loop {
        let chunk = reader.next_chunk().await;
        match chunk {
            None => break,
            Some(data) => {
                let wire = if chunked {
                    encode_chunk(&data)
                } else {
                    data.to_vec()
                };
                send.write_all(&wire)
                    .await
                    .map_err(|e| format!("write body chunk: {e}"))?;
            }
        }
    }
    if chunked {
        if let Some(rx) = trailer_rx {
            // Write the terminal chunk header without the empty-trailer terminator.
            send.write_all(terminal_chunk_start())
                .await
                .map_err(|e| format!("write terminal chunk: {e}"))?;
            // Await trailers from JS (or empty if JS dropped the sender).
            let trailers = rx.await.unwrap_or_default();
            let pairs: Vec<(&str, &str)> = trailers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            send.write_all(&serialize_trailers(&pairs))
                .await
                .map_err(|e| format!("write trailers: {e}"))?;
        } else {
            send.write_all(terminal_chunk())
                .await
                .map_err(|e| format!("write terminal chunk: {e}"))?;
        }
    }
    Ok(())
}

/// Read bytes from a `RecvStream` into a `BodyWriter` channel.
///
/// Handles chunked transfer-encoding decoding.  Closes the channel (signals EOF)
/// when the stream finishes.  After the terminal zero-chunk, reads any trailer
/// block and delivers it via `trailer_tx`.
async fn pump_stream_to_body(
    mut recv: iroh::endpoint::RecvStream,
    writer: BodyWriter,
    already_consumed: Vec<u8>,
    trailer_tx: tokio::sync::oneshot::Sender<Vec<(String, String)>>,
    is_chunked: bool,
) {
    let mut buf = already_consumed;
    let chunked_mode = is_chunked;

    loop {
        // Parse available data in the buffer BEFORE reading more from the
        // stream.  This is critical when `already_consumed` (leftover bytes
        // from response head parsing) already contains body data — without
        // this, the pump would block on `read_chunk` and miss the data.
        if chunked_mode {
            loop {
                match iroh_http_framing::parse_chunk_header(&buf) {
                    None => break, // need more bytes
                    Some((0, header_consumed)) => {
                        // Terminal chunk — read the trailer block that follows.
                        let after_header = buf[header_consumed..].to_vec();
                        let trailers = read_trailers_from_buf(&mut recv, after_header).await;
                        let _ = trailer_tx.send(trailers);
                        return; // EOF — writer drops, reader sees None.
                    }
                    Some((size, header_len)) => {
                        let data_end = header_len + size;
                        let trailer_end = data_end + 2; // skip \r\n after chunk
                        if buf.len() < trailer_end {
                            break; // need more bytes
                        }
                        let data = Bytes::copy_from_slice(&buf[header_len..data_end]);
                        if writer.send_chunk(data).await.is_err() {
                            return; // reader dropped
                        }
                        buf.drain(..trailer_end);
                    }
                }
            }
        } else {
            // Raw / non-chunked: forward whatever we have.
            if !buf.is_empty() {
                let data = Bytes::copy_from_slice(&buf);
                buf.clear();
                if writer.send_chunk(data).await.is_err() {
                    return;
                }
            }
        }

        // Read more data from the stream.
        match recv.read_chunk(READ_BUF).await {
            Err(_) | Ok(None) => break,
            Ok(Some(chunk)) => buf.extend_from_slice(&chunk.bytes),
        }
    }

    // Flush any remaining raw bytes (non-chunked only; chunked streams
    // terminate via the zero-chunk parsed above).
    if !buf.is_empty() && !chunked_mode {
        let data = Bytes::copy_from_slice(&buf);
        let _ = writer.send_chunk(data).await;
    }
    // writer drops here → channel closes → reader returns None.
}

/// Read a QPACK-encoded response head from a stream.
///
/// Wire format: `[2-byte big-endian length][QPACK block]`.
/// Returns `(status, headers, leftover_bytes)`.
async fn read_head_qpack(
    recv: &mut iroh::endpoint::RecvStream,
    codec: &std::sync::Arc<tokio::sync::Mutex<crate::qpack_bridge::QpackCodec>>,
) -> Result<(u16, Vec<(String, String)>, Vec<u8>), String> {
    let mut buf: Vec<u8> = Vec::new();

    loop {
        match recv
            .read_chunk(READ_BUF)
            .await
            .map_err(|e| format!("read: {e}"))?
        {
            None => return Err("stream closed before complete head".into()),
            Some(chunk) => buf.extend_from_slice(&chunk.bytes),
        }

        let mut guard = codec.lock().await;
        match guard.decode_response(&buf) {
            Ok((status, headers, consumed)) => {
                let leftover = buf[consumed..].to_vec();
                return Ok((status, headers, leftover));
            }
            Err(crate::qpack_bridge::DecodeError::Incomplete) => continue,
            Err(e) => return Err(format!("parse response head: {e}")),
        }
    }
}

fn extract_path(url: &str) -> String {
    // httpi://nodeId/path?query  →  /path?query
    if let Some(idx) = url.find("://") {
        let after_scheme = &url[idx + 3..];
        if let Some(slash) = after_scheme.find('/') {
            return after_scheme[slash..].to_string();
        }
        return "/".to_string();
    }
    // Already a path
    if url.starts_with('/') {
        url.to_string()
    } else {
        format!("/{url}")
    }
}

/// Read a complete trailer block from a stream, starting with `buf`.
///
/// Returns the parsed trailers, or an empty `Vec` on parse failure or EOF.
async fn read_trailers_from_buf(
    recv: &mut iroh::endpoint::RecvStream,
    mut buf: Vec<u8>,
) -> Vec<(String, String)> {
    loop {
        match parse_trailers(&buf) {
            Ok((trailers, _)) => return trailers,
            Err(FramingError::Incomplete) => {
                match recv.read_chunk(READ_BUF).await {
                    Ok(Some(chunk)) => buf.extend_from_slice(&chunk.bytes),
                    _ => return Vec::new(), // stream closed before trailers complete
                }
            }
            Err(_) => return Vec::new(),
        }
    }
}

// ── §2 Bidirectional streaming — raw_connect ──────────────────────────────────

/// Open a full-duplex QUIC connection to a remote node.
///
/// Sends an `Iroh-HTTP/1` request with `Upgrade: iroh-duplex` and awaits a
/// `101 Switching Protocols` response.  After the handshake, both the read
/// (`read_handle`) and write (`write_handle`) sides of the stream are exposed
/// as body-channel handles usable with `nextChunk`/`sendChunk`/`finishBody`.
pub async fn raw_connect(
    endpoint: &IrohEndpoint,
    remote_node_id: &str,
    path: &str,
    headers: &[(String, String)],
) -> Result<FfiDuplexStream, String> {
    let node_id = parse_node_id(remote_node_id)?;
    let addr = iroh::EndpointAddr::new(node_id);

    // Connect using the duplex ALPN — the peer must advertise it.
    let ep_raw = endpoint.raw().clone();
    let addr_clone = addr.clone();
    let pooled = endpoint
        .pool()
        .get_or_connect(node_id, ALPN_DUPLEX, || async move {
            ep_raw
                .connect(addr_clone, ALPN_DUPLEX)
                .await
                .map_err(|e| format!("connect duplex: {e}"))
        })
        .await?;

    let (mut send, mut recv) = pooled.conn
        .open_bi()
        .await
        .map_err(|e| format!("open_bi: {e}"))?;

    // Build the upgrade request header block.
    let mut all_headers: Vec<(&str, &str)> = vec![("Upgrade", "iroh-duplex")];
    let extra: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    all_headers.extend_from_slice(&extra);

    let head_bytes = {
        let mut guard = pooled.codec.lock().await;
        guard.encode_request("CONNECT", path, &all_headers)
            .map_err(|e| format!("qpack encode: {e}"))?
    };

    send.write_all(&head_bytes)
        .await
        .map_err(|e| format!("write connect head: {e}"))?;

    // Await the 101 Switching Protocols response.
    let (status, _headers, _leftover) = read_head_qpack(&mut recv, &pooled.codec).await?;
    if status != 101 {
        return Err(format!("server rejected duplex connection: expected 101, got {status}"));
    }

    // Receive side: pump data from server into a BodyReader channel.
    let (server_write, server_read) = make_body_channel();
    let read_handle = insert_reader(server_read);
    tokio::spawn(pump_duplex_recv(recv, server_write));

    // Send side: pump data from a BodyWriter channel to the server.
    let (client_write, client_read) = make_body_channel();
    let write_handle = insert_writer(client_write);
    tokio::spawn(pump_duplex_send(client_read, send));

    Ok(FfiDuplexStream {
        read_handle,
        write_handle,
    })
}

/// Pump raw bytes from a `RecvStream` into a `BodyWriter` (duplex receive side).
async fn pump_duplex_recv(mut recv: iroh::endpoint::RecvStream, writer: BodyWriter) {
    loop {
        match recv.read_chunk(READ_BUF).await {
            Ok(Some(chunk)) => {
                let bytes = bytes::Bytes::copy_from_slice(&chunk.bytes);
                if writer.send_chunk(bytes).await.is_err() {
                    break;
                }
            }
            _ => break,
        }
    }
    // writer drops → BodyReader sees EOF.
}

/// Pump raw bytes from a `BodyReader` into a `SendStream` (duplex send side).
async fn pump_duplex_send(reader: BodyReader, mut send: iroh::endpoint::SendStream) {
    loop {
        match reader.next_chunk().await {
            None => break,
            Some(data) => {
                if send.write_all(&data).await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = send.finish();
}
