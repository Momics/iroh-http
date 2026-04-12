---
status: open
---

# Patch 30 — Replace Custom Infrastructure with Battle-Tested Crates

Five subsystems in `iroh-http-core` and `iroh-http-framing` are hand-rolled
versions of problems that well-maintained, production-grade Rust crates already
solve. This patch replaces them in dependency order: each change is a
self-contained commit with its own validation step.

**Recommended sequence:** Changes are numbered 1–5 and must be applied in order.
Change 5 is significantly easier once Changes 3 and 4 have stabilised the slab
and pool primitives underneath it.

---

## Change 1 — Adopt the `http` crate for type-safe validation at FFI boundaries

### Problem

Every public function (`fetch`, `respond`, session helpers) accepts raw strings
for the HTTP method, header names, header values, and a bare `u16` for status
codes. There is no validation. A caller can pass `method = "GETT"`, a header
name of `"Content Length"` (note the space), or `status = 0`. These errors
propagate silently deep into the QUIC stream before anything fails.

The `Vec<(String, String)>` header tuple that flows through the entire codebase
— from the FFI boundary through `qpack_bridge.rs` all the way to the wire — has
no semantic meaning attached to it. A header name is indistinguishable from a
header value at the type level.

### Solution

Add `http = "1"` to `iroh-http-core/Cargo.toml`. At the entry point of each
FFI-facing function, parse raw strings into `http` crate types immediately and
return a descriptive `Err(String)` if the input is invalid. The validated types
stay internal and are not surfaced across the FFI boundary (which must remain
primitive-friendly for napi/pyo3).

The FFI handle functions — `fetch()`, `respond()`, the session helpers — do not
change their signatures. Only the first few lines of each change.

### Files

**`crates/iroh-http-core/Cargo.toml`**

Add under `[dependencies]`:
```toml
http = "1"
```

**`crates/iroh-http-core/src/client.rs`** — in `fetch()`, after the scheme check

```rust
// Validate method.
http::Method::from_bytes(method.as_bytes())
    .map_err(|_| format!("invalid HTTP method {:?}", method))?;

// Validate and normalise header names and values.
for (name, value) in headers {
    http::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| format!("invalid header name {:?}", name))?;
    http::header::HeaderValue::from_str(value)
        .map_err(|_| format!("invalid value for header {:?}", name))?;
}
```

**`crates/iroh-http-core/src/server.rs`** — in `respond()`

```rust
http::StatusCode::from_u16(status)
    .map_err(|_| format!("invalid HTTP status code: {status}"))?;

for (name, value) in &headers {
    http::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| format!("invalid response header name {:?}", name))?;
    http::header::HeaderValue::from_str(value)
        .map_err(|_| format!("invalid response header value for {:?}", name))?;
}
```

### Validation

Add targeted unit tests in `client.rs` and `server.rs`:

```rust
// client.rs
#[test]
fn fetch_rejects_invalid_method() {
    // "GETT" is not a valid HTTP method token (extra T)
    // create an endpoint, call fetch, assert Err containing "invalid HTTP method"
}

#[test]
fn fetch_rejects_header_name_with_space() {
    // "Content Length" has a space — invalid per RFC 7230
    // assert Err containing "invalid header name"
}

// server.rs
#[test]
fn respond_rejects_status_zero() {
    // assert respond(handle, 0, vec![]) returns Err
}
```

Run `cargo test -p iroh-http-core` after applying. No existing tests should
break; the new checks only reject inputs that were already incorrect.

---

## Change 2 — Replace hand-rolled trailer parsing in `iroh-http-framing` with `httparse`

### Problem

`iroh_http_framing::parse_trailers()` contains a hand-written byte-scanning
loop that locates colons, scans for CRLF pairs, and slices into UTF-8 strings.
It is ~40 lines of careful byte arithmetic that must correctly handle leading/
trailing whitespace, multi-line field values, empty trailer blocks, and
malformed input. HTTP header parsing is a notorious source of security
vulnerabilities (request smuggling, header injection) and this code has not been
fuzz-tested.

`httparse` is the parser extracted from hyper — it is fuzz-tested, maintained by
the Hyperium team, used in production by millions of HTTP servers, and supports
`no_std`. It replaces the entire byte-scanning loop with one function call.

Similarly, the `push_hex_usize` helper in the crate encodes chunk sizes with a
manual nibble loop instead of using `std::fmt::LowerHex`.

