//! Outgoing HTTP request — `fetch()` and `raw_connect()` implementation.
//!
//! HTTP/1.1 framing is delegated entirely to hyper.  Iroh's QUIC stream pair
//! is wrapped in `IrohStream` and handed to hyper's client connection API.

use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderName, HeaderValue, Method, StatusCode};
use http_body_util::{BodyExt, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper_util::rt::TokioIo;

use crate::{
    base32_encode, parse_node_addr,
    io::IrohStream,
    stream::{
        compose_handle, decompose_handle, insert_reader, insert_trailer_receiver,
        insert_writer, make_body_channel, BodyReader, BodyWriter,
    },
    CoreError, FfiDuplexStream, FfiResponse, IrohEndpoint, ALPN, ALPN_DUPLEX,
};

// ── BoxBody type alias ────────────────────────────────────────────────────────

type BoxBody = http_body_util::combinators::BoxBody<Bytes, std::convert::Infallible>;

fn box_body<B>(body: B) -> BoxBody
where
    B: http_body::Body<Data = Bytes, Error = std::convert::Infallible> + Send + Sync + 'static,
{
    body.map_err(|_| unreachable!()).boxed()
}

// ── In-flight fetch cancellation ──────────────────────────────────────────────

/// Allocate a cancellation token for an upcoming `fetch` call.
pub fn alloc_fetch_token() -> u32 {
    let slabs = crate::stream::global_slabs();
    let id = slabs
        .next_fetch_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let notify = Arc::new(tokio::sync::Notify::new());
    slabs
        .fetch_cancel
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, notify);
    compose_handle(0, id)
}

/// Signal an in-flight fetch to abort.
pub fn cancel_in_flight(token: u32) {
    let (ep_idx, id) = decompose_handle(token);
    if let Some(slabs) = crate::stream::get_slabs(ep_idx) {
        if let Some(notify) = slabs
            .fetch_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        {
            notify.notify_one();
        }
    }
}

// ── Public fetch API ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
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
    // Reject standard web schemes.
    {
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("https://") || lower.starts_with("http://") {
            let scheme_end = lower.find("://").map(|i| i + 3).unwrap_or(lower.len());
            return Err(format!(
                "iroh-http URLs must use the \"httpi://\" scheme, not \"{}\". \
                 Example: httpi://nodeId/path",
                &url[..scheme_end]
            ));
        }
    }

    // Validate method and headers at the FFI boundary.
    let http_method = Method::from_bytes(method.as_bytes()).map_err(|_| {
        CoreError::invalid_input(format!("invalid HTTP method {:?}", method)).to_string()
    })?;
    for (name, value) in headers {
        HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            CoreError::invalid_input(format!("invalid header name {:?}", name)).to_string()
        })?;
        HeaderValue::from_str(value).map_err(|_| {
            CoreError::invalid_input(format!("invalid header value for {:?}", name)).to_string()
        })?;
    }

    let cancel_notify = fetch_token.and_then(|token| {
        let (ep_idx, id) = decompose_handle(token);
        crate::stream::get_slabs(ep_idx).and_then(|s| {
            s.fetch_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&id)
                .cloned()
        })
    });
    let ep_idx = endpoint.inner.endpoint_idx;

    let parsed = parse_node_addr(remote_node_id)?;
    let node_id = parsed.node_id;
    let mut addr = iroh::EndpointAddr::new(node_id);
    for a in &parsed.direct_addrs {
        addr = addr.with_ip_addr(*a);
    }
    if let Some(addrs) = direct_addrs {
        for a in addrs {
            addr = addr.with_ip_addr(*a);
        }
    }

    let ep_raw = endpoint.raw().clone();
    let addr_clone = addr.clone();
    let max_header_size = endpoint.max_header_size();

    let pooled = endpoint
        .pool()
        .get_or_connect(node_id, ALPN, || async move {
            ep_raw.connect(addr_clone, ALPN).await.map_err(|e| format!("connect: {e}"))
        })
        .await?;

    let conn = pooled.conn.clone();

    let result = do_fetch(
        ep_idx,
        conn,
        url,
        http_method,
        headers,
        req_body_reader,
        max_header_size,
    );

    let out = if let Some(notify) = cancel_notify {
        tokio::select! {
            _ = notify.notified() => Err("aborted".to_string()),
            r = result => r,
        }
    } else {
        result.await
    };

    // Clean up the cancellation token.
    if let Some(token) = fetch_token {
        let (ep_idx_t, id) = decompose_handle(token);
        if let Some(slabs) = crate::stream::get_slabs(ep_idx_t) {
            slabs.fetch_cancel.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        }
    }

    out
}

