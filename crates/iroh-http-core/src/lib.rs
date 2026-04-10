//! `iroh-http-core` — Iroh QUIC endpoint, HTTP framing, fetch and serve.
//!
//! This crate owns the Iroh endpoint and wires HTTP/1.1 framing to QUIC
//! streams.  Nothing in here knows about JavaScript.

pub mod client;
pub mod endpoint;
pub mod server;
pub mod stream;

pub use endpoint::{IrohEndpoint, NodeOptions};
pub use stream::{alloc_body_writer, next_chunk, send_chunk, finish_body, BodyReader};
pub use client::fetch;
pub use server::serve;

/// Flat request struct that crosses the FFI boundary.
#[derive(Debug, Clone)]
pub struct FfiRequest {
    /// HTTP method, e.g. "GET".
    pub method: String,
    /// Full URL, e.g. `http+iroh://<node-id>/path`.
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
    /// Full `http+iroh://` URL of the responding peer, e.g. `http+iroh://<node-id>/path`.
    pub url: String,
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
    pub method: String,
    /// Full `http+iroh://` URL (server's own node-id + path).
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub remote_node_id: String,
}

/// ALPN protocol identifier for iroh-http/1.
pub const ALPN: &[u8] = b"iroh-http/1";

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