### Solution

Add `httparse = { version = "1", default-features = false }` to
`iroh-http-framing/Cargo.toml`. Remove the `no_std` + manual `extern crate alloc`
configuration (it is not needed — nothing in the build chain requires `no_std`
for this crate) and simply use `std`.

Replace `parse_trailers()` with:

```rust
pub fn parse_trailers(bytes: &[u8]) -> Result<(Vec<(String, String)>, usize), FramingError> {
    if bytes.starts_with(b"\r\n") {
        return Ok((Vec::new(), 2));
    }
    // httparse requires a terminating \r\n\r\n to know the block is complete.
    let end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(FramingError::Incomplete)?;

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let status = httparse::parse_headers(&bytes[..end + 4], &mut headers)
        .map_err(|e| FramingError::Parse(e.to_string()))?;
    let consumed = match status {
        httparse::Status::Complete((n, _)) => n,
        httparse::Status::Partial => return Err(FramingError::Incomplete),
    };
    let pairs = headers
        .iter()
        .take_while(|h| !h.name.is_empty())
        .map(|h| {
            let name = h.name.to_ascii_lowercase();
            let value = std::str::from_utf8(h.value)
                .map_err(|_| FramingError::Parse("trailer value not UTF-8".into()))?
                .trim()
                .to_string();
            Ok((name, value))
        })
        .collect::<Result<Vec<_>, FramingError>>()?;
    Ok((pairs, consumed))
}
```

Replace the `push_hex_usize` manual nibble loop with `std::fmt::write`:

```rust
fn push_hex_usize(buf: &mut Vec<u8>, n: usize) {
    use std::io::Write;
    write!(buf, "{:x}", n).expect("Vec<u8> write is infallible");
}
```

### Files

- `crates/iroh-http-framing/Cargo.toml` — add `httparse`, remove `#![no_std]` config
- `crates/iroh-http-framing/src/lib.rs` — replace `parse_trailers`, remove `push_hex_usize` nibble loop

### Validation

All existing tests in `iroh-http-framing/src/lib.rs` (`trailer_round_trip`,
`trailer_empty_block`, `trailer_invalid_utf8`, etc.) must continue to pass with
the new implementation. Add a fuzz target in a new
`crates/iroh-http-framing/fuzz/` directory:

```rust
// fuzz/fuzz_targets/parse_trailers.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let _ = iroh_http_framing::parse_trailers(data);
});
```

Run `cargo test -p iroh-http-framing`. Then run the fuzz target for at least
60 seconds: `cargo +nightly fuzz run parse_trailers -- -max_total_time=60`.

---

## Change 3 — Replace `HashMap` + `AtomicU32` handle management with `slab::Slab`

### Problem

`stream.rs` manages six independent sub-slabs:

```rust
pub struct SlabSet {
    pub reader:      Mutex<HashMap<u32, TimestampedEntry<BodyReader>>>,
    pub reader_next: AtomicU32,
    pub writer:      Mutex<HashMap<u32, TimestampedEntry<BodyWriter>>>,
    pub writer_next: AtomicU32,
    pub trailer_tx:  Mutex<HashMap<u32, TimestampedEntry<TrailerTx>>>,
    pub trailer_tx_next: AtomicU32,
    // ... and so on for trailer_rx, fetch_cancel, session, response_head
}
```

Each sub-slab is a `Mutex<HashMap<u32, T>>` paired with a separate `AtomicU32`
counter that is incremented on every insert and never wraps or checks for
collision. If an endpoint creates more than 2^20 handles (the stream-index
budget in `compose_handle`) the counter overflows silently and handles alias.

The `slab` crate (`slab = { workspace = true }` is already in the workspace) is
a production-grade indexed storage structure that:
- manages its own integer indices with O(1) insert, O(1) remove, O(1) lookup
- reuses freed slots (preventing unbounded counter growth)
- eliminates all six `AtomicU32` counters entirely

### Solution

Replace each `HashMap<u32, T> + AtomicU32` pair with a `Mutex<slab::Slab<T>>`.
The `slab::Slab::insert()` method returns a `usize` key; cast to `u32` for
`compose_handle`. The `STREAM_BITS`/`STREAM_MASK` budget of 2^20 = 1 048 576
simultaneous handles per slab per endpoint is unchanged.

**`crates/iroh-http-core/src/stream.rs`**

