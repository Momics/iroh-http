//! `iroh-http-framing` — no_std HTTP/1.1 header serialisation and parsing.
//!
//! This crate has no async, no I/O, and no dependency on Iroh or Tokio.
//! It serialises request/response head bytes and parses them back.
//! Body bytes are never touched here; the caller pumps them separately.
//!
//! # Usage
//!
//! ```rust
//! use iroh_http_framing::{serialize_request_head, parse_request_head};
//!
//! let bytes = serialize_request_head("GET", "/api/data", &[("host", "peer1")], false);
//! let (method, path, headers, _consumed) = parse_request_head(&bytes).unwrap();
//! assert_eq!(method, "GET");
//! assert_eq!(path, "/api/data");
//! ```

#![no_std]
extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

/// Error type for framing operations.
#[derive(Debug)]
pub enum FramingError {
    /// The buffer does not yet contain a complete head (caller should read more bytes).
    Incomplete,
    /// The head bytes could not be parsed as valid HTTP/1.1.
    Parse(String),
}

// ── Request head ─────────────────────────────────────────────────────────────

/// Serialize an HTTP/1.1 request head into bytes.
///
/// `method` and `path` are written into the request line.
/// `headers` are name/value pairs appended in order.
/// If `chunked` is true, `Transfer-Encoding: chunked` is appended automatically
/// (only when no `Content-Length` header is already present in `headers`).
pub fn serialize_request_head(
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    chunked: bool,
) -> Vec<u8> {
    let mut buf = Vec::new();
    push_str(&mut buf, method);
    buf.push(b' ');
    push_str(&mut buf, path);
    push_str(&mut buf, " HTTP/1.1\r\n");
    let has_content_len = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
    for (name, value) in headers {
        push_str(&mut buf, name);
        push_str(&mut buf, ": ");
        push_str(&mut buf, value);
        push_str(&mut buf, "\r\n");
    }
    if chunked && !has_content_len {
        push_str(&mut buf, "Transfer-Encoding: chunked\r\n");
    }
    push_str(&mut buf, "\r\n");
    buf
}

/// Parse an HTTP/1.1 request head from `bytes`.
///
/// Returns `(method, path, headers, bytes_consumed)` on success.
/// `headers` strips any `iroh-node-id` entry supplied by the peer (security).
pub fn parse_request_head(
    bytes: &[u8],
) -> Result<(String, String, Vec<(String, String)>, usize), FramingError> {
    let mut headers_buf = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers_buf);
    match req.parse(bytes) {
        Ok(httparse::Status::Complete(n)) => {
            let method = req.method.unwrap_or("GET").to_string();
            let path = req.path.unwrap_or("/").to_string();
            let headers = req
                .headers
                .iter()
                .filter(|h| !h.name.eq_ignore_ascii_case("iroh-node-id"))
                .map(|h| {
                    (
                        h.name.to_string(),
                        String::from_utf8_lossy(h.value).into_owned(),
                    )
                })
                .collect();
            Ok((method, path, headers, n))
        }
        Ok(httparse::Status::Partial) => Err(FramingError::Incomplete),
        Err(e) => Err(FramingError::Parse(e.to_string())),
    }
}

// ── Response head ─────────────────────────────────────────────────────────────

/// Serialize an HTTP/1.1 response head into bytes.
///
/// `status` is the three-digit status code, `reason` the reason phrase.
/// If `chunked` is true and no `Content-Length` is present, appends
/// `Transfer-Encoding: chunked`.
pub fn serialize_response_head(
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
    chunked: bool,
) -> Vec<u8> {
    let mut buf = Vec::new();
    push_str(&mut buf, "HTTP/1.1 ");
    push_u16(&mut buf, status);
    buf.push(b' ');
    push_str(&mut buf, reason);
    push_str(&mut buf, "\r\n");
    let has_content_len = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
    for (name, value) in headers {
        push_str(&mut buf, name);
        push_str(&mut buf, ": ");
        push_str(&mut buf, value);
        push_str(&mut buf, "\r\n");
    }
    if chunked && !has_content_len {
        push_str(&mut buf, "Transfer-Encoding: chunked\r\n");
    }
    push_str(&mut buf, "\r\n");
    buf
}

