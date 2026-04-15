//! `iroh-http-core` — Iroh QUIC endpoint, HTTP/1.1 via hyper, fetch and serve.
//!
//! This crate owns the Iroh endpoint and wires HTTP/1.1 framing to QUIC
//! streams via hyper.  Nothing in here knows about JavaScript.

pub mod client;
pub mod endpoint;
pub(crate) mod io;
pub(crate) mod pool;
pub mod registry;
pub mod server;
pub mod session;
pub mod stream;

pub use client::{fetch, raw_connect};
#[cfg(feature = "compression")]
pub use endpoint::CompressionOptions;
pub use endpoint::{
    parse_direct_addrs, IrohEndpoint, NodeAddrInfo, NodeOptions, PathInfo, PeerStats,
};
pub use registry::{get_endpoint, insert_endpoint, remove_endpoint};
pub use server::serve;
pub use server::ServeHandle;
pub use server::ServerLimits;
pub use session::{
    session_accept, session_close, session_closed, session_connect, session_create_bidi_stream,
    session_create_uni_stream, session_max_datagram_size, session_next_bidi_stream,
    session_next_uni_stream, session_ready, session_recv_datagram, session_remote_id,
    session_send_datagram, CloseInfo,
};
pub use stream::{BodyReader, HandleStore, StoreConfig};
pub use server::respond;

// ── Structured error types ────────────────────────────────────────────────────

/// Machine-readable error codes for the FFI boundary.
///
/// Platform adapters match on this directly — no string parsing needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    InvalidInput,
    ConnectionFailed,
    Timeout,
    BodyTooLarge,
    HeaderTooLarge,
    PeerRejected,
    Cancelled,
    Internal,
}

/// Structured error returned by core functions.
///
/// `code` is machine-readable. `message` carries human-readable detail.
#[derive(Debug, Clone)]
pub struct CoreError {
    pub code: ErrorCode,
    pub message: String,
}

impl CoreError {
    pub fn invalid_input(detail: impl std::fmt::Display) -> Self {
        CoreError {
            code: ErrorCode::InvalidInput,
            message: detail.to_string(),
        }
    }
    pub fn connection_failed(detail: impl std::fmt::Display) -> Self {
        CoreError {
            code: ErrorCode::ConnectionFailed,
            message: detail.to_string(),
        }
    }
    pub fn timeout(detail: impl std::fmt::Display) -> Self {
        CoreError {
            code: ErrorCode::Timeout,
            message: detail.to_string(),
        }
    }
    pub fn body_too_large(detail: impl std::fmt::Display) -> Self {
        CoreError {
            code: ErrorCode::BodyTooLarge,
            message: detail.to_string(),
        }
    }
    pub fn header_too_large(detail: impl std::fmt::Display) -> Self {
        CoreError {
            code: ErrorCode::HeaderTooLarge,
            message: detail.to_string(),
        }
    }
    pub fn peer_rejected(detail: impl std::fmt::Display) -> Self {
        CoreError {
            code: ErrorCode::PeerRejected,
            message: detail.to_string(),
        }
    }
    pub fn internal(detail: impl std::fmt::Display) -> Self {
        CoreError {
            code: ErrorCode::Internal,
            message: detail.to_string(),
        }
    }
    pub fn invalid_handle(handle: u32) -> Self {
        CoreError {
            code: ErrorCode::InvalidInput,
            message: format!("unknown handle: {handle}"),
        }
    }
    pub fn cancelled() -> Self {
        CoreError {
            code: ErrorCode::Cancelled,
            message: "aborted".to_string(),
        }
    }
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for CoreError {}

// ── ALPN protocol identifiers ─────────────────────────────────────────────────

/// ALPN for the HTTP/1.1-over-QUIC protocol (version 2 wire format).
pub const ALPN: &[u8] = b"iroh-http/2";
/// ALPN for base + bidirectional streaming (duplex/raw_connect).
pub const ALPN_DUPLEX: &[u8] = b"iroh-http/2-duplex";

// ── Key operations ───────────────────────────────────────────────────────────

/// Sign arbitrary bytes with a 32-byte Ed25519 secret key.
/// Returns a 64-byte signature, or `Err` if the underlying crypto panics.
pub fn secret_key_sign(secret_key_bytes: &[u8; 32], data: &[u8]) -> Result<[u8; 64], CoreError> {
    std::panic::catch_unwind(|| {
        let key = iroh::SecretKey::from_bytes(secret_key_bytes);
        key.sign(data).to_bytes()
    })
    .map_err(|_| CoreError::internal("secret_key_sign panicked"))
}

/// Verify a 64-byte Ed25519 signature against a 32-byte public key.
/// Returns `true` on success, `false` on any failure (including panics).
pub fn public_key_verify(public_key_bytes: &[u8; 32], data: &[u8], sig_bytes: &[u8; 64]) -> bool {
    std::panic::catch_unwind(|| {
        let Ok(key) = iroh::PublicKey::from_bytes(public_key_bytes) else {
            return false;
        };
        let sig = iroh::Signature::from_bytes(sig_bytes);
        key.verify(data, &sig).is_ok()
    })
    .unwrap_or(false)
}

/// Generate a fresh Ed25519 secret key. Returns 32 raw bytes, or `Err` if the RNG panics.
pub fn generate_secret_key() -> Result<[u8; 32], CoreError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        iroh::SecretKey::generate(&mut rand::rng()).to_bytes()
    }))
    .map_err(|_| CoreError::internal("generate_secret_key panicked"))
}