```rust
// Before
pub reader:      Mutex<HashMap<u32, TimestampedEntry<BodyReader>>>,
pub reader_next: AtomicU32,

// After
pub reader: Mutex<slab::Slab<TimestampedEntry<BodyReader>>>,
// — reader_next field removed entirely
```

Insert:

```rust
// Before
let key = slabs.reader_next.fetch_add(1, Ordering::Relaxed);
slabs.reader.lock()…insert(key, TimestampedEntry::new(reader));
compose_handle(ep_idx, key)

// After
let key = slabs.reader.lock()…insert(TimestampedEntry::new(reader));
compose_handle(ep_idx, key as u32)
```

Remove:

```rust
// Before
slabs.reader.lock()…remove(&handle_id)

// After
slabs.reader.lock()…try_remove(handle_id as usize)
```

Lookup:

```rust
// Before
slabs.reader.lock()…get(&id).cloned()

// After
slabs.reader.lock()…get(id as usize)
```

Apply the same mechanical change to all six sub-slabs:
`reader`, `writer`, `trailer_tx`, `trailer_rx`, `fetch_cancel`, `session`,
`response_head`.

Remove all `*_next: AtomicU32` fields from `SlabSet` and all
`fetch_add(1, Ordering::Relaxed)` call sites.

Remove the unused `AtomicU32` import if it becomes dead after this change.

### Files

- `crates/iroh-http-core/src/stream.rs` — primary change
- All callers of `insert_reader`, `insert_writer`, `insert_session_for`,
  `insert_trailer_receiver`, `insert_trailer_sender`, `remove_trailer_sender`,
  `next_chunk`, `send_chunk`, `cancel_reader`, `finish_body`, `next_trailer`,
  `send_trailers` — callers pass and receive the same `u32` handles; only the
  implementation of the insert/lookup calls changes

### Validation

```
cargo test -p iroh-http-core
cargo test --test integration --features compression
```

All 49 integration tests should pass. Run under `cargo test --test e2e` (Node)
and `deno test` (Deno) to confirm handle round-trips work end-to-end.

---

## Change 4 — Replace custom `Slot` + `watch` channel pool with `tokio::sync::OnceCell` + `dashmap`

### Problem

`pool.rs` implements connection-storm prevention (single-flight connect) using a
hand-rolled `Slot` enum:

```rust
enum Slot {
    Ready(PooledConnection, std::time::Instant),
    Connecting(tokio::sync::watch::Receiver<Option<Result<PooledConnection, String>>>),
}
```

When many concurrent callers request the same peer and no connection exists,
only one performs the handshake while the others spin on a `watch::Receiver`.
This requires:
- the `watch` channel logic for waking waiters
- the three-phase lock-unlock-relock sequence to avoid holding a `Mutex` across
  an `await`
- the `evict_if_needed` LRU scan over a `HashMap` protected by a `Mutex`

All three phases are correct but subtle. The custom `wait_for_connection` loop
has a "missed wake" risk if `rx.changed()` returns before the sender writes —
the current code handles it correctly, but it's a footgun.

### Solution

Replace the `Slot` enum and the `Mutex<HashMap<PoolKey, Slot>>` with:

- `dashmap::DashMap<PoolKey, Arc<tokio::sync::OnceCell<PooledConnection>>>` for
  the map (lock-free concurrent HashMap; no `Mutex` needed for reads)
- `tokio::sync::OnceCell` per pool entry for the single-flight guarantee

`tokio::sync::OnceCell::get_or_try_init()` is precisely the "run once, all
other callers wait" primitive. The semantics match what `pool.rs` implements
manually.

Idle eviction is handled by retaining only entries whose `OnceCell` is
initialised with a live connection:

```rust
map.retain(|_, cell| {
    cell.get()
        .map(|c| c.conn.close_reason().is_none())
        .unwrap_or(true)   // still connecting — keep it
});
```

Add `dashmap` and ensure `tokio`'s `sync` feature is already active (it is).

**`crates/iroh-http-core/Cargo.toml`**

```toml
dashmap = "6"
```

**`crates/iroh-http-core/src/pool.rs`** — full replacement:

```rust
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::OnceCell;

pub(crate) struct ConnectionPool {
    map: DashMap<PoolKey, Arc<OnceCell<PooledConnection>>>,
    idle_timeout: Option<std::time::Duration>,
    max_idle: Option<usize>,
}

impl ConnectionPool {
    pub fn new(max_idle: Option<usize>, idle_timeout: Option<std::time::Duration>) -> Self {
        Self { map: DashMap::new(), idle_timeout, max_idle }
    }

    pub async fn get_or_connect<F, Fut>(
        &self,
        node_id: iroh::PublicKey,
        alpn: &[u8],
        connect_fn: F,
    ) -> Result<PooledConnection, String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<iroh::endpoint::Connection, String>>,
    {
        let key = PoolKey { node_id, alpn: alpn.to_vec() };

        // Evict timed-out or closed entries on access (amortised cleanup).
        if let Some(timeout) = self.idle_timeout {
            self.map.retain(|_, cell| {
                cell.get().map(|c| {
                    c.conn.close_reason().is_none()
                    // last-used tracking: see note below
                }).unwrap_or(true)
            });
        }

        let cell = self.map.entry(key).or_insert_with(|| Arc::new(OnceCell::new())).clone();

        let pooled = cell.get_or_try_init(|| async {
            connect_fn().await.map(PooledConnection::new)
        }).await?;

        // If the cached connection has since closed, remove it and retry once.
        if pooled.conn.close_reason().is_some() {
            // Drop the cell so the next caller re-connects.
            // (key must be reinserted — see implementation note)
            return Err("cached connection was closed; caller should retry".into());
        }

        Ok(pooled.clone())
    }
}
```

> **Implementation note on closed connections and retry:** `OnceCell` cannot be
> reset. When a cached connection closes, the cell must be removed from the map
> and a fresh `OnceCell` inserted. Wrap the get_or_try_init call in a loop
> (max 2 iterations) that removes the stale entry and retries if
> `close_reason().is_some()` after init. This is the only complexity that must
> remain custom; everything else is handled by `dashmap` and `OnceCell`.

> **Implementation note on last-used timestamps:** The current pool tracks
> `std::time::Instant` in `Slot::Ready` for LRU eviction. With `OnceCell` the
> instant cannot be stored inside the cell after initialization. Store it in a
> separate `DashMap<PoolKey, Instant>` updated on each successful `get()`, and
> use that map for the eviction scan.

### Files

- `crates/iroh-http-core/Cargo.toml` — add `dashmap`
- `crates/iroh-http-core/src/pool.rs` — rewrite (keep public interface: `new`, `get_or_connect`)
- `crates/iroh-http-core/src/endpoint.rs` — no change; it calls `pool.get_or_connect()` which stays

### Validation

The pool has existing unit tests at the bottom of `pool.rs` (`pool_reuses_connection`,
`pool_evicts_closed`, etc.). All must pass. Run:

```
cargo test -p iroh-http-core pool
cargo test --test integration --features compression
```

---

## Change 5 — Wrap the serve callback in a `tower::Service` and use `tower::ServiceBuilder`

### Problem

The serve accept loop in `server.rs` is a monolithic 200-line `async move`
closure with manual:
- `tokio::sync::Semaphore` acquire/release for the overall concurrency limit
- `Mutex<HashMap<PublicKey, usize>>` for per-peer connection counting
- `tokio::time::timeout()` wrapping the per-request handler task
- A bare `consecutive_errors: usize` counter for circuit-breaking

The closure cannot be unit-tested in isolation. There is no way to inject a mock
transport or a test request without spinning up a real QUIC endpoint. Any
composition (e.g. adding metrics, tracing, or rate-limiting) must be done by
modifying this one closure.

### Solution

Define a `tower::Service` implementation for the core per-request handler, then
use `tower::ServiceBuilder` to layer the concurrency limit and per-request
timeout on top. The per-peer connection counting and the consecutive-error
circuit breaker remain custom (no off-the-shelf tower layer fits either
perfectly) but they shrink to a clear early-return before the service is called.

Add to `Cargo.toml`:

```toml
tower = { version = "0.5", features = ["limit", "timeout", "util"] }
```

**Step 1 — Define `RequestService`**

