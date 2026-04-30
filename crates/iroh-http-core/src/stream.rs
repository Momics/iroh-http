//! Per-endpoint handle store and body channel types.
//!
//! Rust owns all stream state; JS holds only opaque `u64` handles.
//! Each `IrohEndpoint` has its own `HandleStore` — no process-global registries.
//! Handles are `u64` values equal to `key.data().as_ffi()`, unique within the
//! owning endpoint's slot-map.

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use bytes::Bytes;
use http_body::Frame;
use slotmap::{KeyData, SlotMap};
use tokio::sync::mpsc;

use crate::CoreError;

// ── Constants ─────────────────────────────────────────────────────────────────

pub const DEFAULT_CHANNEL_CAPACITY: usize = 32;
pub const DEFAULT_MAX_CHUNK_SIZE: usize = 64 * 1024; // 64 KB
pub const DEFAULT_DRAIN_TIMEOUT_MS: u64 = 30_000; // 30 s
pub const DEFAULT_SLAB_TTL_MS: u64 = 300_000; // 5 min
pub const DEFAULT_SWEEP_INTERVAL_MS: u64 = 60_000; // 60 s
pub const DEFAULT_MAX_HANDLES: usize = 65_536;

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
slotmap::new_key_type! { pub(crate) struct FetchCancelKey; }
slotmap::new_key_type! { pub(crate) struct SessionKey; }
slotmap::new_key_type! { pub(crate) struct RequestHeadKey; }

// ── Handle encode / decode helpers ───────────────────────────────────────────

fn key_to_handle<K: slotmap::Key>(k: K) -> u64 {
    k.data().as_ffi()
}

macro_rules! handle_to_key {
    ($fn_name:ident, $key_type:ty) => {
        fn $fn_name(h: u64) -> $key_type {
            <$key_type>::from(KeyData::from_ffi(h))
        }
    };
}

handle_to_key!(handle_to_reader_key, ReaderKey);
handle_to_key!(handle_to_writer_key, WriterKey);
handle_to_key!(handle_to_session_key, SessionKey);
handle_to_key!(handle_to_request_head_key, RequestHeadKey);
handle_to_key!(handle_to_fetch_cancel_key, FetchCancelKey);

// ── Body channel primitives ───────────────────────────────────────────────────

/// Consumer end — stored in the reader registry.
/// Uses `tokio::sync::Mutex` so we can `.await` the receiver without holding
/// the registry's `std::sync::Mutex`.
///
/// Per ADR-014 D4 this type implements [`http_body::Body`] directly so it can
/// be wrapped into [`crate::Body`] without an intermediate `StreamBody`
/// adapter. The two consumer paths are disjoint:
///
/// - **Internal hyper path** — the `Body` impl drives `poll_frame`. The
///   [`BodyReader`] is moved into [`crate::Body::new`] and never registered
///   in the FFI handle store.
/// - **FFI path** — JS calls `next_chunk(handle)` via [`HandleStore`]; the
///   [`Body`](http_body::Body) impl is never polled.
pub struct BodyReader {
    pub(crate) rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Bytes>>>,
    /// ISS-010: cancellation signal — notified when `cancel_reader` is called
    /// so in-flight `next_chunk` awaits terminate promptly.
    pub(crate) cancel: Arc<tokio::sync::Notify>,
    /// In-flight recv future for the [`http_body::Body`] poll path. `None`
    /// when no poll is outstanding. mpsc::recv is cancellation-safe so it is
    /// safe to recreate this future after a `Pending` drop. `Send + Sync`
    /// preserves `BodyReader: Sync` (required by the channel-based pump
    /// helpers that take `&BodyReader` across `.await`).
    pending: Option<Pin<Box<dyn Future<Output = Option<Bytes>> + Send + Sync>>>,
}

