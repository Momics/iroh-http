//! Outgoing HTTP request — `fetch()` implementation.

use bytes::Bytes;
use iroh::endpoint::Connection;


use crate::{
    base32_encode, parse_node_id, stream::{BodyReader, BodyWriter, make_body_channel, insert_reader},
    FfiResponse, IrohEndpoint, ALPN,
};
use iroh_http_framing::{
    parse_response_head, serialize_request_head, encode_chunk, terminal_chunk,
    FramingError,
};

const READ_BUF: usize = 16 * 1024;

/// Send an HTTP/1.1 request to a remote node and return the response.
///
/// `req_body_reader` — optional body channel that the caller will pump
/// from the JS side via `sendChunk`/`finishBody`.  `None` for bodyless methods.
pub async fn fetch(
    endpoint: &IrohEndpoint,
    remote_node_id: &str,
    url: &str,
    method: &str,
    headers: &[(String, String)],
    req_body_reader: Option<BodyReader>,
) -> Result<FfiResponse, String> {
    let node_id = parse_node_id(remote_node_id)?;
    let addr = iroh::EndpointAddr::new(node_id);

    let conn = endpoint
        .raw()
        .connect(addr, ALPN)
        .await
        .map_err(|e| format!("connect: {e}"))?;

    do_request(conn, url, method, headers, req_body_reader).await
}

async fn do_request(
    conn: Connection,
    url: &str,
    method: &str,
    headers: &[(String, String)],
    req_body_reader: Option<BodyReader>,
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
    let head_bytes = serialize_request_head(method, &path, &pairs, has_body);

    send.write_all(&head_bytes)
        .await
        .map_err(|e| format!("write head: {e}"))?;

    // Pump request body (chunked) in a separate task so we can concurrently
    // read the response head.
    if let Some(reader) = req_body_reader {
        pump_body_to_stream(reader, &mut send, true).await?;
    }

    send.finish().map_err(|e| format!("finish send: {e}"))?;

    // Read and parse the response head.
    let (status, _reason, resp_headers, consumed) = read_head(&mut recv).await?;

    // Spawn a task to pump the response body into a channel.
    let (res_writer, res_reader) = make_body_channel();
    tokio::spawn(pump_stream_to_body(recv, res_writer, consumed));

    let body_handle = insert_reader(res_reader);

    // Build response URL: set the URL to the remote peer's address.
    let remote_str = base32_encode(conn.remote_id().as_bytes());
    let _response_url = format!("http+iroh://{remote_str}{path}");

    Ok(FfiResponse {
        status,
        headers: resp_headers,
        body_handle,
    })
}

// ── I/O helpers ──────────────────────────────────────────────────────────────

/// Write a `BodyReader`'s data to an Iroh `SendStream`.
///
/// If `chunked`, wraps each chunk in HTTP/1.1 chunked encoding.
/// After all data is written, calls `.finish()` on the sender.
pub(crate) async fn pump_body_to_stream(
    reader: BodyReader,
    send: &mut iroh::endpoint::SendStream,
    chunked: bool,
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
        send.write_all(terminal_chunk())
            .await
            .map_err(|e| format!("write terminal chunk: {e}"))?;
    }
    Ok(())
}

/// Read bytes from a `RecvStream` into a `BodyWriter` channel.
///
/// Handles chunked transfer-encoding decoding.  Closes the channel (signals EOF)
/// when the stream finishes.
async fn pump_stream_to_body(
    mut recv: iroh::endpoint::RecvStream,
    writer: BodyWriter,
    already_consumed: Vec<u8>,
) {
    let mut buf = already_consumed;

    // Determine if chunked by inspecting what we read during head parsing.
    // The actual `Transfer-Encoding: chunked` check is done inside the response-
    // head parse.  For simplicity we always use chunked framing on sends and
    // decode it here.  If no chunk header is found we treat bytes as raw body.
    let mut chunked_mode = false; // set below when we have enough data

    loop {
        // Try to get more data when needed.
        match recv
            .read_chunk(READ_BUF)
            .await
        {
            Err(_) | Ok(None) => break,
            Ok(Some(chunk)) => buf.extend_from_slice(&chunk.bytes),
        }

        // We only determine chunked mode once per stream.
        if !chunked_mode && buf.starts_with(b"0\r\n") {
            // Empty chunked body.
            break;
        }
        // If the first byte(s) look like hex + \r\n we assume chunked.
        chunked_mode = looks_like_chunk_header(&buf);

        if chunked_mode {
            loop {
                match iroh_http_framing::parse_chunk_header(&buf) {
                    None => break, // need more bytes
                    Some((0, _)) => {
                        // Terminal chunk.
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
    }

    // Flush any remaining raw bytes.
    if !buf.is_empty() && !chunked_mode {
        let data = Bytes::copy_from_slice(&buf);
        let _ = writer.send_chunk(data).await;
    }
    // writer drops here → channel closes → reader returns None.
}

fn looks_like_chunk_header(buf: &[u8]) -> bool {
    for &b in buf.iter().take(10) {
        if b == b'\r' {
            return true;
        }
        if !(b.is_ascii_hexdigit()) {
            return false;
        }
    }
    false
}

/// Accumulate bytes from `recv` until a full HTTP/1.1 head is found
/// (i.e. `\r\n\r\n`), then parse it.
/// Returns (status, reason, headers, leftover_bytes_after_head).
async fn read_head(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<(u16, String, Vec<(String, String)>, Vec<u8>), String> {
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

        match parse_response_head(&buf) {            Ok((status, reason, headers, consumed)) => {
                let leftover = buf[consumed..].to_vec();
                return Ok((status, reason, headers, leftover));
            }
            Err(FramingError::Incomplete) => continue,
            Err(FramingError::Parse(e)) => return Err(format!("parse response head: {e}")),
        }
    }
}

fn extract_path(url: &str) -> String {
    // http+iroh://nodeId/path?query  →  /path?query
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
