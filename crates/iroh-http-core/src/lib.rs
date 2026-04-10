//! `iroh-http-core` — Iroh QUIC endpoint, HTTP framing, fetch and serve.
//!
//! This crate owns the Iroh endpoint and wires HTTP/1.1 framing to QUIC
//! streams.  Nothing in here knows about JavaScript.

pub mod client;
pub mod endpoint;
pub mod server;
pub mod stream;

pub use endpoint::{IrohEndpoint, NodeOptions};
pub use stream::{
    alloc_body_writer, next_chunk, send_chunk, finish_body, cancel_reader,
    next_trailer, send_trailers, BodyReader,
};
pub use client::{fetch, raw_connect, alloc_fetch_token, cancel_in_flight};
pub use server::serve;

// ── Structured error serialization ───────────────────────────────────────────

/// Classify a Rust error message and return a JSON string
/// `{"code":"CODE","message":"..."}` suitable for FFI error channels.
///
/// Adapters should use this instead of `.to_string()` so that JS can
/// dispatch by stable error codes rather than fragile regex matching.
pub fn classify_error_json(e: impl std::fmt::Display) -> String {
    let msg = e.to_string();
    let code = classify_error_code(&msg);
    // Minimal JSON string escaping — only sequences that are structurally
    // significant inside a JSON string value.
    let escaped = msg
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("{{\"code\":\"{code}\",\"message\":\"{escaped}\"}}")
}

fn classify_error_code(msg: &str) -> &'static str {
    let m = &msg.to_lowercase();
    if m.contains("timed out") || m.contains("timeout") || m.contains("deadline") {
        "TIMEOUT"
    } else if m.contains("dns") || m.contains("resolv") {
        "DNS_FAILURE"
    } else if m.contains("alpn") {
        "ALPN_MISMATCH"
    } else if (m.contains("upgrade") && m.contains("reject")) || m.contains("non-101") {
        "UPGRADE_REJECTED"
    } else if m.contains("parse") && (m.contains("response head") || m.contains("request head")) {
        "PARSE_FAILURE"
    } else if m.contains("too many headers") {
        "TOO_MANY_HEADERS"
    } else if (m.contains("invalid") || m.contains("unknown")) && m.contains("handle") {
        "INVALID_HANDLE"
    } else if m.contains("writer dropped") {
        "WRITER_DROPPED"
    } else if m.contains("reader dropped") {
        "READER_DROPPED"
    } else if m.contains("stream reset") {
        "STREAM_RESET"
    } else if m.contains("connection") && (m.contains("refused") || m.contains("reset") || m.contains("closed")) {
        "REFUSED"
    } else if m.contains("connect") {
        "REFUSED"
    } else if (m.contains("invalid") && m.contains("key")) || m.contains("key bytes") || m.contains("wrong length") {
        "INVALID_KEY"
    } else if m.contains("bind") || m.contains("endpoint") {
        "ENDPOINT_FAILURE"
    } else {
        "UNKNOWN"
    }
}

/// Flat request struct that crosses the FFI boundary.
#[derive(Debug, Clone)]
pub struct FfiRequest {
    /// HTTP method, e.g. "GET".
    pub method: String,
    /// Full URL, e.g. `httpi://<node-id>/path`.
    pub url: String,
    /// Request headers (iroh-node-id already stripped by framing layer).
    pub headers: Vec<(String, String)>,
    /// Authenticated remote peer identity from the QUIC connection.
    pub remote_node_id: String,
}

/// Flat response-head struct that crosses the FFI boundary.
#[derive(Debug, Clone)]
pub struct FfiResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Handle to a [`BodyReader`] containing the response body.
    pub body_handle: u32,
    /// Full `httpi://` URL of the responding peer, e.g. `httpi://<node-id>/path`.
    pub url: String,
    /// Handle to a trailer receiver — call `next_trailer(handle)` after draining
    /// the body to retrieve any response trailers.
    pub trailers_handle: u32,
}

/// Options passed to the JS serve callback per incoming request.
#[derive(Debug)]
pub struct RequestPayload {
    /// Opaque handle used to send the response head back via [`server::respond`].
    pub req_handle: u32,
    /// Handle to a [`BodyReader`] for reading the request body.
    pub req_body_handle: u32,
    /// Handle to a [`stream::BodyWriter`] that the handler writes the response body into.
    pub res_body_handle: u32,
    /// Handle to a trailer receiver for reading request trailers (after body is consumed).
    /// `0` in duplex mode (trailers not supported for duplex connections).
    pub req_trailers_handle: u32,
    /// Handle to a trailer sender for delivering response trailers.
    /// JS calls `sendTrailers(resTrailersHandle, pairs)` after `finishBody`.
    /// `0` in duplex mode.
    pub res_trailers_handle: u32,
    pub method: String,
    /// Full `httpi://` URL (server's own node-id + path).
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub remote_node_id: String,
    /// True when the client sent `Upgrade: iroh-duplex` — both stream directions
    /// are open immediately after the 101 response.
    pub is_bidi: bool,
}

/// Handles for the two sides of a full-duplex QUIC stream.
///
/// Returned by [`raw_connect`] when the server accepts the upgrade.
#[derive(Debug)]
pub struct FfiDuplexStream {
    /// Body reader handle — JS calls `nextChunk(readHandle)` to receive data from the server.
    pub read_handle: u32,
    /// Body writer handle — JS calls `sendChunk(writeHandle, …)` / `finishBody(writeHandle)`.
    pub write_handle: u32,
}

/// ALPN protocol identifier for the base iroh-http/1 protocol.
pub const ALPN: &[u8] = b"iroh-http/1";
/// ALPN for base + bidirectional streaming.
pub const ALPN_DUPLEX: &[u8] = b"iroh-http/1-duplex";
/// ALPN for base + trailer headers.
pub const ALPN_TRAILERS: &[u8] = b"iroh-http/1-trailers";
/// ALPN for base + bidirectional + trailers + cancellation.
pub const ALPN_FULL: &[u8] = b"iroh-http/1-full";

/// Encode 32 raw bytes as lowercase base32 (no padding).
pub(crate) fn base32_encode(bytes: &[u8]) -> String {
    const BASE32: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut result = String::new();
    let mut bits: u32 = 0;
    let mut value: u32 = 0;
    for &byte in bytes {
        value = (value << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            result.push(BASE32[((value >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        result.push(BASE32[((value << (5 - bits)) & 0x1f) as usize] as char);
    }
    result
}

/// Decode a lowercase base32 string (no padding) to bytes.
pub(crate) fn base32_decode(s: &str) -> Result<Vec<u8>, String> {
    const BASE32: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut bytes = Vec::new();
    let mut bits: u32 = 0;
    let mut value: u32 = 0;
    for c in s.chars() {
        let v = BASE32
            .iter()
            .position(|&b| b as char == c.to_ascii_lowercase())
            .ok_or_else(|| format!("invalid base32 char: {c}"))? as u32;
        value = (value << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push((value >> bits) as u8);
        }
    }
    Ok(bytes)
}

/// Parse a base32 node-id string into an `iroh::PublicKey`.
pub(crate) fn parse_node_id(s: &str) -> Result<iroh::PublicKey, String> {
    let bytes = base32_decode(s)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "node-id must be 32 bytes".to_string())?;
    iroh::PublicKey::from_bytes(&arr).map_err(|e| e.to_string())
}
