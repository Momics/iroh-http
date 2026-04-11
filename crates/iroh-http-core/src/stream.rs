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
    let key = reader_slab().lock().unwrap_or_else(|e| e.into_inner()).insert(TimestampedEntry::new(reader));
    u32::try_from(key).expect("reader slab overflow")
}

/// Insert a `BodyWriter` into the global slab and return its handle.
pub fn insert_writer(writer: BodyWriter) -> u32 {
    let key = writer_slab().lock().unwrap_or_else(|e| e.into_inner()).insert(TimestampedEntry::new(writer));
    u32::try_from(key).expect("writer slab overflow")
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
    pending_readers().lock().unwrap_or_else(|e| e.into_inner()).remove(&writer_handle)
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
        let slab = reader_slab().lock().unwrap_or_else(|e| e.into_inner());
        slab.get(handle as usize)
            .ok_or_else(|| format!("invalid reader handle: {handle}"))?
            .inner
            .rx
            .clone()
    };

    let chunk = rx_arc.lock().await.recv().await;

    // Clean up on EOF so the slab slot is reused promptly.
    if chunk.is_none() {
        let mut slab = reader_slab().lock().unwrap_or_else(|e| e.into_inner());
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
        let slab = writer_slab().lock().unwrap_or_else(|e| e.into_inner());
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
    let mut slab = writer_slab().lock().unwrap_or_else(|e| e.into_inner());
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
    let mut slab = reader_slab().lock().unwrap_or_else(|e| e.into_inner());
    if slab.contains(handle as usize) {
        slab.remove(handle as usize);
    }
}

// ── §4 Trailer slabs ──────────────────────────────────────────────────────────

type TrailerTx = tokio::sync::oneshot::Sender<Vec<(String, String)>>;
pub(crate) type TrailerRx = tokio::sync::oneshot::Receiver<Vec<(String, String)>>;

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
    let key = trailer_tx_slab().lock().unwrap_or_else(|e| e.into_inner()).insert(TimestampedEntry::new(tx));
    u32::try_from(key).expect("trailer_tx slab overflow")
}

/// Insert a trailer oneshot **receiver** into the global slab and return its handle.
pub(crate) fn insert_trailer_receiver(rx: TrailerRx) -> u32 {
    let key = trailer_rx_slab().lock().unwrap_or_else(|e| e.into_inner()).insert(TimestampedEntry::new(rx));
    u32::try_from(key).expect("trailer_rx slab overflow")
}

/// Remove (drop) a trailer sender from the slab without sending.
///
/// This causes the corresponding receiver to resolve with `Err`,
/// which `pump_body_to_stream` handles via `unwrap_or_default()`.
pub(crate) fn remove_trailer_sender(handle: u32) {
    let mut slab = trailer_tx_slab().lock().unwrap_or_else(|e| e.into_inner());
    if slab.contains(handle as usize) {
        slab.remove(handle as usize);
    }
}

