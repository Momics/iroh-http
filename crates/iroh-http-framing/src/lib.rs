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

/// The HTTP/1.1 terminal chunk `0\r\n\r\n` that signals end of body.
pub fn terminal_chunk() -> &'static [u8] {
    b"0\r\n\r\n"
}

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
    fn chunk_round_trip() {
        let data = b"hello world";
        let encoded = encode_chunk(data);
        let (size, header_len) = parse_chunk_header(&encoded).unwrap();
        assert_eq!(size, data.len());
        assert_eq!(&encoded[header_len..header_len + size], data);
    }

    #[test]
    fn strip_iroh_node_id_header() {
        let headers = [("iroh-node-id", "fakevalue"), ("host", "peer1")];
        let bytes = serialize_request_head("GET", "/", &headers, false);
        let (_, _, hdrs, _) = parse_request_head(&bytes).unwrap();
        assert!(!hdrs.iter().any(|(k, _)| k.eq_ignore_ascii_case("iroh-node-id")));
    }
}
