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
    time::{Duration, Instant},
};

use bytes::Bytes;
use slab::Slab;
use tokio::sync::mpsc;

pub const DEFAULT_CHANNEL_CAPACITY: usize = 32;
pub const DEFAULT_MAX_CHUNK_SIZE: usize = 64 * 1024; // 64 KB
pub const DEFAULT_DRAIN_TIMEOUT_MS: u64 = 30_000;     // 30 s
pub const DEFAULT_SLAB_TTL_MS: u64 = 300_000;         // 5 min

// ── Global backpressure config (set at endpoint bind time) ──────────────────

static CHANNEL_CAPACITY: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(DEFAULT_CHANNEL_CAPACITY);
static MAX_CHUNK_SIZE: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(DEFAULT_MAX_CHUNK_SIZE);
static DRAIN_TIMEOUT_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(DEFAULT_DRAIN_TIMEOUT_MS);

/// Configure backpressure parameters.  Call once at endpoint bind time.
/// Subsequent calls update the values for all future channels.
pub fn configure_backpressure(channel_capacity: usize, max_chunk_bytes: usize, drain_timeout_ms: u64) {
    CHANNEL_CAPACITY.store(channel_capacity, std::sync::atomic::Ordering::Relaxed);
    MAX_CHUNK_SIZE.store(max_chunk_bytes, std::sync::atomic::Ordering::Relaxed);
    DRAIN_TIMEOUT_MS.store(drain_timeout_ms, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn drain_timeout() -> Duration {
    Duration::from_millis(DRAIN_TIMEOUT_MS.load(std::sync::atomic::Ordering::Relaxed))
}

// ── Timestamped slab entries ─────────────────────────────────────────────────

pub(crate) struct TimestampedEntry<T> {
    pub inner: T,
    created_at: Instant,
}

impl<T> TimestampedEntry<T> {
    pub(crate) fn new(inner: T) -> Self {
        Self { inner, created_at: Instant::now() }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }
}

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
    let cap = CHANNEL_CAPACITY.load(std::sync::atomic::Ordering::Relaxed);
    let (tx, rx) = mpsc::channel(cap);
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
    /// Send one chunk.  Returns `Err` if the reader has been dropped or if
    /// the drain timeout expires (JS not reading fast enough).
    pub async fn send_chunk(&self, chunk: Bytes) -> Result<(), String> {
        tokio::time::timeout(drain_timeout(), self.tx.send(chunk))
            .await
            .map_err(|_| "drain timeout: body reader is too slow".to_string())?
            .map_err(|_| "body reader dropped".to_string())
    }
}

// ── Global slabs ─────────────────────────────────────────────────────────────

fn reader_slab() -> &'static Mutex<Slab<TimestampedEntry<BodyReader>>> {
    static S: OnceLock<Mutex<Slab<TimestampedEntry<BodyReader>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

fn writer_slab() -> &'static Mutex<Slab<TimestampedEntry<BodyWriter>>> {
    static S: OnceLock<Mutex<Slab<TimestampedEntry<BodyWriter>>>> = OnceLock::new();
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
    reader_slab().lock().unwrap().insert(TimestampedEntry::new(reader)) as u32
}

/// Insert a `BodyWriter` into the global slab and return its handle.
pub fn insert_writer(writer: BodyWriter) -> u32 {
    writer_slab().lock().unwrap().insert(TimestampedEntry::new(writer)) as u32
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
            .inner
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
///
/// Chunks larger than the configured `MAX_CHUNK_SIZE` are split automatically
/// so individual messages stay within the backpressure budget.
pub async fn send_chunk(handle: u32, chunk: Bytes) -> Result<(), String> {
    // Clone the Sender (cheap) and release the lock before awaiting.
    let tx = {
        let slab = writer_slab().lock().unwrap();
        slab.get(handle as usize)
            .ok_or_else(|| format!("invalid writer handle: {handle}"))?
            .inner
            .tx
            .clone()
    };
    let max = MAX_CHUNK_SIZE.load(std::sync::atomic::Ordering::Relaxed);
    if chunk.len() <= max {
        tokio::time::timeout(drain_timeout(), tx.send(chunk))
            .await
            .map_err(|_| "drain timeout: body reader is too slow".to_string())?
            .map_err(|_| "body reader dropped".to_string())
    } else {
        // Split into max-size pieces.
        let mut offset = 0;
        while offset < chunk.len() {
            let end = (offset + max).min(chunk.len());
            tokio::time::timeout(drain_timeout(), tx.send(chunk.slice(offset..end)))
                .await
                .map_err(|_| "drain timeout: body reader is too slow".to_string())?
                .map_err(|_| "body reader dropped".to_string())?;
            offset = end;
        }
        Ok(())
    }
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

// ── §3 AbortSignal — cancel a reader ─────────────────────────────────────────

/// Drop a body reader from the global slab, causing any pending `nextChunk`
/// to return an error and signalling EOF on a cancelled fetch.
pub fn cancel_reader(handle: u32) {
    let mut slab = reader_slab().lock().unwrap();
    if slab.contains(handle as usize) {
        slab.remove(handle as usize);
    }
}

// ── §4 Trailer slabs ──────────────────────────────────────────────────────────

type TrailerTx = tokio::sync::oneshot::Sender<Vec<(String, String)>>;
type TrailerRx = tokio::sync::oneshot::Receiver<Vec<(String, String)>>;

fn trailer_tx_slab() -> &'static Mutex<Slab<TimestampedEntry<TrailerTx>>> {
    static S: OnceLock<Mutex<Slab<TimestampedEntry<TrailerTx>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

fn trailer_rx_slab() -> &'static Mutex<Slab<TimestampedEntry<TrailerRx>>> {
    static S: OnceLock<Mutex<Slab<TimestampedEntry<TrailerRx>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Slab::new()))
}

