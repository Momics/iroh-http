//! Global state managed by the Tauri plugin.

use iroh_http_core::{endpoint::IrohEndpoint, registry};

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
