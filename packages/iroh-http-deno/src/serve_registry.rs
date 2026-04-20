//! Per-endpoint request queues for the serve polling model.
//!
//! Because Deno FFI cannot receive Rust callbacks, the serve loop pushes each
//! incoming [`RequestPayload`] into an `mpsc` channel.  The TypeScript adapter
//! polls by calling `nextRequest` repeatedly (each call awaits one item).
//!
//! Connection events (peer connect/disconnect) are similarly queued — the
//! TypeScript adapter polls them via `nextConnectionEvent`.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use tokio::sync::mpsc;

const QUEUE_CAPACITY: usize = 256;

/// A queued request ready to be delivered to the TypeScript polling loop.
pub type QueuedRequest = serde_json::Value;

/// A queued connection event (peer connect / disconnect).
pub type QueuedConnectionEvent = serde_json::Value;

/// Receiver half — held in the registry, polled by `nextRequest` / `nextConnectionEvent`.
pub struct ServeQueue {
    pub tx: mpsc::Sender<QueuedRequest>,
    pub rx: tokio::sync::Mutex<mpsc::Receiver<QueuedRequest>>,
    /// Connection event channel — pushed by the serve loop, polled by `nextConnectionEvent`.
    pub conn_tx: mpsc::Sender<QueuedConnectionEvent>,
    pub conn_rx: tokio::sync::Mutex<mpsc::Receiver<QueuedConnectionEvent>>,
    /// Persistent shutdown signal: `watch::Sender` is cloned into the registry;
    /// `nextRequest` holds a receiver and races `recv()` against this changing to `true`.
    /// Unlike a `Notify`, `watch` persists its last value, so callers that arrive
    /// after `shutdown()` is triggered still see the closed state immediately.
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    pub shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

fn registry() -> &'static Mutex<HashMap<u32, std::sync::Arc<ServeQueue>>> {
    static R: OnceLock<Mutex<HashMap<u32, std::sync::Arc<ServeQueue>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Create and register a serve queue for an endpoint.
/// Returns a clone of the `Arc` so the serve loop can hold its own `tx` reference.
pub fn register(endpoint_handle: u32) -> std::sync::Arc<ServeQueue> {
    let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
    let (conn_tx, conn_rx) = mpsc::channel(QUEUE_CAPACITY);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let queue = std::sync::Arc::new(ServeQueue {
        tx,
        rx: tokio::sync::Mutex::new(rx),
        conn_tx,
        conn_rx: tokio::sync::Mutex::new(conn_rx),
        shutdown_tx,
        shutdown_rx,
    });
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(endpoint_handle, std::sync::Arc::clone(&queue));
    queue
}

/// Retrieve the queue for an endpoint (used by `nextRequest` / `nextConnectionEvent`).
pub fn get(endpoint_handle: u32) -> Option<std::sync::Arc<ServeQueue>> {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&endpoint_handle)
        .cloned()
}

/// Signal shutdown to all pending `nextRequest` callers, then remove the queue.
///
/// ISS-012 / issue-12: sending `true` on the watch channel wakes any currently
/// blocked `recv()` in `nextRequest`, and any future callers will also observe
/// the shutdown state immediately (watch persists its last value).
pub fn remove(endpoint_handle: u32) {
    if let Some(queue) = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&endpoint_handle)
    {
        // Trigger shutdown — this unblocks all pending nextRequest recv() calls.
        let _ = queue.shutdown_tx.send(true);
    }
}
