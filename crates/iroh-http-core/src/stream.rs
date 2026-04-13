//! Body channel types and global handle registries backed by `slotmap`.
//!
//! Rust owns all stream state; JS holds only opaque `u64` handles.
//! One global `Mutex<SlotMap<K, Entry<T>>>` per resource type is maintained.
//! Handles are `u64` values equal to `key.data().as_ffi()` — globally unique
//! across all endpoints without encoding `ep_idx` into the handle bits.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use slotmap::{KeyData, SlotMap};
use tokio::sync::mpsc;

use crate::CoreError;

// ── Constants ─────────────────────────────────────────────────────────────────

pub const DEFAULT_CHANNEL_CAPACITY: usize = 32;
pub const DEFAULT_MAX_CHUNK_SIZE: usize = 64 * 1024; // 64 KB
pub const DEFAULT_DRAIN_TIMEOUT_MS: u64 = 30_000; // 30 s
pub const DEFAULT_SLAB_TTL_MS: u64 = 300_000; // 5 min

// ── Global backpressure config (set at endpoint bind time) ────────────────────
//
// These values are process-global; they are set from the first endpoint that
// calls configure_backpressure() and remain fixed for the lifetime of the
// process.  Subsequent calls are no-ops so that a second endpoint cannot
// silently change channel behaviour for an already-running endpoint.

static CHANNEL_CAPACITY: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(DEFAULT_CHANNEL_CAPACITY);
static MAX_CHUNK_SIZE: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(DEFAULT_MAX_CHUNK_SIZE);
static DRAIN_TIMEOUT_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(DEFAULT_DRAIN_TIMEOUT_MS);

/// Tracks whether backpressure globals have been initialised.
static BACKPRESSURE_CONFIGURED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Configure backpressure parameters.  Only the **first** call takes effect;
/// subsequent calls are silently ignored so that a second endpoint bind does
/// not clobber the config of an already-running endpoint.
pub fn configure_backpressure(
    channel_capacity: usize,
    max_chunk_bytes: usize,
    drain_timeout_ms: u64,
) {
    if BACKPRESSURE_CONFIGURED
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .is_ok()
    {
        CHANNEL_CAPACITY.store(channel_capacity, std::sync::atomic::Ordering::Relaxed);
        MAX_CHUNK_SIZE.store(max_chunk_bytes, std::sync::atomic::Ordering::Relaxed);
        DRAIN_TIMEOUT_MS.store(drain_timeout_ms, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) fn drain_timeout() -> Duration {
    Duration::from_millis(DRAIN_TIMEOUT_MS.load(std::sync::atomic::Ordering::Relaxed))
}

// ── Resource types ────────────────────────────────────────────────────────────

pub struct SessionEntry {
    pub conn: iroh::endpoint::Connection,
}

pub struct ResponseHeadEntry {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

// ── SlotMap key types ─────────────────────────────────────────────────────────

slotmap::new_key_type! { pub(crate) struct ReaderKey; }
slotmap::new_key_type! { pub(crate) struct WriterKey; }
slotmap::new_key_type! { pub(crate) struct TrailerTxKey; }
slotmap::new_key_type! { pub(crate) struct TrailerRxKey; }
slotmap::new_key_type! { pub(crate) struct FetchCancelKey; }
slotmap::new_key_type! { pub(crate) struct SessionKey; }
slotmap::new_key_type! { pub(crate) struct RequestHeadKey; }

// ── Registry entry ────────────────────────────────────────────────────────────

struct Entry<T> {
    ep_idx: u32,
    value: T,
    created_at: Instant,
}

impl<T> Entry<T> {
    fn new(ep_idx: u32, value: T) -> Self {
        Self {
            ep_idx,
            value,
            created_at: Instant::now(),
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }
}

// ── Handle encode / decode helpers ───────────────────────────────────────────

fn key_to_handle<K: slotmap::Key>(k: K) -> u64 {
    k.data().as_ffi()
}

fn handle_to_reader_key(h: u64) -> ReaderKey {
    ReaderKey::from(KeyData::from_ffi(h))
}
fn handle_to_writer_key(h: u64) -> WriterKey {
    WriterKey::from(KeyData::from_ffi(h))
}
fn handle_to_trailer_tx_key(h: u64) -> TrailerTxKey {
    TrailerTxKey::from(KeyData::from_ffi(h))
}
fn handle_to_trailer_rx_key(h: u64) -> TrailerRxKey {
    TrailerRxKey::from(KeyData::from_ffi(h))
}
fn handle_to_session_key(h: u64) -> SessionKey {
    SessionKey::from(KeyData::from_ffi(h))
}
fn handle_to_request_head_key(h: u64) -> RequestHeadKey {
    RequestHeadKey::from(KeyData::from_ffi(h))
}

// ── Body channel primitives ───────────────────────────────────────────────────

/// Consumer end — stored in the reader registry.
/// Uses `tokio::sync::Mutex` so we can `.await` the receiver without holding
/// the registry's `std::sync::Mutex`.
pub struct BodyReader {
    pub(crate) rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Bytes>>>,
}

/// Producer end — stored in the writer registry.
/// `mpsc::Sender` is `Clone`, so we clone it out of the registry for each call.
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

// ── Trailer type aliases ──────────────────────────────────────────────────────

type TrailerTx = tokio::sync::oneshot::Sender<Vec<(String, String)>>;
pub(crate) type TrailerRx = tokio::sync::oneshot::Receiver<Vec<(String, String)>>;

// ── Global registries ─────────────────────────────────────────────────────────

fn reader_registry() -> &'static Mutex<SlotMap<ReaderKey, Entry<BodyReader>>> {
    static R: OnceLock<Mutex<SlotMap<ReaderKey, Entry<BodyReader>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(SlotMap::with_key()))
}

fn writer_registry() -> &'static Mutex<SlotMap<WriterKey, Entry<BodyWriter>>> {
    static R: OnceLock<Mutex<SlotMap<WriterKey, Entry<BodyWriter>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(SlotMap::with_key()))
}

