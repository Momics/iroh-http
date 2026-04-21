//! Outgoing HTTP request — `fetch()` and `raw_connect()` implementation.
//!
//! HTTP/1.1 framing is delegated entirely to hyper.  Iroh's QUIC stream pair
//! is wrapped in `IrohStream` and handed to hyper's client connection API.

use bytes::Bytes;
use http::{HeaderName, HeaderValue, Method, StatusCode};
use http_body_util::{BodyExt, StreamBody};
use hyper::body::Frame;
use hyper_util::rt::TokioIo;

use crate::{
    io::IrohStream,
    parse_node_addr,
    stream::{BodyReader, BodyWriter, HandleStore},
    CoreError, FfiDuplexStream, FfiResponse, IrohEndpoint, ALPN, ALPN_DUPLEX,
};

// ── BoxBody type alias ────────────────────────────────────────────────────────

use crate::BoxBody;

// ── Compression: thin tower service wrapper around hyper SendRequest ─────────

/// Wraps `SendRequest<BoxBody>` as a `tower::Service` so compression/decompression
/// layers from `tower-http` can be composed around it.
#[cfg(feature = "compression")]
struct HyperClientSvc(hyper::client::conn::http1::SendRequest<BoxBody>);

#[cfg(feature = "compression")]
impl tower::Service<hyper::Request<BoxBody>> for HyperClientSvc {
    type Response = hyper::Response<hyper::body::Incoming>;
    type Error = hyper::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: hyper::Request<BoxBody>) -> Self::Future {
        Box::pin(self.0.send_request(req))
    }
}

// ── In-flight fetch cancellation ──────────────────────────────────────────────

// alloc_fetch_token / cancel_in_flight / get_fetch_cancel_notify / remove_fetch_token
// are now in crate::stream (imported above).
// ── Public fetch API ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn fetch(
    endpoint: &IrohEndpoint,
    remote_node_id: &str,
    url: &str,
    method: &str,
    headers: &[(String, String)],
    req_body_reader: Option<BodyReader>,
    fetch_token: Option<u64>,
    direct_addrs: Option<&[std::net::SocketAddr]>,
) -> Result<FfiResponse, CoreError> {
    // Reject standard web schemes.
    {
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("https://") || lower.starts_with("http://") {
            let scheme_end = lower
                .find("://")
                .map(|i| i.saturating_add(3))
                .unwrap_or(lower.len());
            return Err(CoreError::invalid_input(format!(
                "iroh-http URLs must use the \"httpi://\" scheme, not \"{}\". \
                 Example: httpi://nodeId/path",
                &url[..scheme_end]
            )));
        }
    }

    // Validate method and headers at the FFI boundary.
    let http_method = Method::from_bytes(method.as_bytes())
        .map_err(|_| CoreError::invalid_input(format!("invalid HTTP method {:?}", method)))?;
    for (name, value) in headers {
        HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| CoreError::invalid_input(format!("invalid header name {:?}", name)))?;
        HeaderValue::from_str(value).map_err(|_| {
            CoreError::invalid_input(format!("invalid header value for {:?}", name))
        })?;
    }

    let cancel_notify = fetch_token.and_then(|t| endpoint.handles().get_fetch_cancel_notify(t));
    let handles = endpoint.handles();

    // Wrap all fallible work so the cancel-token cleanup below always runs,
    // even if connection setup returns early via `?`.
    let out = async {
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
                ep_raw
                    .connect(addr_clone, ALPN)
                    .await
                    .map_err(|e| format!("connect: {e}"))
            })
            .await
            .map_err(CoreError::connection_failed)?;

        let conn = pooled.conn.clone();
        let remote_str = pooled.remote_id_str.clone();

        let result = do_fetch(
            handles,
            conn,
            &remote_str,
            url,
            http_method,
            headers,
            req_body_reader,
            max_header_size,
        );

        if let Some(notify) = cancel_notify {
            tokio::select! {
                _ = notify.notified() => Err(CoreError::cancelled()),
                r = result => r,
            }
        } else {
            result.await
        }
    }
    .await;

    // Clean up the cancellation token — always, even on early error.
    if let Some(token) = fetch_token {
        endpoint.handles().remove_fetch_token(token);
    }

    out
}

