//! Global state managed by the Tauri plugin.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use iroh_http_core::{
    endpoint::IrohEndpoint,
    server::respond,
    stream::{BodyReader, BodyWriter, alloc_body_writer, store_pending_reader, claim_pending_reader},
};
use slab::Slab;

// ── Endpoint slab ─────────────────────────────────────────────────────────────

fn endpoint_slab() -> &'static Mutex<Slab<IrohEndpoint>> {
    static S: OnceLock<Mutex<Slab<IrohEndpoint>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

pub fn insert_endpoint(ep: IrohEndpoint) -> u32 {
    endpoint_slab().lock().unwrap().insert(ep) as u32
}

pub fn get_endpoint(handle: u32) -> Option<IrohEndpoint> {
    endpoint_slab().lock().unwrap().get(handle as usize).cloned()
}

pub fn remove_endpoint(handle: u32) -> Option<IrohEndpoint> {
    let mut slab = endpoint_slab().lock().unwrap();
    if slab.contains(handle as usize) {
        Some(slab.remove(handle as usize))
    } else {
        None
    }
}

// Re-export stream helpers so commands.rs has a single import path.
pub use iroh_http_core::stream::{finish_body, next_chunk, send_chunk};
pub use iroh_http_core::server::respond as respond_request;

/// Allocate a writer handle and stash the paired reader so rawFetch can claim it.
pub fn js_alloc_body_writer() -> u32 {
    let (handle, reader) = alloc_body_writer();
    store_pending_reader(handle, reader);
    handle
}

pub use iroh_http_core::stream::claim_pending_reader;