/// Parse an HTTP/1.1 response head from `bytes`.
///
/// Returns `(status, reason, headers, bytes_consumed)` on success.
pub fn parse_response_head(
    bytes: &[u8],
) -> Result<(u16, String, Vec<(String, String)>, usize), FramingError> {
    let mut headers_buf = [httparse::EMPTY_HEADER; 64];
    let mut res = httparse::Response::new(&mut headers_buf);
    match res.parse(bytes) {
        Ok(httparse::Status::Complete(n)) => {
            let status = res.code.unwrap_or(200);
            let reason = res.reason.unwrap_or("").to_string();
            let headers = res
                .headers
                .iter()
                .map(|h| {
                    (
                        h.name.to_string(),
                        String::from_utf8_lossy(h.value).into_owned(),
                    )
                })
                .collect();
            Ok((status, reason, headers, n))
        }
        Ok(httparse::Status::Partial) => Err(FramingError::Incomplete),
        Err(e) => Err(FramingError::Parse(e.to_string())),
    }
}

// ── Chunked body helpers ──────────────────────────────────────────────────────

/// Encode `data` as a single HTTP/1.1 chunked body segment:
/// `<hex-len>\r\n<data>\r\n`.
pub fn encode_chunk(data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    push_hex_usize(&mut buf, data.len());
    push_str(&mut buf, "\r\n");
    buf.extend_from_slice(data);
    push_str(&mut buf, "\r\n");
    buf
}

/// The HTTP/1.1 terminal chunk `0\r\n\r\n` that signals end of body (no trailers).
pub fn terminal_chunk() -> &'static [u8] {
    b"0\r\n\r\n"
}

/// The start of the terminal chunk `0\r\n` without the empty-trailer terminator.
///
/// Use this when you plan to write trailers immediately after.
/// Always follow with [`serialize_trailers`].
pub fn terminal_chunk_start() -> &'static [u8] {
    b"0\r\n"
}

/// Serialize a trailer header block that follows the terminal chunk.
///
/// Produces `Name: Value\r\n` entries terminated by an empty line `\r\n`.
/// An empty `trailers` slice produces just `\r\n` (equivalent to no trailers).
pub fn serialize_trailers(trailers: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = Vec::new();
    for (name, value) in trailers {
        push_str(&mut buf, name);
        push_str(&mut buf, ": ");
        push_str(&mut buf, value);
        push_str(&mut buf, "\r\n");
    }
    push_str(&mut buf, "\r\n");
    buf
}

/// Parse a trailer header block from `bytes` (data immediately after the `0\r\n` terminal chunk).
///
/// A lone `\r\n` is a valid empty trailer block.
/// Returns `(trailers, bytes_consumed)`, or `FramingError::Incomplete` when more data is needed.
pub fn parse_trailers(
    bytes: &[u8],
) -> Result<(Vec<(String, String)>, usize), FramingError> {
    // Empty block: just the trailing \r\n
    if bytes.starts_with(b"\r\n") {
        return Ok((Vec::new(), 2));
    }
    // Find the \r\n\r\n that ends the trailer section.
    let block_end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(FramingError::Incomplete)?;
    let total = block_end + 4;
    // `block` includes each header line's trailing \r\n so we can find value boundaries.
    let block = &bytes[..block_end + 2];

    let mut trailers = Vec::new();
    let mut pos = 0;

    while pos < block.len() {
        if block[pos..].starts_with(b"\r\n") {
            break;
        }
        let colon = block[pos..]
            .iter()
            .position(|&b| b == b':')
            .ok_or_else(|| FramingError::Parse("invalid trailer header".into()))?;
        let name = core::str::from_utf8(&block[pos..pos + colon])
            .map_err(|_| FramingError::Parse("invalid trailer name encoding".into()))?
            .trim()
            .to_string();
        let value_start = pos + colon + 1;
        let crlf = block[value_start..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| FramingError::Parse("missing CRLF in trailer".into()))?;
        let value = core::str::from_utf8(&block[value_start..value_start + crlf])
            .map_err(|_| FramingError::Parse("invalid trailer value encoding".into()))?
            .trim()
            .to_string();
        trailers.push((name, value));
        pos = value_start + crlf + 2;
    }

    Ok((trailers, total))
}