/// Classify a hyper send-request or handshake error: if the error message
/// indicates a header/buffer overflow (hyper emits "header" in its parse error
/// descriptions), return `CoreError::HeaderTooLarge`; otherwise return
/// `CoreError::ConnectionFailed`.  This gives callers consistent error types
/// regardless of where hyper's internal buffer boundary falls relative to the
/// configured `max_header_size`.
fn classify_hyper_error(e: &impl std::fmt::Display, context: &str) -> CoreError {
    let msg = e.to_string();
    // hyper 1.x surfaces header-parse failures as e.g.
    //   "error reading a body from connection: header too large"
    //   "invalid HTTP method"
    //   "header value is too long"
    //   "too many headers"
    // All of these mention "header" in the message.
    if msg.to_ascii_lowercase().contains("header") {
        CoreError::header_too_large(format!("{context}: {msg}"))
    } else {
        CoreError::connection_failed(format!("{context}: {msg}"))
    }
}

#[allow(clippy::too_many_arguments)]
async fn do_fetch(
    handles: &HandleStore,
    conn: iroh::endpoint::Connection,
    remote_str: &str,
    url: &str,
    method: Method,
    headers: &[(String, String)],
    req_body_reader: Option<BodyReader>,
    max_header_size: usize,
) -> Result<FfiResponse, CoreError> {
    let (send, recv) = conn
        .open_bi()
        .await
        .map_err(|e| CoreError::connection_failed(format!("open_bi: {e}")))?;

    let io = TokioIo::new(IrohStream::new(send, recv));

    #[allow(unused_mut)] // mut only needed without the compression feature
    let (mut sender, conn_task) = hyper::client::conn::http1::Builder::new()
        // hyper requires max_buf_size >= 8192; clamp upward so small
        // max_header_size values don't panic.  Header-size enforcement happens
        // via the response parsing error that hyper returns when the actual
        // response head exceeds max_header_size bytes.
        .max_buf_size(max_header_size.max(8192))
        .max_headers(128)
        .handshake::<_, BoxBody>(io)
        .await
        .map_err(|e| CoreError::connection_failed(format!("hyper handshake: {e}")))?;

    // Drive the connection state machine in the background.
    tokio::spawn(conn_task);

    let path = extract_path(url);

    // Build the hyper request.
    let mut req_builder = hyper::Request::builder()
        .method(method)
        .uri(&path)
        .header(hyper::header::HOST, remote_str);

    // When compression is enabled, advertise zstd-only Accept-Encoding — but
    // only if the caller has not already set Accept-Encoding.  A caller passing
    // `Accept-Encoding: identity` is opting out of compression and must not be
    // overridden.
    #[cfg(feature = "compression")]
    {
        let has_accept_encoding = headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("accept-encoding"));
        if !has_accept_encoding {
            req_builder = req_builder.header("accept-encoding", "zstd");
        }
    }

    for (k, v) in headers {
        req_builder = req_builder.header(k.as_str(), v.as_str());
    }

    let req_body: BoxBody = if let Some(reader) = req_body_reader {
        crate::box_body(body_from_reader(reader))
    } else {
        crate::box_body(http_body_util::Empty::new())
    };

    let req = req_builder
        .body(req_body)
        .map_err(|e| CoreError::internal(format!("build request: {e}")))?;

    // Dispatch: with compression, wrap sender in DecompressionLayer so the
    // response body is transparently decompressed before reaching the channel pump.
    #[cfg(feature = "compression")]
    let resp = {
        use tower::ServiceExt;
        let svc = tower::ServiceBuilder::new()
            .layer(tower_http::decompression::DecompressionLayer::new())
            .service(HyperClientSvc(sender));
        svc.oneshot(req)
            .await
            .map_err(|e| classify_hyper_error(&e, "send_request"))?
    };
    #[cfg(not(feature = "compression"))]
    let resp = sender
        .send_request(req)
        .await
        .map_err(|e| classify_hyper_error(&e, "send_request"))?;

    let status = resp.status().as_u16();
    // ISS-011: measure header bytes using raw values before string conversion;
    // reject non-UTF8 response header values deterministically.
    let header_bytes: usize = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            k.as_str()
                .len()
                .saturating_add(v.as_bytes().len())
                .saturating_add(4) // "name: value\r\n"
        })
        .fold(16usize, |acc, x| acc.saturating_add(x)); // approximate status line
    if header_bytes > max_header_size {
        return Err(CoreError::header_too_large(format!(
            "response header size {header_bytes} exceeds limit {max_header_size}"
        )));
    }

    let mut resp_headers: Vec<(String, String)> = Vec::new();
    for (k, v) in resp.headers().iter() {
        match v.to_str() {
            Ok(s) => resp_headers.push((k.as_str().to_string(), s.to_string())),
            Err(_) => {
                return Err(CoreError::invalid_input(format!(
                    "non-UTF8 response header value for '{}'",
                    k.as_str()
                )));
            }
        }
    }

    let response_url = format!("httpi://{remote_str}{path}");

    // RFC 9110 §6.3: responses with status 204, 205, or 304 MUST NOT carry a
    // message body.  Skip channel allocation entirely and return the slotmap
    // null sentinel (0) for body_handle so the JS layer can use
    // `bodyHandle === 0n` as a clean structural check without re-encoding
    // HTTP semantics in every adapter.
    if is_null_body_status(status) {
        // Dropping the body signals to hyper that we are done reading.
        // For a spec-compliant server the body is already empty; this is a
        // defensive drain for misbehaving peers.
        drop(resp.into_body());
        return Ok(FfiResponse {
            status,
            headers: resp_headers,
            body_handle: 0,
            url: response_url,
        });
    }

    // Allocate channels for streaming the response body to JS.
    let mut guard = handles.insert_guard();

    let (res_writer, res_reader) = handles.make_body_channel();
    let body = resp.into_body();
    tokio::spawn(pump_hyper_body_to_channel(body, res_writer));

    let body_handle = guard.insert_reader(res_reader)?;

    guard.commit();
    Ok(FfiResponse {
        status,
        headers: resp_headers,
        body_handle,
        url: response_url,
    })
}

