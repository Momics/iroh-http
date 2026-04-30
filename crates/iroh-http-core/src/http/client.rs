//! Outgoing HTTP request — pure-Rust `fetch()` implementation.
//!
//! HTTP/1.1 framing is delegated entirely to hyper. Iroh's QUIC stream pair
//! is wrapped in `IrohStream` and handed to hyper's client connection API.
//!
//! Slice D (#186) split the original FFI-shaped `fetch(endpoint, &str, ...)`
//! into two layers:
//!
//! - [`fetch`] — pure-Rust API: takes a fully-formed [`hyper::Request<Body>`]
//!   and an [`iroh::EndpointAddr`], returns [`hyper::Response<Body>`] with a
//!   typed [`FetchError`]. No `u64` handles, no `BodyReader`, no string
//!   parsing of error messages.
//! - [`crate::ffi::fetch`] — FFI-shaped wrapper that builds the
//!   `Request<Body>` from flat strings, calls [`fetch`], and translates
//!   the response into a [`crate::FfiResponse`].

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;

use crate::{
    ffi::handles::BodyWriter,
    http::{server::stack::StackConfig, transport::io::IrohStream},
    IrohEndpoint, ALPN,
};

// ── Body type ────────────────────────────────────────────────────────

use crate::Body;

// ── Typed fetch error ────────────────────────────────────────────────────────

