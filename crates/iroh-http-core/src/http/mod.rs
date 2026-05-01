//! Pure-Rust HTTP-over-iroh primitives.
//!
//! No `u64` handles, no JS callbacks, no FFI plumbing. Everything in this
//! module is callable from a pure-Rust application that wants to speak
//! HTTP/1.1 over iroh QUIC streams.
//!
//! Per epic #182 the one-way dependency rule is enforced by
//! `tests/architecture.rs`: code under `crate::http::*` MUST NOT import
//! from `crate::ffi::*`. The FFI bridge wraps this module, never the
//! reverse.

pub(crate) mod body;
pub(crate) mod client;
pub(crate) mod events;
pub(crate) mod server;
pub(crate) mod transport;
