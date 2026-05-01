//! FfiBridge subsystem — per-endpoint FFI handle store.
//!
//! Kept as a thin wrapper today (one field) so future additions —
//! fetch-cancel slabs, request-head slabs, etc. — have a named home.
//!
//! This module is the definition site of the FFI handle-store reference.
//! The `disallowed_types` lint is explicitly suppressed here; the architecture
//! test in `tests/architecture.rs` enforces that `mod http` never imports
//! `HandleStore`.
#![allow(clippy::disallowed_types)]

use crate::ffi::handles::HandleStore;

/// FFI handle store and any future JS-facing token registries.
pub(in crate::endpoint) struct FfiBridge {
    /// Per-endpoint handle store — owns all body readers, writers,
    /// sessions, request-head channels, and fetch-cancel tokens.
    pub(in crate::endpoint) handles: HandleStore,
}