/// Deliver trailers from the JS side to the waiting Rust pump task.
pub fn send_trailers(handle: u32, trailers: Vec<(String, String)>) -> Result<(), String> {
    let tx = {
        let mut slab = trailer_tx_slab().lock().unwrap_or_else(|e| e.into_inner());
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
        let mut slab = trailer_rx_slab().lock().unwrap_or_else(|e| e.into_inner());
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
    let mut s = reader_slab().lock().unwrap_or_else(|e| e.into_inner());
    let expired: Vec<usize> = s.iter()
        .filter(|(_, e)| e.is_expired(ttl))
        .map(|(k, _)| k)
        .collect();
    if !expired.is_empty() {
        for key in &expired { s.remove(*key); }
        tracing::debug!("[iroh-http] swept {} expired reader entries (ttl={ttl:?})", expired.len());
    }
}

fn sweep_writer_slab(ttl: Duration) {
    let mut s = writer_slab().lock().unwrap_or_else(|e| e.into_inner());
    let expired: Vec<usize> = s.iter()
        .filter(|(_, e)| e.is_expired(ttl))
        .map(|(k, _)| k)
        .collect();
    if !expired.is_empty() {
        for key in &expired { s.remove(*key); }
        tracing::debug!("[iroh-http] swept {} expired writer entries (ttl={ttl:?})", expired.len());
    }
}

fn sweep_trailer_tx_slab(ttl: Duration) {
    let mut s = trailer_tx_slab().lock().unwrap_or_else(|e| e.into_inner());
    let expired: Vec<usize> = s.iter()
        .filter(|(_, e)| e.is_expired(ttl))
        .map(|(k, _)| k)
        .collect();
    if !expired.is_empty() {
        for key in &expired { s.remove(*key); }
        tracing::debug!("[iroh-http] swept {} expired trailer_tx entries (ttl={ttl:?})", expired.len());
    }
}

fn sweep_trailer_rx_slab(ttl: Duration) {
    let mut s = trailer_rx_slab().lock().unwrap_or_else(|e| e.into_inner());
    let expired: Vec<usize> = s.iter()
        .filter(|(_, e)| e.is_expired(ttl))
        .map(|(k, _)| k)
        .collect();
    if !expired.is_empty() {
        for key in &expired { s.remove(*key); }
        tracing::debug!("[iroh-http] swept {} expired trailer_rx entries (ttl={ttl:?})", expired.len());
    }
}

// ── Shared pump helpers ──────────────────────────────────────────────────────

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
    loop {
        match recv.read_chunk(PUMP_READ_BUF).await {
            Ok(Some(chunk)) => {
                let bytes = Bytes::copy_from_slice(&chunk.bytes);
                if writer.send_chunk(bytes).await.is_err() {
                    break;
                }
            }
            _ => break,
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
        assert_eq!(collected, vec![
            Bytes::from("a"), Bytes::from("b"), Bytes::from("c"),
        ]);
    }

    #[tokio::test]
    async fn body_channel_reader_dropped_returns_error() {
        let (writer, reader) = make_body_channel();
        drop(reader);
        let result = writer.send_chunk(Bytes::from("data")).await;
        assert!(result.is_err());
    }

    // ── Slab handle operations ──────────────────────────────────────────

    #[tokio::test]
    async fn insert_reader_and_next_chunk() {
        let (writer, reader) = make_body_channel();
        let handle = insert_reader(reader);

        writer.send_chunk(Bytes::from("slab-data")).await.unwrap();
        drop(writer);

        let chunk = next_chunk(handle).await.unwrap();
        assert_eq!(chunk, Some(Bytes::from("slab-data")));

        // EOF cleans up the slab entry
        let eof = next_chunk(handle).await.unwrap();
        assert!(eof.is_none());
    }

    #[tokio::test]
    async fn next_chunk_invalid_handle() {
        let result = next_chunk(999999).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid reader handle"));
    }

    #[tokio::test]
    async fn send_chunk_via_slab_handle() {
        let (writer, reader) = make_body_channel();
        let handle = insert_writer(writer);

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
        assert!(result.unwrap_err().contains("invalid writer handle"));
    }

    #[test]
    fn finish_body_invalid_handle() {
        let result = finish_body(999999);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid writer handle"));
    }

    #[test]
    fn finish_body_signals_eof() {
        let (writer, _reader) = make_body_channel();
        let handle = insert_writer(writer);
        finish_body(handle).unwrap();
        // Double finish should fail
        let result = finish_body(handle);
        assert!(result.is_err());
    }

    // ── alloc_body_writer / pending reader ──────────────────────────────

    #[test]
    fn alloc_body_writer_and_claim() {
        let (handle, reader) = alloc_body_writer();
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
        let handle = insert_reader(reader);
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
        let tx_handle = insert_trailer_sender(tx);
        let rx_handle = insert_trailer_receiver(rx);

        send_trailers(tx_handle, vec![
            ("x-checksum".into(), "abc".into()),
        ]).unwrap();

        let result = next_trailer(rx_handle).await.unwrap();
        let trailers = result.unwrap();
        assert_eq!(trailers.len(), 1);
        assert_eq!(trailers[0], ("x-checksum".into(), "abc".into()));
    }

    #[test]
    fn send_trailers_invalid_handle() {
        let result = send_trailers(999999, vec![]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid trailer sender handle"));
    }

    #[tokio::test]
    async fn next_trailer_invalid_handle() {
        let result = next_trailer(999999).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid trailer receiver handle"));
    }

    #[tokio::test]
    async fn next_trailer_sender_dropped_returns_none() {
        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
        let rx_handle = insert_trailer_receiver(rx);
        drop(tx); // sender dropped without sending
        let result = next_trailer(rx_handle).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn send_trailers_empty_vec() {
        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<(String, String)>>();
        let tx_handle = insert_trailer_sender(tx);
        let rx_handle = insert_trailer_receiver(rx);

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