/// RFC 9110 §6.3 — responses with these status codes MUST NOT contain a
/// message body.  Skipping body-channel allocation for them avoids wasting
/// resources and keeps HTTP semantics in core instead of every adapter.
#[inline]
fn is_null_body_status(status: u16) -> bool {
    status == 204 || status == 205 || status == 304
}

// ── Body bridge utilities ─────────────────────────────────────────────────────

/// Drain a hyper body into `BodyWriter`.
/// Generic over any body type with `Data = Bytes` (e.g. `Incoming`, `DecompressionBody`).
pub(crate) async fn pump_hyper_body_to_channel<B>(body: B, writer: BodyWriter)
where
    B: http_body::Body<Data = Bytes>,
    B::Error: std::fmt::Debug,
{
    let timeout = writer.drain_timeout;
    pump_hyper_body_to_channel_limited(body, writer, None, timeout, None).await;
}

/// Drain with optional byte limit and a per-frame read timeout.
///
/// `frame_timeout` bounds how long we wait for each individual body frame.
/// A slow-drip peer that stalls indefinitely will be cut off after this deadline.
///
/// When a byte limit is set and the body exceeds it, `overflow_tx` is fired
/// so the caller can return a `413 Content Too Large` response (ISS-004).
pub(crate) async fn pump_hyper_body_to_channel_limited<B>(
    body: B,
    writer: BodyWriter,
    max_bytes: Option<usize>,
    frame_timeout: std::time::Duration,
    overflow_tx: Option<tokio::sync::oneshot::Sender<()>>,
) where
    B: http_body::Body<Data = Bytes>,
    B::Error: std::fmt::Debug,
{
    // Box::pin gives Pin<Box<B>>: Unpin (Box<T>: Unpin ∀T), which satisfies BodyExt::frame().
    let mut body = Box::pin(body);
    let mut total = 0usize;

    loop {
        let frame_result = match tokio::time::timeout(frame_timeout, body.frame()).await {
            Err(_elapsed) => {
                tracing::warn!("iroh-http: body frame read timed out after {frame_timeout:?}");
                break;
            }
            Ok(None) => break,
            Ok(Some(r)) => r,
        };
        match frame_result {
            Err(e) => {
                tracing::warn!("iroh-http: body frame error: {e:?}");
                break;
            }
            Ok(frame) => {
                if frame.is_data() {
                    let data = frame.into_data().expect("is_data checked above");
                    total = total.saturating_add(data.len());
                    if let Some(limit) = max_bytes {
                        if total > limit {
                            tracing::warn!("iroh-http: request body exceeded {limit} bytes");
                            // ISS-004: signal overflow so the serve path can send 413.
                            if let Some(tx) = overflow_tx {
                                let _ = tx.send(());
                            }
                            break;
                        }
                    }
                    if writer.send_chunk(data).await.is_err() {
                        return; // reader dropped
                    }
                }
            }
        }
    }

    drop(writer);
}

