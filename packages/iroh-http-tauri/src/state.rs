//! Global state managed by the Tauri plugin.

use std::sync::{Mutex, OnceLock};

use iroh_http_core::{
    endpoint::IrohEndpoint,
    stream::{alloc_body_writer, store_pending_reader},
};
use slab::Slab;

// ── Endpoint slab ─────────────────────────────────────────────────────────────

fn endpoint_slab() -> &'static Mutex<Slab<IrohEndpoint>> {
    static S: OnceLock<Mutex<Slab<IrohEndpoint>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

pub fn insert_endpoint(ep: IrohEndpoint) -> u64 {
    endpoint_slab().lock().unwrap().insert(ep) as u64
}

pub fn get_endpoint(handle: u64) -> Option<IrohEndpoint> {
    endpoint_slab().lock().unwrap().get(handle as usize).cloned()
}

pub fn remove_endpoint(handle: u64) -> Option<IrohEndpoint> {
    let mut slab = endpoint_slab().lock().unwrap();
    if slab.contains(handle as usize) {
        Some(slab.remove(handle as usize))
    } else {
        None
    }
}

// Re-export stream helpers so commands.rs has a single import path.
// (Currently unused — commands.rs uses iroh_http_core::stream directly.)

/// Allocate a writer handle and stash the paired reader so rawFetch can claim it.
pub fn js_alloc_body_writer() -> u64 {
    let (handle, reader) = alloc_body_writer();
    store_pending_reader(handle, reader);
    handle
}
