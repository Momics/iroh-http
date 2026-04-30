//! FFI bridge: handles, callback-shaped serve, flat fetch.
//!
//! This module wraps the pure-Rust API in [`crate::http`] with the
//! `u64`-handle plumbing the JS adapters need. It is the **only** part
//! of the crate that knows about `HandleStore`, `BodyReader`/`BodyWriter`
//! handles, the `RequestPayload` callback shape, and the flat
//! `FfiResponse` return shape.
//!
//! Per epic #182 the one-way dependency rule is `ffi → http`, never
//! the reverse. `tests/architecture.rs` enforces this — `crate::http::*`
//! MUST NOT import from `crate::ffi::*`.

pub(crate) mod dispatcher;
pub(crate) mod fetch;
pub(crate) mod handles;
pub(crate) mod pumps;
pub(crate) mod registry;
pub(crate) mod types;