/// Adapt a `BodyReader` into a hyper-compatible body using `StreamBody`
/// backed by a futures stream.
pub(crate) fn body_from_reader(
    reader: BodyReader,
) -> StreamBody<impl futures::Stream<Item = Result<Frame<Bytes>, std::convert::Infallible>>> {
    use futures::stream;

    let s = stream::unfold(reader, |reader| async move {
        reader
            .next_chunk()
            .await
            .map(|data| (Ok(Frame::data(data)), reader))
    });

    StreamBody::new(s)
}

// ── Path extraction ───────────────────────────────────────────────────────────

pub(crate) fn extract_path(url: &str) -> String {
    let raw = if let Some(idx) = url.find("://") {
        let after_scheme = url.get(idx.saturating_add(3)..).unwrap_or("");
        if let Some(slash) = after_scheme.find('/') {
            after_scheme[slash..].to_string()
        } else if let Some(q) = after_scheme.find('?') {
            // No path segment — check for query string (e.g. "httpi://node?x=1").
            format!("/{}", &after_scheme[q..])
        } else {
            "/".to_string()
        }
    } else if url.starts_with('/') {
        url.to_string()
    } else {
        format!("/{url}")
    };

    // RFC 9110 §4.1: fragment identifiers are client-side only and must
    // never appear in the request-target sent on the wire.
    match raw.find('#') {
        Some(pos) => raw[..pos].to_string(),
        None => raw,
    }
}

// ── Duplex / raw_connect ──────────────────────────────────────────────────────