// ── Encode bytes as base32 ────────────────────────────────────────────────────

/// Encode bytes as lowercase RFC 4648 base32 (no padding).
pub fn base32_encode(bytes: &[u8]) -> String {
    base32::encode(base32::Alphabet::Rfc4648Lower { padding: false }, bytes)
}

/// Decode an RFC 4648 base32 string (no padding, case-insensitive) to bytes.
pub(crate) fn base32_decode(s: &str) -> Result<Vec<u8>, String> {
    base32::decode(base32::Alphabet::Rfc4648Lower { padding: false }, s)
        .ok_or_else(|| format!("invalid base32 string: {s}"))
}

/// Parse a base32 node-id string into an `iroh::PublicKey`.
pub(crate) fn parse_node_id(s: &str) -> Result<iroh::PublicKey, CoreError> {
    let bytes = base32_decode(s).map_err(CoreError::invalid_input)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CoreError::invalid_input("node-id must be 32 bytes"))?;
    iroh::PublicKey::from_bytes(&arr).map_err(|e| CoreError::invalid_input(e.to_string()))
}

// ── Node tickets ──────────────────────────────────────────────────────────────

/// Generate a ticket string for the given endpoint.
///
/// ISS-025: returns `Result` so serialization failures are surfaced to callers
/// instead of being masked as empty strings.
pub fn node_ticket(ep: &IrohEndpoint) -> Result<String, CoreError> {
    let info = ep.node_addr();
    serde_json::to_string(&info).map_err(|e| {
        CoreError::internal(format!("failed to serialize node ticket: {e}"))
    })
}

/// Parsed node address from a ticket string, bare node ID, or JSON address info.
pub struct ParsedNodeAddr {
    pub node_id: iroh::PublicKey,
    pub direct_addrs: Vec<std::net::SocketAddr>,
}

/// Parse a string that may be a bare node ID, a ticket string (JSON-encoded
/// `NodeAddrInfo`), or a JSON object with `id` and `addrs` fields.
///
/// ISS-023: malformed entries that look like socket addresses but fail to parse
/// cause a deterministic error. Entries that are clearly not socket addresses
/// (e.g. relay URLs containing `://`) are silently skipped and handled
/// elsewhere in the protocol stack.
pub fn parse_node_addr(s: &str) -> Result<ParsedNodeAddr, CoreError> {
    if let Ok(info) = serde_json::from_str::<NodeAddrInfo>(s) {
        let node_id = parse_node_id(&info.id)?;
        let mut direct_addrs = Vec::new();
        for addr_str in &info.addrs {
            // Skip relay URLs — they are handled by the relay subsystem.
            if addr_str.contains("://") {
                continue;
            }
            let addr = addr_str
                .parse::<std::net::SocketAddr>()
                .map_err(|_| CoreError::invalid_input(format!("malformed address: {addr_str}")))?;
            direct_addrs.push(addr);
        }
        return Ok(ParsedNodeAddr {
            node_id,
            direct_addrs,
        });
    }
    let node_id = parse_node_id(s)?;
    Ok(ParsedNodeAddr {
        node_id,
        direct_addrs: Vec::new(),
    })
}

// ── FFI types ─────────────────────────────────────────────────────────────────

/// Flat response-head struct that crosses the FFI boundary.
#[derive(Debug, Clone)]
pub struct FfiResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Handle to a [`BodyReader`] containing the response body.
    pub body_handle: u64,
    /// Full `httpi://` URL of the responding peer.
    pub url: String,
    /// Handle to a trailer receiver.
    pub trailers_handle: u64,
}

/// Options passed to the JS serve callback per incoming request.
#[derive(Debug)]
pub struct RequestPayload {
    pub req_handle: u64,
    pub req_body_handle: u64,
    pub res_body_handle: u64,
    pub req_trailers_handle: u64,
    pub res_trailers_handle: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub remote_node_id: String,
    pub is_bidi: bool,
}

/// Handles for the two sides of a full-duplex QUIC stream.
#[derive(Debug)]
pub struct FfiDuplexStream {
    pub read_handle: u64,
    pub write_handle: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_round_trip() {
        let original: Vec<u8> = (0..32).collect();
        let encoded = base32_encode(&original);
        let decoded = base32_decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn base32_empty() {
        let encoded = base32_encode(&[]);
        assert_eq!(encoded, "");
        let decoded = base32_decode("").unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn base32_decode_invalid_char() {
        let result = base32_decode("!!!invalid!!!");
        assert!(result.is_err());
    }

    #[test]
    fn parse_node_id_invalid_base32() {
        let result = parse_node_id("!!!not-base32!!!");
        assert!(result.is_err());
    }

    #[test]
    fn parse_node_id_wrong_length() {
        let result = parse_node_id("aa");
        assert!(result.is_err());
    }

    #[test]
    fn core_error_display() {
        let e = CoreError::timeout("30s elapsed");
        assert!(e.to_string().contains("Timeout"));
        assert!(e.to_string().contains("30s elapsed"));
    }

}