fn fetch_cancel_registry(
) -> &'static Mutex<SlotMap<FetchCancelKey, Entry<Arc<tokio::sync::Notify>>>> {
    static R: OnceLock<Mutex<SlotMap<FetchCancelKey, Entry<Arc<tokio::sync::Notify>>>>> =
        OnceLock::new();
    R.get_or_init(|| Mutex::new(SlotMap::with_key()))
}

fn handle_to_fetch_cancel_key(h: u64) -> FetchCancelKey {
    FetchCancelKey::from(KeyData::from_ffi(h))
}

/// Allocate a cancellation token (as a u64 handle) for an upcoming `fetch` call.
pub fn alloc_fetch_token(ep_idx: u32) -> u64 {
    let notify = Arc::new(tokio::sync::Notify::new());
    let key = fetch_cancel_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(Entry::new(ep_idx, notify));
    key_to_handle(key)
}

/// Signal an in-flight fetch to abort.
pub fn cancel_in_flight(token: u64) {
    if let Some(entry) = fetch_cancel_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(handle_to_fetch_cancel_key(token))
    {
        entry.value.notify_one();
    }
}

/// Retrieve the `Notify` for a fetch token (clones the Arc for use in select!).
pub(crate) fn get_fetch_cancel_notify(token: u64) -> Option<Arc<tokio::sync::Notify>> {
    fetch_cancel_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(handle_to_fetch_cancel_key(token))
        .map(|e| e.value.clone())
}

/// Remove a fetch cancel token after the fetch completes.
pub(crate) fn remove_fetch_token(token: u64) {
    fetch_cancel_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(handle_to_fetch_cancel_key(token));
}

fn pending_readers_map() -> &'static Mutex<HashMap<u64, BodyReader>> {
    static R: OnceLock<Mutex<HashMap<u64, BodyReader>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

fn trailer_tx_registry() -> &'static Mutex<SlotMap<TrailerTxKey, Entry<TrailerTx>>> {
    static R: OnceLock<Mutex<SlotMap<TrailerTxKey, Entry<TrailerTx>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(SlotMap::with_key()))
}

fn trailer_rx_registry() -> &'static Mutex<SlotMap<TrailerRxKey, Entry<TrailerRx>>> {
    static R: OnceLock<Mutex<SlotMap<TrailerRxKey, Entry<TrailerRx>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(SlotMap::with_key()))
}

