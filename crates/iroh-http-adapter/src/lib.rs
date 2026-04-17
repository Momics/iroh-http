//! Shared adapter utilities for iroh-http.
//!
//! Adapters (Deno, Node, Tauri) share a common JSON error convention at the
//! FFI boundary: `{"code":"...","message":"..."}`.  This crate owns that
//! convention so it is defined exactly once.
//!
//! This is intentionally **not** part of `iroh-http-core` — the JSON shape is
//! an adapter-layer concern, not HTTP transport semantics.

use iroh_http_core::{CoreError, ErrorCode};

/// Serialize a [`CoreError`] to the FFI error envelope.
///
/// Produces `{"code":"REFUSED","message":"..."}` (and similar) for each
/// [`ErrorCode`] variant.  Unknown future variants map to `"UNKNOWN"`.
pub fn core_error_to_json(e: &CoreError) -> String {
    let code = match e.code {
        ErrorCode::InvalidInput => "INVALID_INPUT",
        ErrorCode::ConnectionFailed => "REFUSED",
        ErrorCode::Timeout => "TIMEOUT",
        ErrorCode::BodyTooLarge => "BODY_TOO_LARGE",
        ErrorCode::HeaderTooLarge => "HEADER_TOO_LARGE",
        ErrorCode::PeerRejected => "PEER_REJECTED",
        ErrorCode::Cancelled => "CANCELLED",
        ErrorCode::Internal => "INTERNAL",
        _ => "UNKNOWN",
    };
    let json_msg = serde_json::Value::String(e.message.clone());
    format!("{{\"code\":\"{code}\",\"message\":{json_msg}}}")
}

/// Serialize an arbitrary error message to the FFI error envelope with an
/// explicit error code string.
pub fn format_error_json(code: &str, msg: impl std::fmt::Display) -> String {
    let json_msg = serde_json::Value::String(msg.to_string());
    format!("{{\"code\":\"{code}\",\"message\":{json_msg}}}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh_http_core::CoreError;

    #[test]
    fn core_error_to_json_timeout() {
        let e = CoreError::timeout("timed out");
        let json = core_error_to_json(&e);
        assert!(json.contains("\"code\":\"TIMEOUT\""));
        assert!(json.contains("timed out"));
    }

    #[test]
    fn internal_error_maps_to_internal_code() {
        let e = CoreError::internal("something broke");
        let json = core_error_to_json(&e);
        // Internal errors must surface as "INTERNAL", not "UNKNOWN", so that
        // callers can distinguish deliberate internal errors from unrecognised
        // future error codes.
        assert!(json.contains("\"code\":\"INTERNAL\""));
        assert!(json.contains("something broke"));
    }

    #[test]
    fn format_error_json_escapes_message() {
        let json = format_error_json("INVALID_INPUT", "bad \"chars\"");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["code"].as_str().unwrap(), "INVALID_INPUT");
        assert_eq!(v["message"].as_str().unwrap(), "bad \"chars\"");
    }
}