/// Open a full-duplex QUIC connection to a remote node via HTTP Upgrade.
pub async fn raw_connect(
    endpoint: &IrohEndpoint,
    remote_node_id: &str,
    path: &str,
    headers: &[(String, String)],
) -> Result<FfiDuplexStream, CoreError> {
    // Validate headers.
    for (name, value) in headers {
        HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| CoreError::invalid_input(format!("invalid header name {:?}", name)))?;
        HeaderValue::from_str(value).map_err(|_| {
            CoreError::invalid_input(format!("invalid header value for {:?}", name))
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
    let handles = endpoint.handles();

    let pooled = endpoint
        .pool()
        .get_or_connect(node_id, ALPN_DUPLEX, || async move {
            ep_raw
                .connect(addr_clone, ALPN_DUPLEX)
                .await
                .map_err(|e| format!("connect duplex: {e}"))
        })
        .await
        .map_err(CoreError::connection_failed)?;

    let (send, recv) = pooled
        .conn
        .open_bi()
        .await
        .map_err(|e| CoreError::connection_failed(format!("open_bi: {e}")))?;
    let io = TokioIo::new(IrohStream::new(send, recv));

    let (mut sender, conn_task) = hyper::client::conn::http1::Builder::new()
        .max_buf_size(max_header_size.max(8192))
        .handshake::<_, BoxBody>(io)
        .await
        .map_err(|e| CoreError::connection_failed(format!("hyper handshake (duplex): {e}")))?;

    tokio::spawn(conn_task);

    // Build CONNECT request with Upgrade: iroh-duplex.
    // ISS-015: include Connection: upgrade for strict handshake compliance.
    let mut req_builder = hyper::Request::builder()
        .method(Method::from_bytes(b"CONNECT").expect("CONNECT is a valid HTTP method"))
        .uri(path)
        .header(hyper::header::CONNECTION, "upgrade")
        .header(hyper::header::UPGRADE, "iroh-duplex");

    for (k, v) in headers {
        req_builder = req_builder.header(k.as_str(), v.as_str());
    }

    let req = req_builder
        .body(crate::box_body(http_body_util::Empty::new()))
        .map_err(|e| CoreError::internal(format!("build duplex request: {e}")))?;

    let resp = sender
        .send_request(req)
        .await
        .map_err(|e| CoreError::connection_failed(format!("send duplex request: {e}")))?;

    let status = resp.status();
    if status != StatusCode::SWITCHING_PROTOCOLS {
        // ISS-022: use PeerRejected so callers can distinguish policy rejection
        // from transport failure for retry/telemetry purposes.
        return Err(CoreError::peer_rejected(format!(
            "server rejected duplex: expected 101, got {status}"
        )));
    }

    // Perform the protocol upgrade to get raw bidirectional IO.
    let upgraded = hyper::upgrade::on(resp)
        .await
        .map_err(|e| CoreError::connection_failed(format!("upgrade error: {e}")))?;

    let (server_write, server_read) = handles.make_body_channel();
    let (client_write, client_read) = handles.make_body_channel();

    let read_handle = handles.insert_reader(server_read)?;
    let write_handle = handles.insert_writer(client_write)?;

    // Pipe upgraded IO to/from body channels.
    let io = TokioIo::new(upgraded);
    tokio::spawn(crate::stream::pump_duplex(io, server_write, client_read));

    Ok(FfiDuplexStream {
        read_handle,
        write_handle,
    })
}

#[cfg(test)]
mod tests {
    use super::extract_path;

    #[test]
    fn extract_path_basic() {
        assert_eq!(extract_path("httpi://node/foo/bar"), "/foo/bar");
        assert_eq!(extract_path("httpi://node/"), "/");
        assert_eq!(extract_path("httpi://node"), "/");
    }

    #[test]
    fn extract_path_query_string() {
        assert_eq!(extract_path("httpi://node/path?x=1"), "/path?x=1");
        assert_eq!(extract_path("httpi://node?x=1"), "/?x=1");
    }

    #[test]
    fn extract_path_fragment() {
        // RFC 9110 §4.1: fragments must be stripped before sending.
        assert_eq!(extract_path("httpi://node/path#frag"), "/path");
        assert_eq!(extract_path("httpi://node/path?q=1#frag"), "/path?q=1");
        assert_eq!(extract_path("/local#frag"), "/local");
    }

    #[test]
    fn extract_path_bare_path() {
        assert_eq!(extract_path("/already"), "/already");
        assert_eq!(extract_path("no-slash"), "/no-slash");
    }
}