fn session_registry() -> &'static Mutex<SlotMap<SessionKey, Entry<Arc<SessionEntry>>>> {
    static R: OnceLock<Mutex<SlotMap<SessionKey, Entry<Arc<SessionEntry>>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(SlotMap::with_key()))
}

fn request_head_registry(
) -> &'static Mutex<SlotMap<RequestHeadKey, Entry<tokio::sync::oneshot::Sender<ResponseHeadEntry>>>>
{
    static R: OnceLock<
        Mutex<SlotMap<RequestHeadKey, Entry<tokio::sync::oneshot::Sender<ResponseHeadEntry>>>>,
    > = OnceLock::new();
    R.get_or_init(|| Mutex::new(SlotMap::with_key()))
}

// ── Endpoint lifecycle ────────────────────────────────────────────────────────

static NEXT_EP_IDX_COUNTER: AtomicU32 = AtomicU32::new(1);
/// Number of live endpoints. Used to stop the sweep task when the last one closes.
static ACTIVE_ENDPOINT_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn alloc_endpoint_idx() -> u32 {
    NEXT_EP_IDX_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Increment the active endpoint count. Called when an endpoint is bound.
pub fn register_endpoint(_ep_idx: u32) {
    ACTIVE_ENDPOINT_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Remove all registry entries that belong to this endpoint.
/// When the last endpoint closes, the sweep task is also stopped.
pub fn unregister_endpoint(ep_idx: u32) {
    reader_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|_, e| e.ep_idx != ep_idx);
    writer_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|_, e| e.ep_idx != ep_idx);
    trailer_tx_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|_, e| e.ep_idx != ep_idx);
    trailer_rx_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|_, e| e.ep_idx != ep_idx);
    session_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|_, e| e.ep_idx != ep_idx);
    request_head_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|_, e| e.ep_idx != ep_idx);
    fetch_cancel_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|_, e| e.ep_idx != ep_idx);

    // Stop the sweep task when the last endpoint closes.
    let prev = ACTIVE_ENDPOINT_COUNT.fetch_sub(1, Ordering::Relaxed);
    if prev == 1 {
        stop_slab_sweep();
    }
}

// ── Body reader / writer ──────────────────────────────────────────────────────

/// Insert a `BodyReader` into the global registry and return a `u64` handle.
pub(crate) fn insert_reader(ep_idx: u32, reader: BodyReader) -> u64 {
    let key = reader_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(Entry::new(ep_idx, reader));
    key_to_handle(key)
}

/// Insert a `BodyWriter` into the global registry and return a `u64` handle.
pub(crate) fn insert_writer(ep_idx: u32, writer: BodyWriter) -> u64 {
    let key = writer_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(Entry::new(ep_idx, writer));
    key_to_handle(key)
}

/// Allocate a `(writer_handle, reader)` pair using the global (ep_idx=0) pool.
///
/// The writer handle is returned to JS.  The reader must be stored via
/// [`store_pending_reader`] so `rawFetch` can claim it.
pub fn alloc_body_writer() -> (u64, BodyReader) {
    let (writer, reader) = make_body_channel();
    let handle = insert_writer(0, writer);
    (handle, reader)
}

/// Store the reader side of a newly allocated writer channel so that the fetch
/// path can claim it with [`claim_pending_reader`].
pub fn store_pending_reader(writer_handle: u64, reader: BodyReader) {
    pending_readers_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(writer_handle, reader);
}

/// Claim the reader that was paired with `writer_handle`.
/// Returns `None` if already claimed or never stored.
pub fn claim_pending_reader(writer_handle: u64) -> Option<BodyReader> {
    pending_readers_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&writer_handle)
}

// ── Bridge methods (nextChunk / sendChunk / finishBody) ───────────────────────

