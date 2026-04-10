---
status: integrated
---

# iroh-http — Patch 07: Stream Hardening

Addresses the remaining streaming reliability gaps: drain timeouts for
stalled body channels, a periodic sweep for leaked slab entries, and a
bounded serve queue for the Deno polling adapter.

> **Context:** Backpressure basics (configurable channel capacity, max chunk
> size) and consecutive error resilience are already integrated. This patch
> covers what those didn't: time-bounded liveness and resource cleanup.

---

## 1. Drain timeout on body channels

### Problem

A body channel backed by `mpsc::channel(N)` will block the producer
indefinitely when the consumer stops reading. In practice:

- A crashed or wedged JS handler never calls `nextChunk` again
- The Rust pump task (`do_request` / `handle_stream`) awaits
  `tx.send(chunk)` forever
- The QUIC stream, the body pump Tokio task, and the channel's buffered
  chunks are all leaked for the lifetime of the endpoint

### Solution

Wrap `tx.send(chunk)` with `tokio::time::timeout` in the body pump tasks.
If the consumer doesn't drain within the deadline, the pump drops the
sender (signalling EOF) and logs a warning.

#### Affected file: `crates/iroh-http-core/src/stream.rs`

Add a global drain timeout alongside the existing backpressure config:

```rust
const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

static DRAIN_TIMEOUT_MS: AtomicU64 = AtomicU64::new(30_000);

pub fn configure_backpressure(
    channel_capacity: usize,
    max_chunk_bytes: usize,
    drain_timeout_ms: u64,
) {
    CHANNEL_CAPACITY.store(channel_capacity, Ordering::Relaxed);
    MAX_CHUNK_SIZE.store(max_chunk_bytes, Ordering::Relaxed);
    DRAIN_TIMEOUT_MS.store(drain_timeout_ms, Ordering::Relaxed);
}

fn drain_timeout() -> Duration {
    Duration::from_millis(DRAIN_TIMEOUT_MS.load(Ordering::Relaxed))
}
```

#### Affected file: `crates/iroh-http-core/src/client.rs` (body pump)

In `pump_body_to_quic` and `pump_quic_to_reader` (the two body transfer
loops), wrap sends:

```rust
match tokio::time::timeout(drain_timeout(), tx.send(chunk)).await {
    Ok(Ok(())) => { /* sent */ }
    Ok(Err(_)) => break,      // reader dropped — normal EOF
    Err(_) => {
        eprintln!("[iroh-http] body drain timeout after {}ms — dropping stream",
            drain_timeout().as_millis());
        break;
    }
}
```

#### Affected file: `crates/iroh-http-core/src/server.rs` (response body pump)

Same pattern in the response body pump task.

#### `NodeOptions` extension

```ts
interface NodeOptions {
  // ... existing ...
  /** Body drain timeout in milliseconds. A stalled stream (consumer not
   *  reading) errors after this duration. Default: 30000 (30s). */
  drainTimeout?: number;
}
```

Threaded through the existing `configure_backpressure` call in each adapter's
`createEndpoint`.

---

## 2. Slab entry TTL sweep

### Problem

The global slabs (`reader_slab`, `writer_slab`, `trailer_tx_slab`,
`trailer_rx_slab`) grow monotonically. Normal completion cleans up entries,
but abnormal paths can leak:

- JS handler throws before calling `finishBody` → writer entry stays forever
- Network disconnect drops the QUIC stream, but the JS-side body reader
  handle is never closed
- Trailer oneshot senders/receivers that are never awaited

Over hours of operation, leaked entries accumulate. The slab indices are
reused, but the `Arc<Mutex<Receiver>>` / `Sender` inside them are not dropped.

### Solution

Add a creation timestamp to each slab entry and run a periodic sweep.

#### Slab wrapper

```rust
struct TimestampedEntry<T> {
    inner: T,
    created_at: Instant,
}
```

Change all four slabs from `Slab<T>` to `Slab<TimestampedEntry<T>>`.
Existing code that reads the inner value uses `.inner` access.

#### Sweep task

```rust
const DEFAULT_SLAB_TTL: Duration = Duration::from_secs(300); // 5 minutes

pub(crate) fn start_slab_sweep(interval: Duration, ttl: Duration) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            let now = Instant::now();
            sweep_slab(&reader_slab(), now, ttl, "reader");
            sweep_slab(&writer_slab(), now, ttl, "writer");
            sweep_slab(&trailer_tx_slab(), now, ttl, "trailer_tx");
            sweep_slab(&trailer_rx_slab(), now, ttl, "trailer_rx");
        }
    });
}

fn sweep_slab<T>(
    slab: &Mutex<Slab<TimestampedEntry<T>>>,
    now: Instant,
    ttl: Duration,
    label: &str,
) {
    let mut s = slab.lock().unwrap();
    let expired: Vec<usize> = s.iter()
        .filter(|(_, e)| now.duration_since(e.created_at) > ttl)
        .map(|(k, _)| k)
        .collect();
    for key in &expired {
        s.remove(*key);
    }
    if !expired.is_empty() {
        eprintln!(
            "[iroh-http] swept {} expired {label} entries (ttl={ttl:?})",
            expired.len()
        );
    }
}
```

The sweep is started once at endpoint creation time, with a default interval
of 60 seconds and a default TTL of 5 minutes. Both are configurable via
`NodeOptions`:

```ts
interface NodeOptions {
  // ... existing ...
  /** Slab entry TTL in milliseconds. Leaked handles are cleaned up after
   *  this duration. Default: 300000 (5m). Set to 0 to disable. */
  handleTtl?: number;
}
```

#### Impact

- Entries that complete normally are cleaned up immediately (no change)
- Leaked entries are collected within `interval + ttl` worst-case
- The sweep locks each slab for the duration of the scan, but slabs are
  small (typically <100 entries) so this is negligible

---

## 3. Bounded Deno serve queue

### Current state

`packages/iroh-http-deno/src/serve_registry.rs` already uses
`mpsc::channel(QUEUE_CAPACITY)` — not unbounded. The `QUEUE_CAPACITY`
constant needs to be verified as reasonable and documented.

### Changes

If `QUEUE_CAPACITY` is already set (e.g. 256), document it. If not, set it:

```rust
/// Maximum pending requests queued for JS. When the queue is full, the
/// accept loop blocks until JS drains a request — providing backpressure
/// to the QUIC accept loop rather than growing memory unboundedly.
const QUEUE_CAPACITY: usize = 256;
```

Log a warning when the queue is full to make backpressure visible:

```rust
if tx.try_send(payload).is_err() {
    eprintln!(
        "[iroh-http-deno] serve queue full ({QUEUE_CAPACITY}), \
         blocking accept loop until JS processes a request"
    );
    tx.send(payload).await.map_err(|_| "serve loop closed")?;
}
```