/// Producer end — stored in the writer registry.
/// `mpsc::Sender` is `Clone`, so we clone it out of the registry for each call.
pub struct BodyWriter {
    pub(crate) tx: mpsc::Sender<Bytes>,
    /// Drain timeout baked in at channel-creation time from the endpoint config.
    pub(crate) drain_timeout: Duration,
}

/// Create a matched (writer, reader) pair backed by a bounded mpsc channel.
///
/// Prefer [`HandleStore::make_body_channel`] when an endpoint is available so
/// the channel inherits the endpoint's backpressure config.  This free
/// function uses the compile-time defaults and exists for tests and pre-bind
/// code paths.
pub fn make_body_channel() -> (BodyWriter, BodyReader) {
    make_body_channel_with(
        DEFAULT_CHANNEL_CAPACITY,
        Duration::from_millis(DEFAULT_DRAIN_TIMEOUT_MS),
    )
}

fn make_body_channel_with(capacity: usize, drain_timeout: Duration) -> (BodyWriter, BodyReader) {
    let (tx, rx) = mpsc::channel(capacity);
    (
        BodyWriter { tx, drain_timeout },
        BodyReader {
            rx: Arc::new(tokio::sync::Mutex::new(rx)),
            cancel: Arc::new(tokio::sync::Notify::new()),
            pending: None,
        },
    )
}

// ── Cancellable receive ───────────────────────────────────────────────────────

/// Receive the next chunk from a body channel, aborting immediately if
/// `cancel` is notified.
///
/// Returns `None` on EOF (sender dropped) or on cancellation.  Both call
/// sites — [`BodyReader::next_chunk`] and [`HandleStore::next_chunk`] — share
/// this helper so the cancellation semantics are defined and tested in one place.
async fn recv_with_cancel(
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Bytes>>>,
    cancel: Arc<tokio::sync::Notify>,
) -> Option<Bytes> {
    tokio::select! {
        biased;
        _ = cancel.notified() => None,
        chunk = async { rx.lock().await.recv().await } => chunk,
    }
}

impl BodyReader {
    /// Receive the next chunk.  Returns `None` when the writer is gone (EOF)
    /// or when the reader has been cancelled.
    pub async fn next_chunk(&self) -> Option<Bytes> {
        recv_with_cancel(self.rx.clone(), self.cancel.clone()).await
    }
}

/// ADR-014 D4: `BodyReader` is itself an [`http_body::Body`] so callers can
/// wrap it in [`crate::Body::new`] without a `StreamBody`/`unfold` adapter.
impl http_body::Body for BodyReader {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        let fut = this.pending.get_or_insert_with(|| {
            Box::pin(recv_with_cancel(this.rx.clone(), this.cancel.clone()))
        });
        match fut.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(opt) => {
                this.pending = None;
                Poll::Ready(opt.map(|data| Ok(Frame::data(data))))
            }
        }
    }
}

impl BodyWriter {
    /// Send one chunk.  Returns `Err` if the reader has been dropped or if
    /// the drain timeout expires (JS not reading fast enough).
    pub async fn send_chunk(&self, chunk: Bytes) -> Result<(), String> {
        tokio::time::timeout(self.drain_timeout, self.tx.send(chunk))
            .await
            .map_err(|_| "drain timeout: body reader is too slow".to_string())?
            .map_err(|_| "body reader dropped".to_string())
    }
}

// ── StoreConfig ───────────────────────────────────────────────────────────────

/// Configuration for a [`HandleStore`].  Set once at endpoint bind time.
#[derive(Debug, Clone)]
pub struct StoreConfig {
    /// Body-channel capacity (in chunks).  Minimum 1.
    pub channel_capacity: usize,
    /// Maximum byte length of a single chunk in `send_chunk`.  Minimum 1.
    pub max_chunk_size: usize,
    /// Milliseconds to wait for a slow body reader before dropping.
    pub drain_timeout: Duration,
    /// Maximum handle slots per registry.  Prevents unbounded growth.
    pub max_handles: usize,
    /// TTL for handle entries; expired entries are swept periodically.
    /// Zero disables sweeping.
    pub ttl: Duration,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            max_chunk_size: DEFAULT_MAX_CHUNK_SIZE,
            drain_timeout: Duration::from_millis(DEFAULT_DRAIN_TIMEOUT_MS),
            max_handles: DEFAULT_MAX_HANDLES,
            ttl: Duration::from_millis(DEFAULT_SLAB_TTL_MS),
        }
    }
}