// ── ALPN identifiers ─────────────────────────────────────────────────────────

/// Base Iroh-HTTP protocol (unidirectional, no trailers).
pub const ALPN_BASE: &[u8] = b"iroh-http/1";
/// Base + bidirectional streaming (§2).
pub const ALPN_DUPLEX: &[u8] = b"iroh-http/1-duplex";
/// Base + trailer headers (§4).
pub const ALPN_TRAILERS: &[u8] = b"iroh-http/1-trailers";
/// Base + bidirectional + trailers + cancellation signals.
pub const ALPN_FULL: &[u8] = b"iroh-http/1-full";

/// Parse the header of one chunked segment from `data`.
///
/// Returns `Some((chunk_size, header_bytes_consumed))` when a complete
/// chunk header is found, or `None` if more bytes are needed.
pub fn parse_chunk_header(data: &[u8]) -> Option<(usize, usize)> {
    let crlf_pos = data.windows(2).position(|w| w == b"\r\n")?;
    let hex_str = core::str::from_utf8(&data[..crlf_pos]).ok()?;
    // Strip optional chunk extensions after ';'
    let hex_only = hex_str.split(';').next()?;
    let size = usize::from_str_radix(hex_only.trim(), 16).ok()?;
    Some((size, crlf_pos + 2))
}

// ── reason phrase helper ──────────────────────────────────────────────────────

/// Return a standard reason phrase for a status code.
pub fn reason_phrase(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        413 => "Content Too Large",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

// ── private helpers ───────────────────────────────────────────────────────────

fn push_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
}

fn push_u16(buf: &mut Vec<u8>, n: u16) {
    // Write decimal digits without std::format!
    let s = u16_to_decimal(n);
    buf.extend_from_slice(&s);
}

fn push_hex_usize(buf: &mut Vec<u8>, n: usize) {
    // Write hex digits for chunk size
    let s = usize_to_hex(n);
    buf.extend_from_slice(&s);
}

fn u16_to_decimal(mut n: u16) -> Vec<u8> {
    if n == 0 {
        return vec![b'0'];
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(b'0' + (n % 10) as u8);
        n /= 10;
    }
    digits.reverse();
    digits
}