/// Pull the next chunk from a reader handle.
///
/// Returns `Ok(None)` at EOF.  The handle remains valid until EOF so JS can
/// safely call `nextChunk` again after partial reads.  After returning `None`
/// the handle is cleaned up from the registry automatically.
pub async fn next_chunk(handle: u64) -> Result<Option<Bytes>, CoreError> {
    // Clone the Arc — allows awaiting without holding the registry mutex.
    let rx_arc = {
        let reg = reader_registry().lock().unwrap_or_else(|e| e.into_inner());
        reg.get(handle_to_reader_key(handle))
            .ok_or_else(|| CoreError::invalid_handle(handle as u32))?
            .value
            .rx
            .clone()
    };

    let chunk = rx_arc.lock().await.recv().await;

    // Clean up on EOF so the slot is released promptly.
    if chunk.is_none() {
        reader_registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(handle_to_reader_key(handle));
    }

    Ok(chunk)
}

/// Push a chunk into a writer handle.
///
/// Chunks larger than the configured `MAX_CHUNK_SIZE` are split automatically
/// so individual messages stay within the backpressure budget.
pub async fn send_chunk(handle: u64, chunk: Bytes) -> Result<(), CoreError> {
    // Clone the Sender (cheap) and release the lock before awaiting.
    let tx = {
        let reg = writer_registry().lock().unwrap_or_else(|e| e.into_inner());
        reg.get(handle_to_writer_key(handle))
            .ok_or_else(|| CoreError::invalid_handle(handle as u32))?
            .value
            .tx
            .clone()
    };
    let max = MAX_CHUNK_SIZE.load(std::sync::atomic::Ordering::Relaxed);
    if chunk.len() <= max {
        tokio::time::timeout(drain_timeout(), tx.send(chunk))
            .await
            .map_err(|_| CoreError::timeout("drain timeout: body reader is too slow"))?
            .map_err(|_| CoreError::internal("body reader dropped"))
    } else {
        // Split into max-size pieces.
        let mut offset = 0;
        while offset < chunk.len() {
            let end = (offset + max).min(chunk.len());
            tokio::time::timeout(drain_timeout(), tx.send(chunk.slice(offset..end)))
                .await
                .map_err(|_| CoreError::timeout("drain timeout: body reader is too slow"))?
                .map_err(|_| CoreError::internal("body reader dropped"))?;
            offset = end;
        }
        Ok(())
    }
}

/// Signal end-of-body by dropping the writer from the registry.
///
/// The associated `BodyReader` will return `None` on its next poll.
pub fn finish_body(handle: u64) -> Result<(), CoreError> {
    writer_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(handle_to_writer_key(handle))
        .ok_or_else(|| CoreError::invalid_handle(handle as u32))?;
    Ok(())
}

/// Drop a body reader from the global registry, causing any pending `nextChunk`
/// to return an error and signalling EOF on a cancelled fetch.
pub fn cancel_reader(handle: u64) {
    reader_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(handle_to_reader_key(handle));
}

// ── Trailer operations ────────────────────────────────────────────────────────

/// Insert a trailer oneshot **sender** into the global registry and return a `u64` handle.
pub(crate) fn insert_trailer_sender(ep_idx: u32, tx: TrailerTx) -> u64 {
    let key = trailer_tx_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(Entry::new(ep_idx, tx));
    key_to_handle(key)
}

/// Insert a trailer oneshot **receiver** into the global registry and return a `u64` handle.
pub(crate) fn insert_trailer_receiver(ep_idx: u32, rx: TrailerRx) -> u64 {
    let key = trailer_rx_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(Entry::new(ep_idx, rx));
    key_to_handle(key)
}

/// Remove (drop) a trailer sender from the registry without sending.
///
/// This causes the corresponding receiver to resolve with `Err`,
/// which `pump_body_to_stream` handles via `unwrap_or_default()`.
pub(crate) fn remove_trailer_sender(handle: u64) {
    trailer_tx_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(handle_to_trailer_tx_key(handle));
}

/// Deliver trailers from the JS side to the waiting Rust pump task.
pub fn send_trailers(handle: u64, trailers: Vec<(String, String)>) -> Result<(), CoreError> {
    let tx = trailer_tx_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(handle_to_trailer_tx_key(handle))
        .ok_or_else(|| CoreError::invalid_handle(handle as u32))?
        .value;
    tx.send(trailers)
        .map_err(|_| CoreError::internal("trailer receiver dropped"))
}

