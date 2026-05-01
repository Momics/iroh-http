//! FFI-shaped types that cross the boundary between the Rust core and
//! the JS adapters.
//!
//! These are FFI primitives, not HTTP primitives — `mod http` MUST NOT
//! depend on them. Per epic #182 they were extracted from `lib.rs` so
//! the dependency direction is enforceable.
// Definition site — the disallowed_types lint would fire on the definitions
// themselves; allow it here so the lint only catches imports in mod http.
#![allow(clippy::disallowed_types)]

/// Flat response-head struct that crosses the FFI boundary.
///
/// `body_handle` is `0` (the slotmap null sentinel) for null-body status codes
/// (RFC 9110 §6.3: 204, 205, 304).  Adapters should treat `0` as "no body"
/// rather than inspecting the status code themselves.
#[derive(Debug, Clone)]
pub struct FfiResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Handle to a [`crate::ffi::handles::BodyReader`] containing the response body.
    pub body_handle: u64,
    /// Full `httpi://` URL of the responding peer.
    pub url: String,
}

/// Options passed to the JS serve callback per incoming request.
#[derive(Debug)]
pub struct RequestPayload {
    /// Handle to the [`crate::ffi::handles::ResponseHeadEntry`] slot the adapter
    /// must call [`crate::ffi::dispatcher::respond`] with. Source: `HandleStore`.
    /// `0` is the slotmap null sentinel and must never appear here at runtime.
    pub req_handle: u64,
    /// Handle to a [`crate::ffi::handles::BodyReader`] for the *inbound* request
    /// body (source — adapters read from this). `0` = no request body.
    pub req_body_handle: u64,
    /// Handle to a [`crate::ffi::handles::BodyWriter`] for the *outbound* response
    /// body (sink — adapters write into this). `0` = response body already closed.
    pub res_body_handle: u64,
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