fn usize_to_hex(mut n: usize) -> Vec<u8> {
    if n == 0 {
        return vec![b'0'];
    }
    let nibbles = b"0123456789abcdef";
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(nibbles[n & 0xf]);
        n >>= 4;
    }
    digits.reverse();
    digits
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    // ── Request head ────────────────────────────────────────────────────

    #[test]
    fn round_trip_request() {
        let headers = [("host", "peer1"), ("content-length", "0")];
        let bytes = serialize_request_head("GET", "/hello", &headers, false);
        let (method, path, hdrs, _) = parse_request_head(&bytes).unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path, "/hello");
        assert!(hdrs.iter().any(|(k, _)| k == "host"));
    }

    #[test]
    fn request_with_chunked_adds_te_header() {
        let bytes = serialize_request_head("POST", "/upload", &[], true);
        let (_, _, hdrs, _) = parse_request_head(&bytes).unwrap();
        assert!(hdrs.iter().any(|(k, v)|
            k.eq_ignore_ascii_case("Transfer-Encoding") && v == "chunked"
        ));
    }

    #[test]
    fn request_chunked_skipped_when_content_length_present() {
        let headers = [("content-length", "42")];
        let bytes = serialize_request_head("POST", "/up", &headers, true);
        let (_, _, hdrs, _) = parse_request_head(&bytes).unwrap();
        assert!(!hdrs.iter().any(|(k, _)| k.eq_ignore_ascii_case("Transfer-Encoding")));
    }

    #[test]
    fn parse_request_head_incomplete() {
        let partial = b"GET /hello HTTP/1.1\r\nhost: x\r\n"; // no final \r\n
        match parse_request_head(partial) {
            Err(FramingError::Incomplete) => {}
            other => panic!("expected Incomplete, got {other:?}"),
        }
    }

    #[test]
    fn parse_request_head_garbage() {
        let garbage = b"NOT A VALID HTTP REQUEST\r\n\r\n";
        match parse_request_head(garbage) {
            Err(FramingError::Parse(_)) => {}
            other => panic!("expected Parse error, got {other:?}"),
        }
    }

    #[test]
    fn strip_iroh_node_id_header() {
        let headers = [("iroh-node-id", "fakevalue"), ("host", "peer1")];
        let bytes = serialize_request_head("GET", "/", &headers, false);
        let (_, _, hdrs, _) = parse_request_head(&bytes).unwrap();
        assert!(!hdrs.iter().any(|(k, _)| k.eq_ignore_ascii_case("iroh-node-id")));
    }

    #[test]
    fn request_preserves_all_standard_headers() {
        let headers = [
            ("host", "peer1"),
            ("authorization", "Bearer tok"),
            ("x-custom", "val"),
        ];
        let bytes = serialize_request_head("DELETE", "/resource/42", &headers, false);
        let (method, path, hdrs, consumed) = parse_request_head(&bytes).unwrap();
        assert_eq!(method, "DELETE");
        assert_eq!(path, "/resource/42");
        assert_eq!(consumed, bytes.len());
        assert!(hdrs.iter().any(|(k, v)| k == "authorization" && v == "Bearer tok"));
        assert!(hdrs.iter().any(|(k, v)| k == "x-custom" && v == "val"));
    }

    // ── Response head ───────────────────────────────────────────────────

    #[test]
    fn round_trip_response() {
        let headers = [("content-type", "text/plain")];
        let bytes = serialize_response_head(200, "OK", &headers, true);
        let (status, reason, hdrs, _) = parse_response_head(&bytes).unwrap();
        assert_eq!(status, 200);
        assert_eq!(reason, "OK");
        assert!(hdrs
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("Transfer-Encoding")));
    }

    #[test]
    fn response_chunked_skipped_when_content_length_present() {
        let headers = [("content-length", "100")];
        let bytes = serialize_response_head(200, "OK", &headers, true);
        let (_, _, hdrs, _) = parse_response_head(&bytes).unwrap();
        assert!(!hdrs.iter().any(|(k, _)| k.eq_ignore_ascii_case("Transfer-Encoding")));
    }

    #[test]
    fn parse_response_head_incomplete() {
        let partial = b"HTTP/1.1 200 OK\r\ncontent-type: text\r\n";
        match parse_response_head(partial) {
            Err(FramingError::Incomplete) => {}
            other => panic!("expected Incomplete, got {other:?}"),
        }
    }

    #[test]
    fn parse_response_head_garbage() {
        let garbage = b"GARBAGE RESPONSE\r\n\r\n";
        match parse_response_head(garbage) {
            Err(FramingError::Parse(_)) => {}
            other => panic!("expected Parse error, got {other:?}"),
        }
    }

    #[test]
    fn response_with_various_status_codes() {
        for (status, expected_reason) in &[
            (201, "Created"), (204, "No Content"), (404, "Not Found"),
            (500, "Internal Server Error"),
        ] {
            let bytes = serialize_response_head(*status, expected_reason, &[], false);
            let (s, r, _, _) = parse_response_head(&bytes).unwrap();
            assert_eq!(s, *status);
            assert_eq!(r, *expected_reason);
        }
    }

    // ── Chunked encoding ────────────────────────────────────────────────

    #[test]
    fn chunk_round_trip() {
        let data = b"hello world";
        let encoded = encode_chunk(data);
        let (size, header_len) = parse_chunk_header(&encoded).unwrap();
        assert_eq!(size, data.len());
        assert_eq!(&encoded[header_len..header_len + size], data);
    }

    #[test]
    fn chunk_empty_data() {
        let encoded = encode_chunk(b"");
        let (size, _) = parse_chunk_header(&encoded).unwrap();
        assert_eq!(size, 0);
    }

    #[test]
    fn parse_chunk_header_incomplete() {
        assert!(parse_chunk_header(b"b").is_none());
        assert!(parse_chunk_header(b"").is_none());
    }

    #[test]
    fn parse_chunk_header_with_extension() {
        // Chunk extensions after ';' should be ignored
        let data = b"a;ext=foo\r\nhello12345\r\n";
        let (size, header_len) = parse_chunk_header(data).unwrap();
        assert_eq!(size, 10);
        assert_eq!(&data[header_len..header_len + size], b"hello12345");
    }

    #[test]
    fn terminal_chunk_values() {
        assert_eq!(terminal_chunk(), b"0\r\n\r\n");
        assert_eq!(terminal_chunk_start(), b"0\r\n");
    }

    // ── Trailers ────────────────────────────────────────────────────────

    #[test]
    fn trailers_round_trip() {
        let trailers = [("x-checksum", "abc123"), ("x-hash", "def456")];
        let bytes = serialize_trailers(&trailers);
        let (parsed, consumed) = parse_trailers(&bytes).unwrap();
        assert_eq!(consumed, bytes.len());
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], ("x-checksum".into(), "abc123".into()));
        assert_eq!(parsed[1], ("x-hash".into(), "def456".into()));
    }

    #[test]
    fn trailers_empty() {
        let bytes = serialize_trailers(&[]);
        assert_eq!(bytes, b"\r\n");
        let (parsed, consumed) = parse_trailers(&bytes).unwrap();
        assert_eq!(consumed, 2);
        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_trailers_incomplete() {
        let partial = b"x-checksum: abc123\r\n"; // no trailing \r\n
        match parse_trailers(partial) {
            Err(FramingError::Incomplete) => {}
            other => panic!("expected Incomplete, got {other:?}"),
        }
    }

    #[test]
    fn parse_trailers_no_colon() {
        let bad = b"no-colon-here\r\n\r\n";
        match parse_trailers(bad) {
            Err(FramingError::Parse(_)) => {}
            other => panic!("expected Parse error, got {other:?}"),
        }
    }

    // ── reason_phrase ───────────────────────────────────────────────────

    #[test]
    fn reason_phrase_known_codes() {
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(500), "Internal Server Error");
        assert_eq!(reason_phrase(101), "Switching Protocols");
    }

    #[test]
    fn reason_phrase_unknown_code() {
        assert_eq!(reason_phrase(999), "Unknown");
        assert_eq!(reason_phrase(0), "Unknown");
    }

    // ── consumed bytes correctness ──────────────────────────────────────

    #[test]
    fn request_head_consumed_is_exact() {
        let headers = [("host", "peer1")];
        let head = serialize_request_head("GET", "/", &headers, false);
        let extra = b"BODY DATA HERE";
        let mut combined = head.clone();
        combined.extend_from_slice(extra);
        let (_, _, _, consumed) = parse_request_head(&combined).unwrap();
        assert_eq!(consumed, head.len());
    }

    #[test]
    fn response_head_consumed_is_exact() {
        let head = serialize_response_head(200, "OK", &[], false);
        let extra = b"RESPONSE BODY";
        let mut combined = head.clone();
        combined.extend_from_slice(extra);
        let (_, _, _, consumed) = parse_response_head(&combined).unwrap();
        assert_eq!(consumed, head.len());
    }
}