/// Insert a trailer oneshot **sender** into the global slab and return its handle.
pub(crate) fn insert_trailer_sender(tx: TrailerTx) -> u32 {
    trailer_tx_slab().lock().unwrap().insert(TimestampedEntry::new(tx)) as u32
}

/// Insert a trailer oneshot **receiver** into the global slab and return its handle.
pub(crate) fn insert_trailer_receiver(rx: TrailerRx) -> u32 {
    trailer_rx_slab().lock().unwrap().insert(TimestampedEntry::new(rx)) as u32
}

/// Deliver trailers from the JS side to the waiting Rust pump task.
pub fn send_trailers(handle: u32, trailers: Vec<(String, String)>) -> Result<(), String> {
    let tx = {
        let mut slab = trailer_tx_slab().lock().unwrap();
        if !slab.contains(handle as usize) {
            return Err(format!("invalid trailer sender handle: {handle}"));
        }
        slab.remove(handle as usize).inner
    };
    tx.send(trailers).map_err(|_| "trailer receiver dropped".to_string())
}

/// Await and retrieve trailers produced by the Rust pump task.
pub async fn next_trailer(handle: u32) -> Result<Option<Vec<(String, String)>>, String> {
    let rx = {
        let mut slab = trailer_rx_slab().lock().unwrap();
        if !slab.contains(handle as usize) {
            return Err(format!("invalid trailer receiver handle: {handle}"));
        }
        slab.remove(handle as usize).inner
    };
    match rx.await {
        Ok(trailers) => Ok(Some(trailers)),
        Err(_) => Ok(None), // sender dropped = no trailers
    }
}

// ── Slab TTL sweep ────────────────────────────────────────────────────────────

/// Start a background task that sweeps expired slab entries every 60 seconds.
/// Pass `ttl_ms = 0` to disable sweeping.
pub fn start_slab_sweep(ttl_ms: u64) {
    if ttl_ms == 0 {
        return;
    }
    let ttl = Duration::from_millis(ttl_ms);
    let interval = Duration::from_secs(60);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            sweep_reader_slab(ttl);
            sweep_writer_slab(ttl);
            sweep_trailer_tx_slab(ttl);
            sweep_trailer_rx_slab(ttl);
        }
    });
}

fn sweep_reader_slab(ttl: Duration) {
    let mut s = reader_slab().lock().unwrap();
    let expired: Vec<usize> = s.iter()
        .filter(|(_, e)| e.is_expired(ttl))
        .map(|(k, _)| k)
        .collect();
    if !expired.is_empty() {
        for key in &expired { s.remove(*key); }
        eprintln!("[iroh-http] swept {} expired reader entries (ttl={ttl:?})", expired.len());
    }
}

fn sweep_writer_slab(ttl: Duration) {
    let mut s = writer_slab().lock().unwrap();
    let expired: Vec<usize> = s.iter()
        .filter(|(_, e)| e.is_expired(ttl))
        .map(|(k, _)| k)
        .collect();
    if !expired.is_empty() {
        for key in &expired { s.remove(*key); }
        eprintln!("[iroh-http] swept {} expired writer entries (ttl={ttl:?})", expired.len());
    }
}

fn sweep_trailer_tx_slab(ttl: Duration) {
    let mut s = trailer_tx_slab().lock().unwrap();
    let expired: Vec<usize> = s.iter()
        .filter(|(_, e)| e.is_expired(ttl))
        .map(|(k, _)| k)
        .collect();
    if !expired.is_empty() {
        for key in &expired { s.remove(*key); }
        eprintln!("[iroh-http] swept {} expired trailer_tx entries (ttl={ttl:?})", expired.len());
    }
}

fn sweep_trailer_rx_slab(ttl: Duration) {
    let mut s = trailer_rx_slab().lock().unwrap();
    let expired: Vec<usize> = s.iter()
        .filter(|(_, e)| e.is_expired(ttl))
        .map(|(k, _)| k)
        .collect();
    if !expired.is_empty() {
        for key in &expired { s.remove(*key); }
        eprintln!("[iroh-http] swept {} expired trailer_rx entries (ttl={ttl:?})", expired.len());
    }
}