/// Await and retrieve trailers produced by the Rust pump task.
pub async fn next_trailer(handle: u64) -> Result<Option<Vec<(String, String)>>, CoreError> {
    let rx = trailer_rx_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(handle_to_trailer_rx_key(handle))
        .ok_or_else(|| CoreError::invalid_handle(handle as u32))?
        .value;
    match rx.await {
        Ok(trailers) => Ok(Some(trailers)),
        Err(_) => Ok(None), // sender dropped = no trailers
    }
}

// ── Session ───────────────────────────────────────────────────────────────────

/// Insert a `SessionEntry` into the global registry and return a `u64` handle.
pub(crate) fn insert_session_for(ep_idx: u32, entry: SessionEntry) -> u64 {
    let key = session_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(Entry::new(ep_idx, Arc::new(entry)));
    key_to_handle(key)
}

/// Look up a session by handle without consuming it.
pub(crate) fn lookup_session(handle: u64) -> Option<Arc<SessionEntry>> {
    session_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(handle_to_session_key(handle))
        .map(|e| e.value.clone())
}

/// Remove a session entry by handle and return it.
pub(crate) fn remove_session(handle: u64) -> Option<Arc<SessionEntry>> {
    session_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(handle_to_session_key(handle))
        .map(|e| e.value)
}

/// Return the endpoint index for a session handle.
pub(crate) fn session_ep_idx(handle: u64) -> Option<u32> {
    session_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(handle_to_session_key(handle))
        .map(|e| e.ep_idx)
}

// ── Request head (for server respond path) ────────────────────────────────────

/// Insert a response-head oneshot sender into the global registry and return a `u64` handle.
pub(crate) fn allocate_req_handle(
    ep_idx: u32,
    sender: tokio::sync::oneshot::Sender<ResponseHeadEntry>,
) -> u64 {
    let key = request_head_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(Entry::new(ep_idx, sender));
    key_to_handle(key)
}

/// Remove and return the response-head sender for the given handle.
pub(crate) fn take_req_sender(
    handle: u64,
) -> Option<tokio::sync::oneshot::Sender<ResponseHeadEntry>> {
    request_head_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(handle_to_request_head_key(handle))
        .map(|e| e.value)
}

// ── TTL sweep ─────────────────────────────────────────────────────────────────

/// Ensures at most one sweep task is running process-wide.
static SWEEP_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Shutdown signal for the sweep task.  Initialised when the task starts.
static SWEEP_SHUTDOWN: OnceLock<Arc<tokio::sync::Notify>> = OnceLock::new();

/// Start a background task that sweeps expired registry entries every 60 seconds.
/// Pass `ttl_ms = 0` to disable sweeping.
///
/// Only the first call starts the sweep task; subsequent calls are no-ops so
/// that multiple endpoint binds do not accumulate duplicate sweepers.
pub fn start_slab_sweep(ttl_ms: u64) {
    if ttl_ms == 0 {
        return;
    }
    if SWEEP_STARTED
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .is_err()
    {
        return; // already running
    }
    let shutdown = Arc::new(tokio::sync::Notify::new());
    // Store the notify so stop_slab_sweep() can signal it.  If two callers race
    // on the very first start, one will win the compare_exchange above and the
    // other will have returned early, so set() here is guaranteed to succeed.
    let _ = SWEEP_SHUTDOWN.set(shutdown.clone());

    let ttl = Duration::from_millis(ttl_ms);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = ticker.tick() => sweep_all(ttl),
                _ = shutdown.notified() => {
                    tracing::debug!("[iroh-http] slab sweep task stopped");
                    break;
                }
            }
        }
    });
}

/// Signal the sweep task to stop.  Safe to call even if no sweep is running.
///
/// After this returns, `start_slab_sweep` may be called again to restart.
pub fn stop_slab_sweep() {
    if let Some(notify) = SWEEP_SHUTDOWN.get() {
        notify.notify_one();
    }
    // Reset so the next endpoint bind can restart the sweep.
    SWEEP_STARTED.store(false, std::sync::atomic::Ordering::Release);
}

