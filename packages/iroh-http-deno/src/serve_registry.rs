//! Per-endpoint request queues for the serve polling model.
//!
//! Because Deno FFI cannot receive Rust callbacks, the serve loop pushes each
//! incoming [`RequestPayload`] into an `mpsc` channel.  The TypeScript adapter
//! polls by calling `nextRequest` repeatedly (each call awaits one item).

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use tokio::sync::mpsc;

const QUEUE_CAPACITY: usize = 128;

/// A queued request ready to be delivered to the TypeScript polling loop.
pub type QueuedRequest = serde_json::Value;

/// Sender stored in Rust, used by the serve accept loop.
pub struct ServeQueue {
    pub tx: mpsc::Sender<QueuedRequest>,
    pub rx: tokio::sync::Mutex<mpsc::Receiver<QueuedRequest>>,
}

fn registry() -> &'static Mutex<HashMap<u32, std::sync::Arc<ServeQueue>>> {
    static R: OnceLock<Mutex<HashMap<u32, std::sync::Arc<ServeQueue>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Create and register a serve queue for an endpoint.
/// Returns a clone of the `Arc` so the serve loop can hold its own reference.
pub fn register(endpoint_handle: u32) -> std::sync::Arc<ServeQueue> {
    let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
    let queue = std::sync::Arc::new(ServeQueue {
        tx,
        rx: tokio::sync::Mutex::new(rx),
    });
    registry()
        .lock()
        .unwrap()
        .insert(endpoint_handle, std::sync::Arc::clone(&queue));
    queue
}

/// Retrieve the queue for an endpoint (used by `nextRequest`).
pub fn get(endpoint_handle: u32) -> Option<std::sync::Arc<ServeQueue>> {
    registry()
        .lock()
        .unwrap()
        .get(&endpoint_handle)
        .cloned()
}

/// Remove and drop the queue for an endpoint (called on `closeEndpoint`).
pub fn remove(endpoint_handle: u32) {
    registry().lock().unwrap().remove(&endpoint_handle);
}
