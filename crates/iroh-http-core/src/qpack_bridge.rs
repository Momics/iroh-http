//! Thin bridge between iroh-http and the `qpack` crate.
//!
//! Provides QPACK header compression for iroh-http request/response heads.
//!
//! ## Wire format
//!
//! ```text
//! [ 2-byte big-endian length ] [ QPACK-encoded header block ]
//! ```
//!
//! Pseudo-headers follow HTTP/3 conventions:
//!   - Request:  `:method`, `:path`
//!   - Response: `:status`

use bytes::BufMut;
use qpack::{decode_stateless, encode_stateless, HeaderField};

/// Maximum decoded header size accepted by `decode_stateless` (256 KB).
const MAX_HEADER_SIZE: u64 = 256 * 1024;

// ── Stateless (Phase 1) ──────────────────────────────────────────────────────

/// Encode a request head (method, path, headers) into the wire format:
/// `[2-byte length][QPACK block]`.
fn encode_request_stateless(
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> Result<Vec<u8>, String> {
    let fields = build_request_fields(method, path, headers);
    encode_fields_to_wire(&fields)
}

/// Encode a response head (status, headers) into the wire format.
fn encode_response_stateless(status: u16, headers: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    let fields = build_response_fields(status, headers);
    encode_fields_to_wire(&fields)
}

/// Decode a request head from the wire format.
///
/// Returns `(method, path, headers, bytes_consumed)`.
#[allow(clippy::type_complexity)]
fn decode_request_stateless(
    buf: &[u8],
) -> Result<(String, String, Vec<(String, String)>, usize), DecodeError> {
    let (fields, consumed) = decode_wire_block(buf)?;
    let mut method = None;
    let mut path = None;
    let mut headers = Vec::new();

    for f in fields {
        let name = String::from_utf8_lossy(&f.name).to_string();
        let value = String::from_utf8_lossy(&f.value).to_string();
        match name.as_str() {
            ":method" => method = Some(value),
            ":path" => path = Some(value),
            _ => headers.push((name, value)),
        }
    }

    let method = method.ok_or(DecodeError::MissingPseudo(":method"))?;
    let path = path.ok_or(DecodeError::MissingPseudo(":path"))?;
    Ok((method, path, headers, consumed))
}

/// Decode a response head from the wire format.
///
/// Returns `(status, headers, bytes_consumed)`.
#[allow(clippy::type_complexity)]
fn decode_response_stateless(
    buf: &[u8],
) -> Result<(u16, Vec<(String, String)>, usize), DecodeError> {
    let (fields, consumed) = decode_wire_block(buf)?;
    let mut status = None;
    let mut headers = Vec::new();

    for f in fields {
        let name = String::from_utf8_lossy(&f.name).to_string();
        let value = String::from_utf8_lossy(&f.value).to_string();
        if name == ":status" {
            status = Some(
                value
                    .parse::<u16>()
                    .map_err(|_| DecodeError::InvalidStatus(value.clone()))?,
            );
        } else {
            headers.push((name, value));
        }
    }

    let status = status.ok_or(DecodeError::MissingPseudo(":status"))?;
    Ok((status, headers, consumed))
}

// ── Stateful (Phase 2) ──────────────────────────────────────────────────────
//
// The `qpack` crate v0.1.0 does not publicly export `Encoder` / `Decoder`,
// so true dynamic-table compression is not yet available.  This wrapper
// presents the same API surface so callers can be written once; internally
// it delegates to stateless encode/decode.  When a future qpack version
// (or a fork) exposes the stateful types, only this struct needs to change.

/// Per-connection QPACK codec state.
///
/// Currently backed by stateless encoding (Phase 1).  The struct is kept
/// so that `pool.rs` and `client.rs` / `server.rs` are already wired for
/// per-connection codec state.
pub struct QpackCodec {
    _private: (),
}

impl QpackCodec {
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Encode a request head.
    pub fn encode_request(
        &mut self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> Result<Vec<u8>, String> {
        encode_request_stateless(method, path, headers)
    }

    /// Encode a response head.
    pub fn encode_response(
        &mut self,
        status: u16,
        headers: &[(&str, &str)],
    ) -> Result<Vec<u8>, String> {
        encode_response_stateless(status, headers)
    }

    /// Decode a request head.
    #[allow(clippy::type_complexity)]
    pub fn decode_request(
        &mut self,
        buf: &[u8],
    ) -> Result<(String, String, Vec<(String, String)>, usize), DecodeError> {
        decode_request_stateless(buf)
    }

    /// Decode a response head.
    #[allow(clippy::type_complexity)]
    pub fn decode_response(
        &mut self,
        buf: &[u8],
    ) -> Result<(u16, Vec<(String, String)>, usize), DecodeError> {
        decode_response_stateless(buf)
    }
}

impl Default for QpackCodec {
    fn default() -> Self {
        Self::new()
    }
}

// ── Decode error type ────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum DecodeError {
    /// Not enough bytes yet — caller should read more.
    Incomplete,
    /// The QPACK block could not be decoded.
    Qpack(String),
    /// A required pseudo-header is missing.
    MissingPseudo(&'static str),
    /// The `:status` value is not a valid `u16`.
    InvalidStatus(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Incomplete => write!(f, "incomplete QPACK block"),
            DecodeError::Qpack(msg) => write!(f, "QPACK decode error: {msg}"),
            DecodeError::MissingPseudo(name) => write!(f, "missing pseudo-header {name}"),
            DecodeError::InvalidStatus(val) => write!(f, "invalid :status value: {val}"),
        }
    }
}

impl std::error::Error for DecodeError {}

// ── Shared helpers ───────────────────────────────────────────────────────────

fn build_request_fields(method: &str, path: &str, headers: &[(&str, &str)]) -> Vec<HeaderField> {
    let mut fields = Vec::with_capacity(2 + headers.len());
    fields.push(HeaderField::new(":method", method));
    fields.push(HeaderField::new(":path", path));
    for (k, v) in headers {
        fields.push(HeaderField::new(*k, *v));
    }
    fields
}

fn build_response_fields(status: u16, headers: &[(&str, &str)]) -> Vec<HeaderField> {
    let mut fields = Vec::with_capacity(1 + headers.len());
    fields.push(HeaderField::new(":status", status.to_string()));
    for (k, v) in headers {
        fields.push(HeaderField::new(*k, *v));
    }
    fields
}

/// Encode a list of header fields into the wire format using stateless encoding.
fn encode_fields_to_wire(fields: &[HeaderField]) -> Result<Vec<u8>, String> {
    let mut block = Vec::new();
    encode_stateless(&mut block, fields).map_err(|e| format!("qpack encode: {e}"))?;
    let len = block.len();
    if len > u16::MAX as usize {
        return Err(format!("QPACK block too large: {len} bytes"));
    }
    let mut wire = Vec::with_capacity(2 + len);
    wire.put_u16(len as u16);
    wire.extend_from_slice(&block);
    Ok(wire)
}

/// Decode a wire-format block (2-byte length prefix + QPACK) using stateless decoding.
fn decode_wire_block(buf: &[u8]) -> Result<(Vec<HeaderField>, usize), DecodeError> {
    if buf.len() < 2 {
        return Err(DecodeError::Incomplete);
    }
    let block_len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
    let total = 2 + block_len;
    if buf.len() < total {
        return Err(DecodeError::Incomplete);
    }

    let mut block = &buf[2..total];
    let decoded = decode_stateless(&mut block, MAX_HEADER_SIZE)
        .map_err(|e| DecodeError::Qpack(format!("{e}")))?;
    Ok((decoded.fields, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stateless_request_roundtrip() {
        let headers = [("content-type", "application/json"), ("x-custom", "hello")];
        let wire = encode_request_stateless("POST", "/api/data", &headers).unwrap();

        let (method, path, decoded_headers, consumed) = decode_request_stateless(&wire).unwrap();
        assert_eq!(method, "POST");
        assert_eq!(path, "/api/data");
        assert_eq!(consumed, wire.len());
        assert_eq!(decoded_headers.len(), 2);
        assert_eq!(
            decoded_headers[0],
            ("content-type".to_string(), "application/json".to_string())
        );
        assert_eq!(
            decoded_headers[1],
            ("x-custom".to_string(), "hello".to_string())
        );
    }

    #[test]
    fn stateless_response_roundtrip() {
        let headers = [("content-type", "text/plain"), ("x-req-id", "42")];
        let wire = encode_response_stateless(200, &headers).unwrap();

        let (status, decoded_headers, consumed) = decode_response_stateless(&wire).unwrap();
        assert_eq!(status, 200);
        assert_eq!(consumed, wire.len());
        assert_eq!(decoded_headers.len(), 2);
        assert_eq!(
            decoded_headers[0],
            ("content-type".to_string(), "text/plain".to_string())
        );
    }

    #[test]
    fn stateless_incomplete_returns_error() {
        // Empty buffer
        assert!(matches!(
            decode_request_stateless(&[]),
            Err(DecodeError::Incomplete)
        ));
        // Only length prefix, no body
        assert!(matches!(
            decode_request_stateless(&[0, 10]),
            Err(DecodeError::Incomplete)
        ));
    }

    #[test]
    fn stateless_missing_pseudo_header() {
        // Encode a response but try to decode as request — should fail.
        let wire = encode_response_stateless(200, &[]).unwrap();
        assert!(matches!(
            decode_request_stateless(&wire),
            Err(DecodeError::MissingPseudo(":method"))
        ));
    }

    #[test]
    fn stateful_request_roundtrip() {
        let mut codec = QpackCodec::new();
        let headers = [("accept", "text/html")];
        let wire = codec
            .encode_request("GET", "/index.html", &headers)
            .unwrap();

        let mut decoder_codec = QpackCodec::new();
        let (method, path, decoded_headers, consumed) =
            decoder_codec.decode_request(&wire).unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path, "/index.html");
        assert_eq!(consumed, wire.len());
        assert_eq!(
            decoded_headers,
            vec![("accept".to_string(), "text/html".to_string())]
        );
    }

    #[test]
    fn stateful_response_roundtrip() {
        let mut codec = QpackCodec::new();
        let headers = [("server", "iroh")];
        let wire = codec.encode_response(404, &headers).unwrap();

        let mut decoder_codec = QpackCodec::new();
        let (status, decoded_headers, consumed) = decoder_codec.decode_response(&wire).unwrap();
        assert_eq!(status, 404);
        assert_eq!(consumed, wire.len());
        assert_eq!(
            decoded_headers,
            vec![("server".to_string(), "iroh".to_string())]
        );
    }

    #[test]
    fn wire_format_length_prefix() {
        let wire = encode_request_stateless("GET", "/", &[]).unwrap();
        // First two bytes are the big-endian length of the QPACK block.
        let block_len = u16::from_be_bytes([wire[0], wire[1]]) as usize;
        assert_eq!(wire.len(), 2 + block_len);
    }
}
