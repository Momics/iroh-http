//! Body channel types and global handle slab.
//!
//! Rust owns all stream state; JS holds only opaque `u32` handles.
//! Two global slabs are maintained:
//! - `READER_SLAB` — `SlottedReader` handles (JS calls `nextChunk`)
//! - `WRITER_SLAB` — `mpsc::Sender` handles (JS calls `sendChunk` / `finishBody`)
//!
//! A companion map `PENDING_READERS` holds the reader side of newly created
//! writer channels until `rawFetch` (or the serve path) claims them.
//!
//! Each slotted reader uses a `tokio::sync::Mutex` so the `recv` future can
//! be awaited without holding the slab's `std::sync::Mutex`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use bytes::Bytes;
use slab::Slab;
use tokio::sync::mpsc;

const CHANNEL_CAPACITY: usize = 32;

// ── Body channel primitives ───────────────────────────────────────────────────

/// Consumer end — stored in the reader slab.
/// Uses `tokio::sync::Mutex` so we can `.await` the receiver without holding
/// the slab's `std::sync::Mutex`.
pub struct BodyReader {
    pub(crate) rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Bytes>>>,
}

/// Producer end — stored in the writer slab.
/// `mpsc::Sender` is `Clone`, so we clone it out of the slab for each call.
pub struct BodyWriter {
    pub(crate) tx: mpsc::Sender<Bytes>,
}

/// Create a matched (writer, reader) pair backed by a bounded mpsc channel.
pub fn make_body_channel() -> (BodyWriter, BodyReader) {
    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
    (
        BodyWriter { tx },
        BodyReader {
            rx: Arc::new(tokio::sync::Mutex::new(rx)),
        },
    )
}

impl BodyReader {
    /// Receive the next chunk.  Returns `None` when the writer is gone (EOF).
    pub async fn next_chunk(&self) -> Option<Bytes> {
        self.rx.lock().await.recv().await
    }
}

impl BodyWriter {
    /// Send one chunk.  Returns `Err` if the reader has been dropped.
    pub async fn send_chunk(&self, chunk: Bytes) -> Result<(), String> {
        self.tx
            .send(chunk)
            .await
            .map_err(|_| "body reader dropped".to_string())
    }
}

// ── Global slabs ─────────────────────────────────────────────────────────────

fn reader_slab() -> &'static Mutex<Slab<BodyReader>> {
    static S: OnceLock<Mutex<Slab<BodyReader>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

fn writer_slab() -> &'static Mutex<Slab<BodyWriter>> {
    static S: OnceLock<Mutex<Slab<BodyWriter>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

/// Pending reader halves waiting for `rawFetch` to claim them.
fn pending_readers() -> &'static Mutex<HashMap<u32, BodyReader>> {
    static S: OnceLock<Mutex<HashMap<u32, BodyReader>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

// ── Public handle operations ──────────────────────────────────────────────────

/// Insert a `BodyReader` into the global slab and return its handle.
pub fn insert_reader(reader: BodyReader) -> u32 {
    reader_slab().lock().unwrap().insert(reader) as u32
}

/// Insert a `BodyWriter` into the global slab and return its handle.
pub fn insert_writer(writer: BodyWriter) -> u32 {
    writer_slab().lock().unwrap().insert(writer) as u32
}

/// Allocate a `(writer_handle, reader)` pair.
///
/// The writer handle is returned to JS.  The reader must be stored via
/// [`store_pending_reader`] so `rawFetch` can claim it.
pub fn alloc_body_writer() -> (u32, BodyReader) {
    let (writer, reader) = make_body_channel();
    let handle = insert_writer(writer);
    (handle, reader)
}

/// Store the reader side of a newly allocated writer channel.
pub fn store_pending_reader(writer_handle: u32, reader: BodyReader) {
    pending_readers()
        .lock()
        .unwrap()
        .insert(writer_handle, reader);
}

/// Claim the reader that was paired with `writer_handle`.
/// Returns `None` if already claimed or never stored.
pub fn claim_pending_reader(writer_handle: u32) -> Option<BodyReader> {
    pending_readers().lock().unwrap().remove(&writer_handle)
}

// ── Bridge methods (nextChunk / sendChunk / finishBody) ───────────────────────

/// Pull the next chunk from a reader handle.
///
/// Returns `Ok(None)` at EOF.  The handle remains valid until EOF so JS can
/// safely call `nextChunk` again after partial reads.  After returning `None`
/// the handle is cleaned up from the slab automatically.
pub async fn next_chunk(handle: u32) -> Result<Option<Bytes>, String> {
    // Clone the Arc — allows awaiting without holding the slab mutex.
    let rx_arc = {
        let slab = reader_slab().lock().unwrap();
        slab.get(handle as usize)
            .ok_or_else(|| format!("invalid reader handle: {handle}"))?
            .rx
            .clone()
    };

    let chunk = rx_arc.lock().await.recv().await;

    // Clean up on EOF so the slab slot is reused promptly.
    if chunk.is_none() {
        let mut slab = reader_slab().lock().unwrap();
        if slab.contains(handle as usize) {
            slab.remove(handle as usize);
        }
    }

    Ok(chunk)
}

/// Push a chunk into a writer handle.
pub async fn send_chunk(handle: u32, chunk: Bytes) -> Result<(), String> {
    // Clone the Sender (cheap) and release the lock before awaiting.
    let tx = {
        let slab = writer_slab().lock().unwrap();
        slab.get(handle as usize)
            .ok_or_else(|| format!("invalid writer handle: {handle}"))?
            .tx
            .clone()
    };
    tx.send(chunk)
        .await
        .map_err(|_| "body reader dropped".to_string())
}

/// Signal end-of-body by dropping the writer from the slab.
///
/// The associated `BodyReader` will return `None` on its next poll.
pub fn finish_body(handle: u32) -> Result<(), String> {
    let mut slab = writer_slab().lock().unwrap();
    if !slab.contains(handle as usize) {
        return Err(format!("invalid writer handle: {handle}"));
    }
    slab.remove(handle as usize);
    Ok(())
}