fn sweep_all(ttl: Duration) {
    sweep_registry(
        &mut *reader_registry().lock().unwrap_or_else(|e| e.into_inner()),
        ttl,
    );
    sweep_registry(
        &mut *writer_registry().lock().unwrap_or_else(|e| e.into_inner()),
        ttl,
    );
    sweep_registry(
        &mut *trailer_tx_registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
        ttl,
    );
    sweep_registry(
        &mut *trailer_rx_registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
        ttl,
    );
}

fn sweep_registry<K: slotmap::Key, T>(registry: &mut SlotMap<K, Entry<T>>, ttl: Duration) {
    let before = registry.len();
    registry.retain(|_, e| !e.is_expired(ttl));
    let removed = before - registry.len();
    if removed > 0 {
        tracing::debug!("[iroh-http] swept {removed} expired registry entries (ttl={ttl:?})");
    }
}

// ── Shared pump helpers ───────────────────────────────────────────────────────

/// Default read buffer size for QUIC stream reads.
const PUMP_READ_BUF: usize = 64 * 1024;

/// Pump raw bytes from a QUIC `RecvStream` into a `BodyWriter`.
///
/// Reads `PUMP_READ_BUF`-sized chunks and forwards them through the body
/// channel.  Stops when the stream ends or the writer is dropped.
pub(crate) async fn pump_quic_recv_to_body(
    mut recv: iroh::endpoint::RecvStream,
    writer: BodyWriter,
) {
    while let Ok(Some(chunk)) = recv.read_chunk(PUMP_READ_BUF).await {
        let bytes = Bytes::copy_from_slice(&chunk.bytes);
        if writer.send_chunk(bytes).await.is_err() {
            break;
        }
    }
    // writer drops → BodyReader sees EOF.
}

