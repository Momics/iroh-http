//! Global state managed by the Tauri plugin.

use iroh_http_core::{
    endpoint::IrohEndpoint,
    registry,
    stream::{alloc_body_writer, store_pending_reader},
};

// ── Endpoint slab (delegates to core registry) ───────────────────────────────

pub fn insert_endpoint(ep: IrohEndpoint) -> u64 {
    registry::insert_endpoint(ep)
}

pub fn get_endpoint(handle: u64) -> Option<IrohEndpoint> {
    registry::get_endpoint(handle)
}

pub fn remove_endpoint(handle: u64) -> Option<IrohEndpoint> {
    registry::remove_endpoint(handle)
}

// Re-export stream helpers so commands.rs has a single import path.
// (Currently unused — commands.rs uses iroh_http_core::stream directly.)

/// Allocate a writer handle and stash the paired reader so rawFetch can claim it.
pub fn js_alloc_body_writer() -> u64 {
    let (handle, reader) = alloc_body_writer();
    store_pending_reader(handle, reader);
    handle
}
