//! `iroh-http-framing` — HTTP chunked body encoding/decoding and trailer serialisation.
//!
//! This crate has no async, no I/O, and no dependency on Iroh or Tokio.
//! It provides chunk framing helpers used by `iroh-http-core` to encode/decode
//! HTTP/1.1 chunked transfer encoding and trailer headers.
//!
//! Request and response heads are handled separately via QPACK in `iroh-http-core`.
//!
//! # Usage
//!
//! ```rust
//! use iroh_http_framing::{encode_chunk, parse_chunk_header, terminal_chunk};
//!
//! let data = b"hello world";
//! let encoded = encode_chunk(data);
//! let (size, header_len) = parse_chunk_header(&encoded).unwrap();
//! assert_eq!(size, data.len());
//! assert_eq!(&encoded[header_len..header_len + size], data);
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

// ── internal helpers ──────────────────────────────────────────────────────────

fn push_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
}

fn push_hex_usize(buf: &mut Vec<u8>, n: usize) {
    let s = usize_to_hex(n);
    buf.extend_from_slice(&s);
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
}