/// Pump raw bytes from a `BodyReader` into a QUIC `SendStream`.
///
/// Reads chunks from the body channel and writes them to the stream.
/// Finishes the stream when the reader reaches EOF.
pub(crate) async fn pump_body_to_quic_send(
    reader: BodyReader,
    mut send: iroh::endpoint::SendStream,
) {
    loop {
        match reader.next_chunk().await {
            None => break,
            Some(data) => {
                if send.write_all(&data).await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = send.finish();
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Body channel basics ─────────────────────────────────────────────

    #[tokio::test]
    async fn body_channel_send_recv() {
        let (writer, reader) = make_body_channel();
        writer.send_chunk(Bytes::from("hello")).await.unwrap();
        drop(writer); // signal EOF
        let chunk = reader.next_chunk().await;
        assert_eq!(chunk, Some(Bytes::from("hello")));
        let eof = reader.next_chunk().await;
        assert!(eof.is_none());
    }

    #[tokio::test]
    async fn body_channel_multiple_chunks() {
        let (writer, reader) = make_body_channel();
        writer.send_chunk(Bytes::from("a")).await.unwrap();
        writer.send_chunk(Bytes::from("b")).await.unwrap();
        writer.send_chunk(Bytes::from("c")).await.unwrap();
        drop(writer);

        let mut collected = Vec::new();
        while let Some(chunk) = reader.next_chunk().await {
            collected.push(chunk);
        }
        assert_eq!(
            collected,
            vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c"),]
        );
    }

    #[tokio::test]
    async fn body_channel_reader_dropped_returns_error() {
        let (writer, reader) = make_body_channel();
        drop(reader);
        let result = writer.send_chunk(Bytes::from("data")).await;
        assert!(result.is_err());
    }

    // ── Registry handle operations ──────────────────────────────────────

    #[tokio::test]
    async fn insert_reader_and_next_chunk() {
        let (writer, reader) = make_body_channel();
        let handle: u64 = insert_reader(0, reader);

        writer.send_chunk(Bytes::from("slab-data")).await.unwrap();
        drop(writer);

        let chunk = next_chunk(handle).await.unwrap();
        assert_eq!(chunk, Some(Bytes::from("slab-data")));

        // EOF cleans up the registry entry
        let eof = next_chunk(handle).await.unwrap();
        assert!(eof.is_none());
    }

    #[tokio::test]
    async fn next_chunk_invalid_handle() {
        let result = next_chunk(999999).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::ErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn send_chunk_via_slab_handle() {
        let (writer, reader) = make_body_channel();
        let handle: u64 = insert_writer(0, writer);

        send_chunk(handle, Bytes::from("via-slab")).await.unwrap();
        finish_body(handle).unwrap();

        let chunk = reader.next_chunk().await;
        assert_eq!(chunk, Some(Bytes::from("via-slab")));
        let eof = reader.next_chunk().await;
        assert!(eof.is_none());
    }

    #[tokio::test]
    async fn send_chunk_invalid_handle() {
        let result = send_chunk(999999, Bytes::from("nope")).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::ErrorCode::InvalidInput);
    }

    #[test]
    fn finish_body_invalid_handle() {
        let result = finish_body(999999);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::ErrorCode::InvalidInput);
    }

    #[test]
    fn finish_body_signals_eof() {
        let (writer, _reader) = make_body_channel();
        let handle: u64 = insert_writer(0, writer);
        finish_body(handle).unwrap();
        // Double finish should fail
        let result = finish_body(handle);
        assert!(result.is_err());
    }

    // ── alloc_body_writer / pending reader ──────────────────────────────

    #[test]
    fn alloc_body_writer_and_claim() {
        let (handle, reader): (u64, BodyReader) = alloc_body_writer();
        store_pending_reader(handle, reader);
        let claimed = claim_pending_reader(handle);
        assert!(claimed.is_some());
        // Second claim returns None
        let again = claim_pending_reader(handle);
        assert!(again.is_none());
    }

    // ── cancel_reader ───────────────────────────────────────────────────

    #[tokio::test]
    async fn cancel_reader_drops_from_slab() {
        let (_writer, reader) = make_body_channel();
        let handle: u64 = insert_reader(0, reader);
        cancel_reader(handle);
        // Subsequent next_chunk should fail (handle invalid)
        let result = next_chunk(handle).await;
        assert!(result.is_err());
    }

    #[test]
    fn cancel_reader_nonexistent_is_noop() {
        // Should not panic
        cancel_reader(999999);
    }

    // ── Trailer operations ──────────────────────────────────────────────

    #[tokio::test]
    async fn trailers_send_and_receive() {
        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
        let tx_handle: u64 = insert_trailer_sender(0, tx);
        let rx_handle: u64 = insert_trailer_receiver(0, rx);

        send_trailers(tx_handle, vec![("x-checksum".into(), "abc".into())]).unwrap();

        let result = next_trailer(rx_handle).await.unwrap();
        let trailers = result.unwrap();
        assert_eq!(trailers.len(), 1);
        assert_eq!(trailers[0], ("x-checksum".into(), "abc".into()));
    }

    #[test]
    fn send_trailers_invalid_handle() {
        let result = send_trailers(999999, vec![]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::ErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn next_trailer_invalid_handle() {
        let result = next_trailer(999999).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::ErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn next_trailer_sender_dropped_returns_none() {
        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
        let rx_handle: u64 = insert_trailer_receiver(0, rx);
        drop(tx); // sender dropped without sending
        let result = next_trailer(rx_handle).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn send_trailers_empty_vec() {
        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
        let tx_handle: u64 = insert_trailer_sender(0, tx);
        let rx_handle: u64 = insert_trailer_receiver(0, rx);

        send_trailers(tx_handle, vec![]).unwrap();
        let result = next_trailer(rx_handle).await.unwrap();
        let trailers = result.unwrap();
        assert!(trailers.is_empty());
    }

    // ── configure_backpressure ──────────────────────────────────────────

    #[test]
    fn configure_backpressure_updates_atomics() {
        configure_backpressure(64, 128 * 1024, 60_000);
        assert_eq!(
            CHANNEL_CAPACITY.load(std::sync::atomic::Ordering::Relaxed),
            64
        );
        assert_eq!(
            MAX_CHUNK_SIZE.load(std::sync::atomic::Ordering::Relaxed),
            128 * 1024
        );
        assert_eq!(
            DRAIN_TIMEOUT_MS.load(std::sync::atomic::Ordering::Relaxed),
            60_000
        );
        // Reset to defaults to avoid affecting other tests
        configure_backpressure(
            DEFAULT_CHANNEL_CAPACITY,
            DEFAULT_MAX_CHUNK_SIZE,
            DEFAULT_DRAIN_TIMEOUT_MS,
        );
    }
}