/// Typed error returned by the pure-Rust [`fetch`] API.
///
/// Replaces the string-matched `classify_hyper_error` that the old FFI
/// surface relied on (Slice D / #186). FFI callers translate variants
/// into [`crate::CoreError`] codes at the boundary.
#[derive(Debug)]
#[non_exhaustive]
pub enum FetchError {
    /// Connection setup, hyper handshake, or send-request transport
    /// failure that is *not* a header-size violation.
    ConnectionFailed { detail: String },
    /// Response head exceeded the endpoint's `max_header_size` budget,
    /// detected either by hyper's parser or by the post-receive byte
    /// count check in the FFI wrapper.
    HeaderTooLarge { detail: String },
    /// Response body exceeded the configured byte limit. Surfaced by the
    /// FFI wrapper after the body is drained, never by [`fetch`] itself.
    BodyTooLarge,
    /// `cfg.timeout` elapsed before the response head arrived.
    Timeout,
    /// Caller dropped the future or signalled cancellation via the FFI
    /// fetch token.
    Cancelled,
    /// Bug or unexpected internal failure (request build, body wrap, …).
    Internal(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::ConnectionFailed { detail } => write!(f, "connection failed: {detail}"),
            FetchError::HeaderTooLarge { detail } => {
                write!(f, "response header too large: {detail}")
            }
            FetchError::BodyTooLarge => f.write_str("response body too large"),
            FetchError::Timeout => f.write_str("request timed out"),
            FetchError::Cancelled => f.write_str("request cancelled"),
            FetchError::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// Classify a hyper send-request or handshake error. hyper 1.x lumps
/// header-parse failures into a single `Error::Parse` family; we
/// substring-match on the description because hyper does not expose the
/// underlying reason struct. This is the *only* place in the crate that
/// inspects hyper error strings — kept inside the typed-error mapping so
/// the public API never carries a string-coded error.
fn hyper_to_fetch_error(e: hyper::Error, context: &str) -> FetchError {
    let msg = format!("{context}: {e}");
    let lower = e.to_string().to_ascii_lowercase();
    if lower.contains("header") {
        FetchError::HeaderTooLarge { detail: msg }
    } else {
        FetchError::ConnectionFailed { detail: msg }
    }
}

// ── Pure-Rust fetch API ──────────────────────────────────────────────────────

/// Pure-Rust outbound entry — the canonical client API.
///
/// Establishes (or reuses, via [`crate::http::transport::pool`]) an Iroh
/// QUIC connection to `addr`, runs hyper's HTTP/1.1 client handshake on
/// a freshly opened bidirectional stream, dispatches `req` through the
/// shared client tower stack ([`crate::http::server::stack::build_client_stack`]),
/// and returns the response.
///
/// The returned [`hyper::Response<Body>`] carries the response body as a
/// streaming [`Body`]; the caller is responsible for draining it. The
/// body's lifetime is bound to a background task that drives hyper's
/// connection state machine — dropping the body without reading it is
/// fine, hyper will close the stream cleanly.
///
/// # Errors
///
/// Returns [`FetchError::Timeout`] if `cfg.timeout` is set and elapsed
/// before the response head arrived. Connection / handshake / transport
/// failures map to [`FetchError::ConnectionFailed`]; response heads that
/// exceed the endpoint's `max_header_size` map to
/// [`FetchError::HeaderTooLarge`].
pub async fn fetch(
    endpoint: &IrohEndpoint,
    addr: &iroh::EndpointAddr,
    req: hyper::Request<Body>,
    cfg: &StackConfig,
) -> Result<hyper::Response<Body>, FetchError> {
    let work = async {
        let node_id = addr.id;
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
            .map_err(|e| FetchError::ConnectionFailed { detail: e })?;

        let conn = pooled.conn.clone();

        let (send, recv) = conn
            .open_bi()
            .await
            .map_err(|e| FetchError::ConnectionFailed {
                detail: format!("open_bi: {e}"),
            })?;
        let io = TokioIo::new(IrohStream::new(send, recv));

        let (sender, conn_task) = hyper::client::conn::http1::Builder::new()
            // hyper requires max_buf_size >= 8192; clamp upward so small
            // max_header_size values don't panic. Header-size enforcement
            // happens via the response parsing error that hyper returns
            // when the actual response head exceeds max_header_size bytes.
            .max_buf_size(max_header_size.max(8192))
            .max_headers(128)
            .handshake::<_, Body>(io)
            .await
            .map_err(|e| hyper_to_fetch_error(e, "hyper handshake"))?;

        // Drive the connection state machine in the background.
        tokio::spawn(conn_task);

        // Dispatch through the shared client stack (Slice B / #184).
        // Composition of decompression + body normalisation lives in
        // [`crate::http::server::stack::build_client_stack`]; both
        // directions of the crate share one composition function.
        use tower::ServiceExt;
        let svc = crate::http::server::stack::build_client_stack(sender, cfg);
        svc.oneshot(req)
            .await
            .map_err(|e| hyper_to_fetch_error(e, "send_request"))
    };

    match cfg.timeout {
        Some(t) => match tokio::time::timeout(t, work).await {
            Ok(r) => r,
            Err(_) => Err(FetchError::Timeout),
        },
        None => work.await,
    }
}

// ── Body bridge utilities ─────────────────────────────────────────────────────

/// Drain a hyper body into `BodyWriter`.
/// Generic over any body type with `Data = Bytes` (e.g. `Incoming`, `DecompressionBody`).
#[allow(dead_code)]
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
    mut overflow_tx: Option<tokio::sync::oneshot::Sender<()>>,
) where
    B: http_body::Body<Data = Bytes>,
    B::Error: std::fmt::Debug,
{
    // Box::pin gives Pin<Box<B>>: Unpin (Box<T>: Unpin ∀T), which satisfies BodyExt::frame().
    let mut body = Box::pin(body);
    let mut total = 0usize;
    // Set to true once max_bytes is exceeded. When true, frames are read and
    // discarded so the peer's QUIC send stream can receive flow-control ACKs
    // and close cleanly instead of stalling until idle timeout (ISS-015).
    let mut overflowed = false;

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
                if overflowed {
                    // Drain: discard the frame but keep reading so the QUIC
                    // flow-control window advances and the peer can finish
                    // writing its body.
                    continue;
                }
                if frame.is_data() {
                    let data = frame.into_data().expect("is_data checked above");
                    total = total.saturating_add(data.len());
                    if let Some(limit) = max_bytes {
                        if total > limit {
                            tracing::warn!("iroh-http: request body exceeded {limit} bytes");
                            // ISS-004: signal overflow so the serve path can send 413.
                            if let Some(tx) = overflow_tx.take() {
                                let _ = tx.send(());
                            }
                            overflowed = true;
                            continue; // drain remaining frames without stalling the peer
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