// ── Timed wrapper ─────────────────────────────────────────────────────────────

struct Timed<T> {
    value: T,
    /// Updated on every access so that actively-used handles are not TTL-swept
    /// mid-transfer (fix for iroh-http#119 Bug 3).
    last_accessed: Instant,
}

impl<T> Timed<T> {
    fn new(value: T) -> Self {
        Self {
            value,
            last_accessed: Instant::now(),
        }
    }

    /// Refresh the last-access timestamp.  Call inside the registry lock.
    fn touch(&mut self) {
        self.last_accessed = Instant::now();
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.last_accessed.elapsed() > ttl
    }
}

/// Pending reader tracked with insertion time for TTL sweep.
struct PendingReaderEntry {
    reader: BodyReader,
    created: Instant,
}

// ── HandleStore ───────────────────────────────────────────────────────────────

/// Tracks handles inserted during a multi-handle allocation sequence.
/// On drop, removes all tracked handles unless [`commit`](InsertGuard::commit)
/// has been called. This prevents orphaned handles when a later insert fails.
pub(crate) struct InsertGuard<'a> {
    store: &'a HandleStore,
    tracked: Vec<TrackedHandle>,
    committed: bool,
}

/// A handle tracked by [`InsertGuard`] for rollback on drop.
///
/// # Intentionally omitted variants
///
/// `Session` and `FetchCancel` are not tracked here because their lifecycles
/// are managed outside of multi-handle allocation sequences:
/// - Sessions are created and closed by `Session::connect` / `Session::close`
///   independently and are never allocated inside a guard.
/// - Fetch cancel tokens are allocated before a guard is opened and are
///   always cleaned up by `remove_fetch_token` after the fetch resolves.
///
/// If either type is ever added to a guard-guarded allocation path in the
/// future, add `Session(u64)` or `FetchCancel(u64)` variants here with the
/// corresponding rollback arms in [`InsertGuard::drop`].
enum TrackedHandle {
    Reader(u64),
    Writer(u64),
    ReqHead(u64),
}

impl<'a> InsertGuard<'a> {
    fn new(store: &'a HandleStore) -> Self {
        Self {
            store,
            tracked: Vec::new(),
            committed: false,
        }
    }

    pub fn insert_reader(&mut self, reader: BodyReader) -> Result<u64, CoreError> {
        let h = self.store.insert_reader(reader)?;
        self.tracked.push(TrackedHandle::Reader(h));
        Ok(h)
    }

    pub fn insert_writer(&mut self, writer: BodyWriter) -> Result<u64, CoreError> {
        let h = self.store.insert_writer(writer)?;
        self.tracked.push(TrackedHandle::Writer(h));
        Ok(h)
    }

    pub fn allocate_req_handle(
        &mut self,
        sender: tokio::sync::oneshot::Sender<ResponseHeadEntry>,
    ) -> Result<u64, CoreError> {
        let h = self.store.allocate_req_handle(sender)?;
        self.tracked.push(TrackedHandle::ReqHead(h));
        Ok(h)
    }

    /// Consume the guard without rolling back. Call after all inserts succeed.
    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for InsertGuard<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for handle in &self.tracked {
            match handle {
                TrackedHandle::Reader(h) => self.store.cancel_reader(*h),
                TrackedHandle::Writer(h) => {
                    let _ = self.store.finish_body(*h);
                }
                TrackedHandle::ReqHead(h) => {
                    self.store
                        .request_heads
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(handle_to_request_head_key(*h));
                }
            }
        }
    }
}

