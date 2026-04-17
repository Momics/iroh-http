//! Global endpoint registry shared by all FFI adapters.
//!
//! Centralises the `Slab<IrohEndpoint>` that was previously triplicated
//! across Node, Deno, and Tauri adapters.  Handles are `u64`, consistent
//! with stream handles from `slotmap`.

use std::sync::{Mutex, OnceLock};

use slab::Slab;

use crate::endpoint::IrohEndpoint;

fn endpoint_slab() -> &'static Mutex<Slab<IrohEndpoint>> {
    static S: OnceLock<Mutex<Slab<IrohEndpoint>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

/// Insert an endpoint into the global registry and return its handle.
pub fn insert_endpoint(ep: IrohEndpoint) -> u64 {
    endpoint_slab()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(ep) as u64
}

/// Look up an endpoint by handle (cheap `Arc` clone).
pub fn get_endpoint(handle: u64) -> Option<IrohEndpoint> {
    endpoint_slab()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(handle as usize)
        .cloned()
}

/// Remove an endpoint from the registry, returning it if it existed.
pub fn remove_endpoint(handle: u64) -> Option<IrohEndpoint> {
    let mut slab = endpoint_slab()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if slab.contains(handle as usize) {
        Some(slab.remove(handle as usize))
    } else {
        None
    }
}
