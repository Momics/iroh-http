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
    let mut slab = endpoint_slab().lock().unwrap_or_else(|e| e.into_inner());
    if slab.contains(handle as usize) {
        Some(slab.remove(handle as usize))
    } else {
        None
    }
}

/// Drain the entire registry and force-close every endpoint.
///
/// Called on `WindowEvent::Destroyed` in the Tauri plugin to prevent QUIC
/// socket leaks when the webview hot-reloads without calling `close_endpoint`.
///
/// Removes all entries from the registry synchronously, then drives
/// `close_force` on each in a background thread (safe to call from any
/// context, including synchronous window-event handlers outside a tokio task).
pub fn close_all_endpoints() {
    let endpoints: Vec<IrohEndpoint> = {
        let mut slab = endpoint_slab().lock().unwrap_or_else(|e| e.into_inner());
        let keys: Vec<usize> = slab.iter().map(|(k, _)| k).collect();
        keys.into_iter()
            .filter_map(|k| {
                if slab.contains(k) {
                    Some(slab.remove(k))
                } else {
                    None
                }
            })
            .collect()
    };
    if endpoints.is_empty() {
        return;
    }
    // Spawn a background OS thread with its own single-threaded tokio runtime
    // so that `close_force` (which is async) can be awaited without requiring
    // the caller to be inside an existing tokio context.
    std::thread::spawn(move || {
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            rt.block_on(async move {
                for ep in endpoints {
                    ep.close_force().await;
                }
            });
        }
        // If runtime creation fails, `endpoints` is dropped here — the Arc
        // refcount reaches zero, which still frees the registry entry.
        // The OS reclaims the underlying QUIC sockets on process exit.
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_all_endpoints_is_idempotent_on_empty_registry() {
        // Should not panic when there are no endpoints.
        close_all_endpoints();
    }
}