/// Per-endpoint handle registry.  Owns all body readers, writers,
/// sessions, request-head rendezvous channels, and fetch-cancel tokens for
/// a single `IrohEndpoint`.
///
/// When the endpoint is dropped, this store is dropped with it — all
/// slot-maps are freed and any remaining handles become invalid.
pub struct HandleStore {
    readers: Mutex<SlotMap<ReaderKey, Timed<BodyReader>>>,
    writers: Mutex<SlotMap<WriterKey, Timed<BodyWriter>>>,
    sessions: Mutex<SlotMap<SessionKey, Timed<Arc<SessionEntry>>>>,
    request_heads:
        Mutex<SlotMap<RequestHeadKey, Timed<tokio::sync::oneshot::Sender<ResponseHeadEntry>>>>,
    fetch_cancels: Mutex<SlotMap<FetchCancelKey, Timed<Arc<tokio::sync::Notify>>>>,
    pending_readers: Mutex<HashMap<u64, PendingReaderEntry>>,
    pub(crate) config: StoreConfig,
}

impl HandleStore {
    /// Create a new handle store with the given configuration.
    pub fn new(config: StoreConfig) -> Self {
        Self {
            readers: Mutex::new(SlotMap::with_key()),
            writers: Mutex::new(SlotMap::with_key()),
            sessions: Mutex::new(SlotMap::with_key()),
            request_heads: Mutex::new(SlotMap::with_key()),
            fetch_cancels: Mutex::new(SlotMap::with_key()),
            pending_readers: Mutex::new(HashMap::new()),
            config,
        }
    }

    // ── Config accessors ─────────────────────────────────────────────────

