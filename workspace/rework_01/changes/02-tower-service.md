# Change 02 — Serve loop via tower::Service and tower::ServiceBuilder

## Risk: Medium — restructures the accept loop, depends on change 01

## Problem

The serve accept loop in `server.rs` is a monolithic 200+ line `async move`
closure with inline:
- `tokio::sync::Semaphore` acquire/release for global concurrency
- `Mutex<HashMap<PublicKey, usize>>` for per-peer connection counting
- `tokio::time::timeout` wrapping per-request handler tasks
- A `consecutive_errors: usize` counter for circuit-breaking
- Shutdown signaling via `tokio::sync::Notify`

The closure cannot be unit-tested in isolation, and any additional
cross-cutting concern (metrics, tracing, rate limiting) requires modifying
this one closure.

After change 01, hyper's `serve_connection` call becomes the request handler.
It naturally wraps a `tower::Service` — this is hyper's intended usage model.

## Solution

Define the per-request handler as a `tower::Service`, then compose concurrency
and timeout layers using `tower::ServiceBuilder`. The per-peer connection
guard and circuit breaker remain custom (no library does per-key limiting or
this specific circuit-breaking pattern) but are extracted into clean RAII
types outside the loop.

### Step 1 — RequestService

```rust
/// The core per-request handler as a Tower Service.
///
/// Wraps the user-supplied `on_request` callback. Receives a hyper
/// `Request<Incoming>`, converts it to `RequestPayload`, stores the
/// response-head sender in the slab (as the `req_handle`), allocates
/// body/trailer channels, and fires the callback.
#[derive(Clone)]
struct RequestService {
    on_request: Arc<dyn Fn(RequestPayload) + Send + Sync>,
    ep_idx: u32,
    #[cfg(feature = "compression")]
    compression: Option<CompressionOptions>,
}

impl Service<Request<Incoming>> for RequestService {
    type Response = Response<BoxBody>;
    type Error = String;
    type Future = /* BoxFuture */;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        // 1. Parse method, path, headers from req
        // 2. Detect is_bidi from Upgrade header
        // 3. Allocate req/res body channels, trailer channels
        // 4. Store response-head sender in slab → req_handle
        // 5. Fire on_request(payload)
        // 6. Return the response future (awaits response-head from slab)
        Box::pin(async move { ... })
    }
}
```

### Step 2 — Composed service in serve()

```rust
let svc = tower::ServiceBuilder::new()
    .concurrency_limit(max_concurrency)      // replaces Semaphore
    .timeout(request_timeout)               // replaces tokio::time::timeout
    .layer(tower_http::trace::TraceLayer::new_for_http())  // optional
    .service(RequestService { ... });
```

The accept loop then becomes:

```rust
loop {
    select! {
        biased;
        _ = shutdown.notified() => break,
        Some(incoming) = ep.accept() => {
            // Per-peer connection guard (see PeerConnectionGuard below)
            let guard = match peer_guard.acquire(remote_id, max_per_peer) {
                None => { incoming.refuse(); continue; }
                Some(g) => g,
            };

            // Circuit breaker
            if consecutive_errors >= max_consecutive_errors {
                warn!("circuit breaker open"); break;
            }

            let svc = svc.clone();
            tokio::spawn(async move {
                let _guard = guard;  // holds peer slot open
                let io = IrohStream::new(send, recv);
                let result = hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        hyper_util::rt::TokioIo::new(io),
                        svc,
                    )
                    .with_upgrades()
                    .await;
                if result.is_err() { /* increment consecutive_errors */ }
                else { consecutive_errors = 0; }
            });
        }
    }
}
```

### PeerConnectionGuard — RAII per-peer slot

```rust
struct PeerConnectionGuard {
    counts: Arc<Mutex<HashMap<PublicKey, usize>>>,
    peer: PublicKey,
}

impl PeerConnectionGuard {
    /// Returns None if the peer is already at capacity.
    fn acquire(
        counts: &Arc<Mutex<HashMap<PublicKey, usize>>>,
        peer: PublicKey,
        max: usize,
    ) -> Option<Self> {
        let mut map = counts.lock().unwrap();
        let count = map.entry(peer).or_insert(0);
        if *count >= max { return None; }
        *count += 1;
        Some(PeerConnectionGuard { counts: counts.clone(), peer })
    }
}

impl Drop for PeerConnectionGuard {
    fn drop(&mut self) {
        let mut map = self.counts.lock().unwrap();
        if let Some(c) = map.get_mut(&self.peer) {
            *c = c.saturating_sub(1);
            if *c == 0 { map.remove(&self.peer); }
        }
    }
}
```

### Graceful drain

Tower's `ConcurrencyLimit` layer holds a permit per active request. For
graceful drain, we can count in-flight requests via a separate
`Arc<AtomicUsize>` counter that the accept loop increments on spawn and
decrements in the spawned task's drop. On shutdown, spin-wait until the
counter reaches zero or drain_timeout expires:

```rust
pub async fn drain(self) {
    let deadline = tokio::time::Instant::now() + self.drain_timeout;
    loop {
        if self.in_flight.load(Ordering::Acquire) == 0 { break; }
        if tokio::time::Instant::now() >= deadline { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
```

Alternatively, use a `tokio::sync::Semaphore` with the full capacity of
`max_concurrency` permits and acquire all of them on drain (the existing
approach). Either is acceptable; the existing approach is already correct.

## Files changed

| File | Change |
|---|---|
| `iroh-http-core/Cargo.toml` | Add `tower = { version = "0.5", features = ["limit", "timeout", "util"] }` |
| `iroh-http-core/src/server.rs` | Add `RequestService`, `PeerConnectionGuard`, restructure `serve()` |
| `iroh-http-core/tests/integration.rs` | Add unit test for `RequestService` without QUIC |

## Validation

```
cargo test -p iroh-http-core
cargo test --test integration --features compression
```

Unit test to add:
```rust
#[tokio::test]
async fn request_service_invokes_callback() {
    let called = Arc::new(AtomicBool::new(false));
    let c = called.clone();
    let svc = RequestService {
        on_request: Arc::new(move |_| { c.store(true, Ordering::SeqCst); }),
        ep_idx: 0,
    };
    // Drive with a mock Request<Incoming> — no QUIC needed
    // Assert called.load(…) is true
}
```
