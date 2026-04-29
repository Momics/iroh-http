//! Global endpoint registry shared by all FFI adapters.
//!
//! Centralises the `SlotMap<EndpointKey, IrohEndpoint>` that was previously
//! triplicated across Node, Deno, and Tauri adapters.  Handles are `u64`
//! (via `KeyData::as_ffi`), consistent with stream handles from `slotmap`.
//!
//! Using `SlotMap` instead of `Slab` prevents the ABA handle-reuse problem:
//! each key carries a generation counter, so a stale handle from a closed
//! endpoint will never accidentally resolve to a newly inserted one.
//!
//! ## FFI handle invariant — DO NOT TRUNCATE TO `u32`
//!
//! `KeyData::as_ffi()` packs `(version << 32) | idx` into the returned
//! `u64`.  The high 32 bits carry the generation counter; the low 32 bits
//! are the slot index.  Truncating the handle to `u32` anywhere along the
//! FFI path strips the version bits, defeats the slotmap's anti-ABA
//! guarantee, and re-introduces the stale-handle bugs that motivated the
//! switch from `Slab` (issue #161 was exactly this — a `u32` cast in the
//! Deno dispatch reused slot index 0 across freshly-bound endpoints in
//! consecutive tests).
//!
//! All FFI adapters MUST keep the endpoint handle as `u64` from the JS/TS
//! layer through the `extern "C"` boundary into `registry::get_endpoint`.

use std::sync::{Mutex, OnceLock};

use slotmap::{Key, KeyData, SlotMap};

use crate::endpoint::IrohEndpoint;

slotmap::new_key_type! { struct EndpointKey; }

fn endpoint_map() -> &'static Mutex<SlotMap<EndpointKey, IrohEndpoint>> {
    static S: OnceLock<Mutex<SlotMap<EndpointKey, IrohEndpoint>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(SlotMap::with_key()))
}

fn key_to_handle(k: EndpointKey) -> u64 {
    k.data().as_ffi()
}

fn handle_to_key(h: u64) -> EndpointKey {
    EndpointKey::from(KeyData::from_ffi(h))
}

/// Insert an endpoint into the global registry and return its handle.
pub fn insert_endpoint(ep: IrohEndpoint) -> u64 {
    let key = endpoint_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(ep);
    key_to_handle(key)
}

/// Look up an endpoint by handle (cheap `Arc` clone).
pub fn get_endpoint(handle: u64) -> Option<IrohEndpoint> {
    endpoint_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(handle_to_key(handle))
        .cloned()
}

/// Remove an endpoint from the registry, returning it if it existed.
pub fn remove_endpoint(handle: u64) -> Option<IrohEndpoint> {
    endpoint_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(handle_to_key(handle))
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
        let mut map = endpoint_map().lock().unwrap_or_else(|e| e.into_inner());
        let keys: Vec<EndpointKey> = map.keys().collect();
        keys.into_iter().filter_map(|k| map.remove(k)).collect()
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