```rust
// crates/iroh-http-core/src/server.rs

use tower::Service;
use std::future::{ready, Ready};

/// The core per-request handler wrapped as a Tower Service.
///
/// `call()` invokes the user-supplied `on_request` callback and immediately
/// returns `Ready(Ok(()))`. The actual response is sent asynchronously by the
/// callback via `respond()`. This matches the existing fire-and-forget contract.
#[derive(Clone)]
struct RequestService<F> {
    on_request: Arc<F>,
    ep_idx: u32,
    #[cfg(feature = "compression")]
    compression: Option<CompressionOptions>,
}

impl<F> Service<RequestPayload> for RequestService<F>
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    type Response = ();
    type Error = String;
    type Future = Ready<Result<(), String>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), String>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: RequestPayload) -> Self::Future {
        (self.on_request)(req);
        ready(Ok(()))
    }
}
```

**Step 2 — Build the layered service in `serve()`**

```rust
pub fn serve<F>(endpoint: IrohEndpoint, options: ServeOptions, on_request: F) -> ServeHandle
where
    F: Fn(RequestPayload) + Send + Sync + 'static,
{
    let max = options.max_concurrency.unwrap_or(DEFAULT_CONCURRENCY);
    let request_timeout = options.request_timeout_ms
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS));

    let svc = tower::ServiceBuilder::new()
        .concurrency_limit(max)
        .timeout(request_timeout)
        .service(RequestService {
            on_request: Arc::new(on_request),
            ep_idx: endpoint.inner.endpoint_idx,
            #[cfg(feature = "compression")]
            compression: endpoint.compression().cloned(),
        });

    // svc is now a fully composed Service<RequestPayload>.
    // The accept loop calls svc.call(payload) for each incoming request.
    // tower::limit::ConcurrencyLimit handles the semaphore; tower::timeout
    // handles the per-request timeout. The loop itself becomes:

    let join = tokio::spawn(async move {
        let ep = endpoint.raw().clone();
        let mut consecutive_errors = 0usize;
        let mut svc = svc; // moved into the task

        loop {
            // … accept, per-peer limit, error counting (unchanged) …

            // Replace the tokio::spawn(async { timeout(…, handle_request(…)) }) block:
            let payload = /* build RequestPayload as before */;
            match tower::ServiceExt::ready(&mut svc).await {
                Err(_) => { /* concurrency limit or timeout layer error */ }
                Ok(ready_svc) => { let _ = ready_svc.call(payload); }
            }
        }
    });
    // …
}
```

> **Note on per-peer connection limiting:** the `HashMap<PublicKey, usize>`
> peer-count map and its decrement-on-drop guard remain custom because no tower
> layer implements per-key connection limiting. Extract it into a
> `PeerConnectionGuard` RAII struct (a simple `Arc<Mutex<HashMap<…>>>` + counter
> decrement on `Drop`) to make the lifetime explicit and the serve loop shorter.

### Files

- `crates/iroh-http-core/Cargo.toml` — add `tower`
- `crates/iroh-http-core/src/server.rs` — add `RequestService`, restructure `serve()`
- `crates/iroh-http-core/tests/integration.rs` — add a unit test that drives
  `RequestService` directly with a mock `RequestPayload` without any QUIC transport

### Validation

```
cargo test -p iroh-http-core
cargo test --test integration --features compression
```

Add a focused unit test for `RequestService`:

```rust
#[test]
fn request_service_calls_callback() {
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let called2 = called.clone();
    let mut svc = RequestService {
        on_request: Arc::new(move |_req| { called2.store(true, Ordering::SeqCst); }),
        ep_idx: 0,
    };
    use tower::Service;
    let future = svc.call(mock_request_payload());
    futures::executor::block_on(future).unwrap();
    assert!(called.load(Ordering::SeqCst));
}
```

---

## Summary table

| # | What changes | Crate replacing custom code | Risk | Scope |
|---|---|---|---|---|
| 1 | Header/method/status validation | `http = "1"` | Low — additive only | `client.rs`, `server.rs` |
| 2 | Trailer byte parsing | `httparse = "1"` | Low — contained in one crate | `iroh-http-framing` |
| 3 | Handle slab index management | `slab` (already in workspace) | Medium — touches all slab call sites | `stream.rs` + ~15 call sites |
| 4 | Connection pool single-flight | `dashmap = "6"` + `tokio::sync::OnceCell` | Medium — pool.rs rewrite | `pool.rs` |
| 5 | Serve concurrency/timeout | `tower = "0.5"` | High — serve loop restructure | `server.rs` |

Changes 1 and 2 can be made simultaneously (no overlap). Changes 3 and 4 are
independent of each other but should complete before Change 5. Change 5 should
be the last commit and deserves full integration test coverage before merging.