    /// Create a guard for multi-handle allocation with automatic rollback.
    pub(crate) fn insert_guard(&self) -> InsertGuard<'_> {
        InsertGuard::new(self)
    }

    /// The configured drain timeout.
    pub fn drain_timeout(&self) -> Duration {
        self.config.drain_timeout
    }

    /// The configured maximum chunk size.
    pub fn max_chunk_size(&self) -> usize {
        self.config.max_chunk_size
    }

    /// Snapshot of handle counts for observability.
    ///
    /// Returns `(active_readers, active_writers, active_sessions, total_handles)`.
    pub fn count_handles(&self) -> (usize, usize, usize, usize) {
        let readers = self.readers.lock().unwrap_or_else(|e| e.into_inner()).len();
        let writers = self.writers.lock().unwrap_or_else(|e| e.into_inner()).len();
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len();
        let total = readers
            .saturating_add(writers)
            .saturating_add(sessions)
            .saturating_add(
                self.request_heads
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .len(),
            )
            .saturating_add(
                self.fetch_cancels
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .len(),
            );
        (readers, writers, sessions, total)
    }

    // ── Body channels ────────────────────────────────────────────────────

    /// Create a matched (writer, reader) pair using this store's config.
    pub fn make_body_channel(&self) -> (BodyWriter, BodyReader) {
        make_body_channel_with(self.config.channel_capacity, self.config.drain_timeout)
    }

    // ── Capacity-checked insert ──────────────────────────────────────────

    fn insert_checked<K: slotmap::Key, T>(
        registry: &Mutex<SlotMap<K, Timed<T>>>,
        value: T,
        max: usize,
    ) -> Result<u64, CoreError> {
        let mut reg = registry.lock().unwrap_or_else(|e| e.into_inner());
        if reg.len() >= max {
            return Err(CoreError::internal("handle registry at capacity"));
        }
        let key = reg.insert(Timed::new(value));
        Ok(key_to_handle(key))
    }

    // ── Body reader / writer ─────────────────────────────────────────────

    /// Insert a `BodyReader` and return a handle.
    pub fn insert_reader(&self, reader: BodyReader) -> Result<u64, CoreError> {
        Self::insert_checked(&self.readers, reader, self.config.max_handles)
    }

    /// Insert a `BodyWriter` and return a handle.
    pub fn insert_writer(&self, writer: BodyWriter) -> Result<u64, CoreError> {
        Self::insert_checked(&self.writers, writer, self.config.max_handles)
    }

    /// Allocate a `(writer_handle, reader)` pair for streaming request bodies.
    ///
    /// The writer handle is returned to JS.  The reader must be stashed via
    /// [`store_pending_reader`](Self::store_pending_reader) so the fetch path
    /// can claim it.
    pub fn alloc_body_writer(&self) -> Result<(u64, BodyReader), CoreError> {
        let (writer, reader) = self.make_body_channel();
        let handle = self.insert_writer(writer)?;
        Ok((handle, reader))
    }

    /// Store the reader side of a newly allocated writer channel so that the
    /// fetch path can claim it with [`claim_pending_reader`](Self::claim_pending_reader).
    pub fn store_pending_reader(&self, writer_handle: u64, reader: BodyReader) {
        self.pending_readers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                writer_handle,
                PendingReaderEntry {
                    reader,
                    created: Instant::now(),
                },
            );
    }

    /// Claim the reader that was paired with `writer_handle`.
    /// Returns `None` if already claimed or never stored.
    pub fn claim_pending_reader(&self, writer_handle: u64) -> Option<BodyReader> {
        self.pending_readers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&writer_handle)
            .map(|e| e.reader)
    }

    // ── Bridge methods (nextChunk / sendChunk / finishBody) ──────────────

    /// Pull the next chunk from a reader handle.
    ///
    /// Returns `Ok(None)` at EOF.  After returning `None` the handle is
    /// cleaned up from the registry automatically.
    pub async fn next_chunk(&self, handle: u64) -> Result<Option<Bytes>, CoreError> {
        // Clone the Arc — allows awaiting without holding the registry mutex.
        let (rx_arc, cancel) = {
            let mut reg = self.readers.lock().unwrap_or_else(|e| e.into_inner());
            let entry = reg
                .get_mut(handle_to_reader_key(handle))
                .ok_or_else(|| CoreError::invalid_handle(handle))?;
            entry.touch();
            (entry.value.rx.clone(), entry.value.cancel.clone())
        };

        let chunk = recv_with_cancel(rx_arc, cancel).await;

        // Clean up on EOF so the slot is released promptly.
        if chunk.is_none() {
            self.readers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(handle_to_reader_key(handle));
        }

        Ok(chunk)
    }

    /// Non-blocking variant of [`next_chunk`](Self::next_chunk).
    ///
    /// Returns:
    /// - `Ok(Some(bytes))` — a chunk was immediately available.
    /// - `Ok(None)` — EOF; the reader is cleaned up.
    /// - `Err(_)` — no data available yet (channel empty or lock contended),
    ///   or invalid handle. Caller should retry after yielding.
    ///
    /// #126: Used by the Deno adapter to avoid `spawn_blocking` overhead on
    /// the body-read hot path.  When data is already buffered in the channel,
    /// this returns it synchronously on the JS thread.
    pub fn try_next_chunk(&self, handle: u64) -> Result<Option<Bytes>, CoreError> {
        let rx_arc = {
            let mut reg = self.readers.lock().unwrap_or_else(|e| e.into_inner());
            let entry = reg
                .get_mut(handle_to_reader_key(handle))
                .ok_or_else(|| CoreError::invalid_handle(handle))?;
            entry.touch();
            entry.value.rx.clone()
        };

        // Try to acquire the tokio mutex without blocking.
        let mut rx_guard = match rx_arc.try_lock() {
            Ok(g) => g,
            Err(_) => return Err(CoreError::internal("try_next_chunk: lock contended")),
        };

        match rx_guard.try_recv() {
            Ok(chunk) => Ok(Some(chunk)),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                Err(CoreError::internal("try_next_chunk: channel empty"))
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                // EOF — clean up the reader.
                drop(rx_guard);
                self.readers
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(handle_to_reader_key(handle));
                Ok(None)
            }
        }
    }

    /// Push a chunk into a writer handle.
    ///
    /// Chunks larger than the configured `max_chunk_size` are split
    /// automatically so individual messages stay within the backpressure budget.
    pub async fn send_chunk(&self, handle: u64, chunk: Bytes) -> Result<(), CoreError> {
        // Clone the Sender (cheap) and release the lock before awaiting.
        let (tx, timeout) = {
            let mut reg = self.writers.lock().unwrap_or_else(|e| e.into_inner());
            let entry = reg
                .get_mut(handle_to_writer_key(handle))
                .ok_or_else(|| CoreError::invalid_handle(handle))?;
            entry.touch();
            (entry.value.tx.clone(), entry.value.drain_timeout)
        };
        let max = self.config.max_chunk_size;
        if chunk.len() <= max {
            tokio::time::timeout(timeout, tx.send(chunk))
                .await
                .map_err(|_| CoreError::timeout("drain timeout: body reader is too slow"))?
                .map_err(|_| CoreError::internal("body reader dropped"))
        } else {
            // Split into max-size pieces.
            let mut offset = 0;
            while offset < chunk.len() {
                let end = offset.saturating_add(max).min(chunk.len());
                tokio::time::timeout(timeout, tx.send(chunk.slice(offset..end)))
                    .await
                    .map_err(|_| CoreError::timeout("drain timeout: body reader is too slow"))?
                    .map_err(|_| CoreError::internal("body reader dropped"))?;
                offset = end;
            }
            Ok(())
        }
    }

    /// Signal end-of-body by dropping the writer from the registry.
    pub fn finish_body(&self, handle: u64) -> Result<(), CoreError> {
        self.writers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(handle_to_writer_key(handle))
            .ok_or_else(|| CoreError::invalid_handle(handle))?;
        Ok(())
    }

    /// Drop a body reader, signalling cancellation of any in-flight read.
    pub fn cancel_reader(&self, handle: u64) {
        let entry = self
            .readers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(handle_to_reader_key(handle));
        if let Some(e) = entry {
            e.value.cancel.notify_waiters();
        }
    }

    // ── Session ──────────────────────────────────────────────────────────

    /// Insert a `SessionEntry` and return a handle.
    pub fn insert_session(&self, entry: SessionEntry) -> Result<u64, CoreError> {
        Self::insert_checked(&self.sessions, Arc::new(entry), self.config.max_handles)
    }

    /// Look up a session by handle without consuming it.
    pub fn lookup_session(&self, handle: u64) -> Option<Arc<SessionEntry>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(handle_to_session_key(handle))
            .map(|e| e.value.clone())
    }

    /// Remove a session entry by handle and return it.
    pub fn remove_session(&self, handle: u64) -> Option<Arc<SessionEntry>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(handle_to_session_key(handle))
            .map(|e| e.value)
    }

    // ── Request head (for server respond path) ───────────────────────────

    /// Insert a response-head oneshot sender and return a handle.
    pub fn allocate_req_handle(
        &self,
        sender: tokio::sync::oneshot::Sender<ResponseHeadEntry>,
    ) -> Result<u64, CoreError> {
        Self::insert_checked(&self.request_heads, sender, self.config.max_handles)
    }

    /// Remove and return the response-head sender for the given handle.
    pub fn take_req_sender(
        &self,
        handle: u64,
    ) -> Option<tokio::sync::oneshot::Sender<ResponseHeadEntry>> {
        self.request_heads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(handle_to_request_head_key(handle))
            .map(|e| e.value)
    }

    // ── Fetch cancel ─────────────────────────────────────────────────────

    /// Allocate a cancellation token for an upcoming `fetch` call.
    pub fn alloc_fetch_token(&self) -> Result<u64, CoreError> {
        let notify = Arc::new(tokio::sync::Notify::new());
        Self::insert_checked(&self.fetch_cancels, notify, self.config.max_handles)
    }

    /// Signal an in-flight fetch to abort.
    pub fn cancel_in_flight(&self, token: u64) {
        if let Some(entry) = self
            .fetch_cancels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(handle_to_fetch_cancel_key(token))
        {
            entry.value.notify_one();
        }
    }

    /// Retrieve the `Notify` for a fetch token (clones the Arc for use in select!).
    pub fn get_fetch_cancel_notify(&self, token: u64) -> Option<Arc<tokio::sync::Notify>> {
        self.fetch_cancels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(handle_to_fetch_cancel_key(token))
            .map(|e| e.value.clone())
    }

    /// Remove a fetch cancel token after the fetch completes.
    pub fn remove_fetch_token(&self, token: u64) {
        self.fetch_cancels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(handle_to_fetch_cancel_key(token));
    }

    // ── TTL sweep ────────────────────────────────────────────────────────

    /// Sweep all registries, removing entries older than `ttl`.
    /// Also compacts any registry that is empty after sweeping to reclaim
    /// the backing memory from traffic bursts.
    pub fn sweep(&self, ttl: Duration) {
        Self::sweep_readers(&self.readers, ttl);
        Self::sweep_registry(&self.writers, ttl);
        Self::sweep_registry(&self.request_heads, ttl);
        Self::sweep_registry(&self.sessions, ttl);
        Self::sweep_registry(&self.fetch_cancels, ttl);
        self.sweep_pending_readers(ttl);
    }

    /// Sweep expired readers, firing the cancel signal so any in-flight
    /// `next_chunk` awaits terminate promptly instead of hanging.
    fn sweep_readers(registry: &Mutex<SlotMap<ReaderKey, Timed<BodyReader>>>, ttl: Duration) {
        let mut reg = registry.lock().unwrap_or_else(|e| e.into_inner());
        let expired: Vec<ReaderKey> = reg
            .iter()
            .filter(|(_, e)| e.is_expired(ttl))
            .map(|(k, _)| k)
            .collect();

        if expired.is_empty() {
            return;
        }

        for key in &expired {
            if let Some(entry) = reg.remove(*key) {
                entry.value.cancel.notify_waiters();
            }
        }
        tracing::debug!(
            "[iroh-http] swept {} expired reader entries (ttl={ttl:?})",
            expired.len()
        );
        if reg.is_empty() && reg.capacity() > 128 {
            *reg = SlotMap::with_key();
        }
    }

    fn sweep_registry<K: slotmap::Key, T>(registry: &Mutex<SlotMap<K, Timed<T>>>, ttl: Duration) {
        let mut reg = registry.lock().unwrap_or_else(|e| e.into_inner());
        let expired: Vec<K> = reg
            .iter()
            .filter(|(_, e)| e.is_expired(ttl))
            .map(|(k, _)| k)
            .collect();

        if expired.is_empty() {
            return;
        }

        for key in &expired {
            reg.remove(*key);
        }
        tracing::debug!(
            "[iroh-http] swept {} expired registry entries (ttl={ttl:?})",
            expired.len()
        );
        // Compact when empty to reclaim backing memory after traffic bursts.
        if reg.is_empty() && reg.capacity() > 128 {
            *reg = SlotMap::with_key();
        }
    }

    fn sweep_pending_readers(&self, ttl: Duration) {
        let mut map = self
            .pending_readers
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let before = map.len();
        map.retain(|_, e| e.created.elapsed() < ttl);
        let removed = before.saturating_sub(map.len());
        if removed > 0 {
            tracing::debug!("[iroh-http] swept {removed} stale pending readers (ttl={ttl:?})");
        }
    }
}