async fn do_fetch(
    ep_idx: u32,
    conn: iroh::endpoint::Connection,
    url: &str,
    method: Method,
    headers: &[(String, String)],
    req_body_reader: Option<BodyReader>,
    max_header_size: usize,
) -> Result<FfiResponse, String> {
    let (send, recv) = conn.open_bi().await.map_err(|e| format!("open_bi: {e}"))?;

    let io = TokioIo::new(IrohStream::new(send, recv));

    let (mut sender, conn_task) = hyper::client::conn::http1::Builder::new()
        .max_buf_size(max_header_size)
        .max_headers(128)
        .handshake::<_, BoxBody>(io)
        .await
        .map_err(|e| format!("hyper handshake: {e}"))?;

    // Drive the connection state machine in the background.
    tokio::spawn(conn_task);

    let path = extract_path(url);
    let remote_str = base32_encode(conn.remote_id().as_bytes());

    // Build the hyper request.
    let mut req_builder = hyper::Request::builder()
        .method(method)
        .uri(&path)
        .header(hyper::header::HOST, &remote_str)
        // Tell the server we accept chunked trailers (required for HTTP/1.1 trailer delivery).
        .header("te", "trailers");

    for (k, v) in headers {
        req_builder = req_builder.header(k.as_str(), v.as_str());
    }

    let req_body: BoxBody = if let Some(reader) = req_body_reader {
        // Adapt BodyReader → hyper body (no trailers on request side for now).
        box_body(body_from_reader(reader, None))
    } else {
        box_body(http_body_util::Empty::new())
    };

    let req = req_builder.body(req_body).map_err(|e| format!("build request: {e}"))?;

    let resp = sender.send_request(req).await.map_err(|e| format!("send_request: {e}"))?;

    let status = resp.status().as_u16();
    let resp_headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    // Allocate channels for streaming the response body to JS.
    let (trailer_tx, trailer_rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
    let trailer_handle = insert_trailer_receiver(ep_idx, trailer_rx);

    let (res_writer, res_reader) = make_body_channel();
    let body = resp.into_body();
    tokio::spawn(pump_hyper_body_to_channel(body, res_writer, trailer_tx));

    let body_handle = insert_reader(ep_idx, res_reader);
    let response_url = format!("httpi://{remote_str}{path}");

    Ok(FfiResponse {
        status,
        headers: resp_headers,
        body_handle,
        url: response_url,
        trailers_handle: trailer_handle,
    })
}

// ── Body bridge utilities ─────────────────────────────────────────────────────

/// Drain a hyper `Incoming` body into `BodyWriter`, delivering trailers via
/// the oneshot when the body ends.
pub(crate) async fn pump_hyper_body_to_channel(
    body: Incoming,
    writer: BodyWriter,
    trailer_tx: tokio::sync::oneshot::Sender<Vec<(String, String)>>,
) {
    pump_hyper_body_to_channel_limited(body, writer, trailer_tx, None).await;
}

/// Drain with optional byte limit.
pub(crate) async fn pump_hyper_body_to_channel_limited(
    mut body: Incoming,
    writer: BodyWriter,
    trailer_tx: tokio::sync::oneshot::Sender<Vec<(String, String)>>,
    max_bytes: Option<usize>,
) {
    let mut total = 0usize;
    let mut trailers_vec: Vec<(String, String)> = Vec::new();

    while let Some(frame_result) = body.frame().await {
        match frame_result {
            Err(e) => {
                tracing::warn!("iroh-http: body frame error: {e}");
                break;
            }
            Ok(frame) => {
                if frame.is_data() {
                    let data = frame.into_data().expect("is_data checked above");
                    total += data.len();
                    if let Some(limit) = max_bytes {
                        if total > limit {
                            tracing::warn!("iroh-http: request body exceeded {limit} bytes");
                            break;
                        }
                    }
                    if writer.send_chunk(data).await.is_err() {
                        return; // reader dropped
                    }
                } else if frame.is_trailers() {
                    let hdrs = frame.into_trailers().expect("is_trailers checked above");
                    trailers_vec = hdrs
                        .iter()
                        .map(|(k, v)| {
                            (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())
                        })
                        .collect();
                }
            }
        }
    }

    drop(writer);
    let _ = trailer_tx.send(trailers_vec);
}

/// Adapt a `BodyReader` + optional trailer channel into a hyper-compatible
/// body using `StreamBody` backed by a futures stream.
pub(crate) fn body_from_reader(
    reader: BodyReader,
    trailer_rx: Option<tokio::sync::oneshot::Receiver<Vec<(String, String)>>>,
) -> StreamBody<impl futures::Stream<Item = Result<Frame<Bytes>, std::convert::Infallible>>> {
    use futures::stream;

    // State machine: first yield data frames, then optionally a trailer frame.
    let s = stream::unfold(
        (reader, trailer_rx, false),
        |(reader, trailer_rx, done)| async move {
            if done {
                return None;
            }
            match reader.next_chunk().await {
                Some(data) => Some((Ok(Frame::data(data)), (reader, trailer_rx, false))),
                None => {
                    // Body data complete — check for trailers.
                    if let Some(rx) = trailer_rx {
                        if let Ok(trailers) = rx.await {
                            let mut map = http::HeaderMap::new();
                            for (k, v) in trailers {
                                if let (Ok(name), Ok(val)) = (
                                    HeaderName::from_bytes(k.as_bytes()),
                                    HeaderValue::from_str(&v),
                                ) {
                                    map.append(name, val);
                                }
                            }
                            if !map.is_empty() {
                                return Some((
                                    Ok(Frame::trailers(map)),
                                    (reader, None, true),
                                ));
                            }
                        }
                    }
                    None
                }
            }
        },
    );

    StreamBody::new(s)
}

// ── Path extraction ───────────────────────────────────────────────────────────

pub(crate) fn extract_path(url: &str) -> String {
    if let Some(idx) = url.find("://") {
        let after_scheme = &url[idx + 3..];
        if let Some(slash) = after_scheme.find('/') {
            return after_scheme[slash..].to_string();
        }
        return "/".to_string();
    }
    if url.starts_with('/') {
        url.to_string()
    } else {
        format!("/{url}")
    }
}

// ── Duplex / raw_connect ──────────────────────────────────────────────────────

/// Open a full-duplex QUIC connection to a remote node via HTTP Upgrade.
pub async fn raw_connect(
    endpoint: &IrohEndpoint,
    remote_node_id: &str,
    path: &str,
    headers: &[(String, String)],
) -> Result<FfiDuplexStream, String> {
    // Validate headers.
    for (name, value) in headers {
        HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            CoreError::invalid_input(format!("invalid header name {:?}", name)).to_string()
        })?;
        HeaderValue::from_str(value).map_err(|_| {
            CoreError::invalid_input(format!("invalid header value for {:?}", name)).to_string()
        })?;
    }

    let parsed = parse_node_addr(remote_node_id)?;
    let node_id = parsed.node_id;
    let mut addr = iroh::EndpointAddr::new(node_id);
    for a in &parsed.direct_addrs {
        addr = addr.with_ip_addr(*a);
    }

    let ep_raw = endpoint.raw().clone();
    let addr_clone = addr.clone();
    let max_header_size = endpoint.max_header_size();

    let pooled = endpoint
        .pool()
        .get_or_connect(node_id, ALPN_DUPLEX, || async move {
            ep_raw.connect(addr_clone, ALPN_DUPLEX).await.map_err(|e| format!("connect duplex: {e}"))
        })
        .await?;

    let (send, recv) = pooled.conn.open_bi().await.map_err(|e| format!("open_bi: {e}"))?;
    let io = TokioIo::new(IrohStream::new(send, recv));

    let (mut sender, conn_task) = hyper::client::conn::http1::Builder::new()
        .max_buf_size(max_header_size)
        .handshake::<_, BoxBody>(io)
        .await
        .map_err(|e| format!("hyper handshake (duplex): {e}"))?;

    tokio::spawn(conn_task);

    // Build CONNECT request with Upgrade: iroh-duplex.
    let mut req_builder = hyper::Request::builder()
        .method(Method::from_bytes(b"CONNECT").unwrap())
        .uri(path)
        .header(hyper::header::UPGRADE, "iroh-duplex");

    for (k, v) in headers {
        req_builder = req_builder.header(k.as_str(), v.as_str());
    }

    let req = req_builder
        .body(box_body(http_body_util::Empty::new()))
        .map_err(|e| format!("build duplex request: {e}"))?;

    let resp = sender.send_request(req).await.map_err(|e| format!("send duplex request: {e}"))?;

    let status = resp.status();
    if status != StatusCode::SWITCHING_PROTOCOLS {
        return Err(format!("server rejected duplex: expected 101, got {status}"));
    }

    // Perform the protocol upgrade to get raw bidirectional IO.
    let upgraded = hyper::upgrade::on(resp)
        .await
        .map_err(|e| format!("upgrade error: {e}"))?;

    let ep_idx = endpoint.inner.endpoint_idx;
    let (server_write, server_read) = make_body_channel();
    let (client_write, client_read) = make_body_channel();

    let read_handle = insert_reader(ep_idx, server_read);
    let write_handle = insert_writer(ep_idx, client_write);

    // Pipe upgraded IO to/from body channels.
    tokio::spawn(pump_upgraded(upgraded, server_write, client_read));

    Ok(FfiDuplexStream { read_handle, write_handle })
}

/// Pump data between an upgraded hyper IO object and body channels.
async fn pump_upgraded(
    upgraded: hyper::upgrade::Upgraded,
    writer: BodyWriter,   // server→client: write incoming data here
    reader: BodyReader,   // client→server: read outgoing data from here
) {
    let io = TokioIo::new(upgraded);
    let (mut recv, mut send) = tokio::io::split(io);

    tokio::join!(
        async {
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                use tokio::io::AsyncReadExt;
                match recv.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if writer.send_chunk(bytes::Bytes::copy_from_slice(&buf[..n])).await.is_err() {
                            break;
                        }
                    }
                }
            }
        },
        async {
            use tokio::io::AsyncWriteExt;
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
            let _ = send.shutdown().await;
        },
    );
}