// ── Shared pump helpers ───────────────────────────────────────────────────────

/// Default read buffer size for QUIC stream reads.
pub(crate) const PUMP_READ_BUF: usize = 64 * 1024;

/// Pump raw bytes from a QUIC `RecvStream` into a `BodyWriter`.
///
/// Reads `PUMP_READ_BUF`-sized chunks and forwards them through the body
/// channel.  Stops when the stream ends or the writer is dropped.
pub(crate) async fn pump_quic_recv_to_body(
    mut recv: iroh::endpoint::RecvStream,
    writer: BodyWriter,
) {
    while let Ok(Some(chunk)) = recv.read_chunk(PUMP_READ_BUF).await {
        if writer.send_chunk(chunk.bytes).await.is_err() {
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

/// Bidirectional pump between a byte-level I/O object and a pair of body channels.
///
/// Reads from `io` → sends to `writer` (incoming data).
/// Reads from `reader` → writes to `io` (outgoing data).
///
/// Used for both client-side and server-side duplex upgrade pumps.
pub(crate) async fn pump_duplex<IO>(io: IO, writer: BodyWriter, reader: BodyReader)
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut recv, mut send) = tokio::io::split(io);

    tokio::join!(
        async {
            use bytes::BytesMut;
            use tokio::io::AsyncReadExt;
            let mut buf = BytesMut::with_capacity(PUMP_READ_BUF);
            loop {
                buf.clear();
                match recv.read_buf(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if writer.send_chunk(buf.split().freeze()).await.is_err() {
                            break;
                        }
                    }
                }
            }
        },
        async {
            use tokio::io::AsyncWriteExt;
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
            let _ = send.shutdown().await;
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> HandleStore {
        HandleStore::new(StoreConfig::default())
    }

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

    // ── HandleStore operations ──────────────────────────────────────────

    #[tokio::test]
    async fn insert_reader_and_next_chunk() {
        let store = test_store();
        let (writer, reader) = store.make_body_channel();
        let handle = store.insert_reader(reader).unwrap();

        writer.send_chunk(Bytes::from("slab-data")).await.unwrap();
        drop(writer);

        let chunk = store.next_chunk(handle).await.unwrap();
        assert_eq!(chunk, Some(Bytes::from("slab-data")));

        // EOF cleans up the registry entry
        let eof = store.next_chunk(handle).await.unwrap();
        assert!(eof.is_none());
    }

    #[tokio::test]
    async fn next_chunk_invalid_handle() {
        let store = test_store();
        let result = store.next_chunk(999999).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::ErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn send_chunk_via_handle() {
        let store = test_store();
        let (writer, reader) = store.make_body_channel();
        let handle = store.insert_writer(writer).unwrap();

        store
            .send_chunk(handle, Bytes::from("via-slab"))
            .await
            .unwrap();
        store.finish_body(handle).unwrap();

        let chunk = reader.next_chunk().await;
        assert_eq!(chunk, Some(Bytes::from("via-slab")));
        let eof = reader.next_chunk().await;
        assert!(eof.is_none());
    }

    #[tokio::test]
    async fn capacity_cap_rejects_overflow() {
        let store = HandleStore::new(StoreConfig {
            max_handles: 2,
            ..StoreConfig::default()
        });
        let (_, r1) = store.make_body_channel();
        let (_, r2) = store.make_body_channel();
        let (_, r3) = store.make_body_channel();
        store.insert_reader(r1).unwrap();
        store.insert_reader(r2).unwrap();
        let err = store.insert_reader(r3).unwrap_err();
        assert!(err.message.contains("capacity"));
    }

    // ── #84 regression: recv_with_cancel cancellation ──────────────────

    #[tokio::test]
    async fn recv_with_cancel_returns_none_on_cancel() {
        let (_tx, rx) = mpsc::channel::<Bytes>(4);
        let rx = Arc::new(tokio::sync::Mutex::new(rx));
        let cancel = Arc::new(tokio::sync::Notify::new());
        // Notify before polling — biased select must return None immediately.
        cancel.notify_one();
        let result = recv_with_cancel(rx, cancel).await;
        assert!(result.is_none());
    }
}
