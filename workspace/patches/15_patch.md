---
status: done
---

# iroh-http — Patch 15: Graceful Shutdown

## Problem

`node.close()` calls `endpoint.close()` which sends `CONNECTION_CLOSE` to
all peers immediately. Any in-flight requests — mid-body-transfer, mid-handler
execution, mid-response-write — are killed instantly. The peer receives an
abrupt connection reset with no indication of what happened.

This causes:

- **Data loss**: A partially-written response body is truncated. The client
  gets an incomplete stream with no error signal beyond the connection drop.
- **Handler abandonment**: A JS handler that's midway through processing
  (e.g. writing to a database) has its response channel dropped.
- **Poor DX for app developers**: No way to "drain" the server before
  shutting down, which is standard practice in every HTTP server framework.

---

## Design

### `node.close()` becomes graceful by default

```ts
// Graceful (default): stop accepting, drain in-flight, then close
await node.close();

// Immediate (opt-in): current behaviour
await node.close({ force: true });

// Custom drain timeout
await node.close({ drainTimeout: 10_000 }); // 10 seconds
```

### Shutdown sequence

1. **Stop accepting**: Close the Iroh endpoint's incoming connection
   listener. No new connections are accepted. No new streams are accepted
   on existing connections.
2. **Drain in-flight**: Wait for all currently-held semaphore permits to be
   released (i.e. all active `handle_stream` tasks complete normally).
3. **Timeout**: If in-flight requests don't finish within `drainTimeout`
   (default: 30 seconds), force-close remaining connections.
4. **Close endpoint**: Call `endpoint.close()` to send `CONNECTION_CLOSE`
   to all remaining peers and release the UDP socket.

### Rust implementation

Add a shutdown signal channel to the serve loop:

```rust
pub struct ServeHandle {
    join: tokio::task::JoinHandle<()>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
}

impl ServeHandle {
    /// Signal the serve loop to stop accepting and drain.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Wait for the serve loop to finish draining.
    pub async fn join(self) {
        let _ = self.join.await;
    }
}
```

The accept loop checks the shutdown signal:

```rust
loop {
    tokio::select! {
        _ = shutdown_rx.changed() => {
            // Stop accepting, wait for in-flight to finish
            break;
        }
        incoming = ep.accept() => {
            // ... existing accept logic ...
        }
    }
}

// After breaking out of accept loop:
// Wait for all semaphore permits to be returned (= all requests done)
let _ = tokio::time::timeout(
    drain_timeout,
    semaphore.acquire_many(max as u32),
).await;
```

The semaphore trick works because each in-flight request holds one permit.
When `acquire_many(max)` succeeds, all permits are free — all requests have
completed.

### `IrohEndpoint.close()` update

```rust
impl IrohEndpoint {
    /// Graceful close: signal serve loop shutdown, drain, then close endpoint.
    pub async fn close(&self, drain_timeout: Duration) {
        // 1. Signal serve loop to stop accepting
        if let Some(handle) = self.serve_handle() {
            handle.shutdown();
        }

        // 2. Wait for drain (with timeout)
        if let Some(handle) = self.take_serve_handle() {
            let _ = tokio::time::timeout(drain_timeout, handle.join()).await;
        }

        // 3. Close the endpoint
        self.inner.ep.close().await;
    }

    /// Immediate close: no drain, current behaviour.
    pub async fn close_force(&self) {
        self.inner.ep.close().await;
    }
}
```

### JS API

```ts
interface CloseOptions {
    /** Force immediate shutdown with no drain period. Default: false. */
    force?: boolean;
    /** Drain timeout in milliseconds. Default: 30000. */
    drainTimeout?: number;
}

interface IrohNode {
    // ... existing members ...
    close(options?: CloseOptions): Promise<void>;
}
```

When `force` is true, call `close_force()` on the Rust side. Otherwise call
`close(drain_timeout)`.

---

## Scope of changes

| Layer | Change |
|---|---|
| `iroh-http-core/src/server.rs` | Return `ServeHandle` instead of `JoinHandle`. Add shutdown signal channel. Add drain-wait logic after accept loop. |
| `iroh-http-core/src/endpoint.rs` | Add `close(drain_timeout)` and `close_force()`. Optionally hold a `ServeHandle`. |
| `iroh-http-node/src/lib.rs` | Pass `CloseOptions` through to Rust `close()` / `close_force()`. |
| `iroh-http-tauri/src/commands.rs` | Same: pass options through. |
| `iroh-http-deno/src/lib.rs` | Same: pass options through. |
| `iroh-http-shared/src/bridge.ts` | Update `close()` signature to accept `CloseOptions`. |
| `iroh-http-shared/src/index.ts` | Export `CloseOptions` type. |

---

## Verification

1. **Drain test**: Start serve, make a request with a 5-second handler delay,
   call `close()` immediately. The request should complete normally and the
   close should resolve after ~5 seconds.
2. **Drain timeout test**: Start serve, make a request that sleeps 60 seconds,
   call `close({ drainTimeout: 2000 })`. The close should resolve after ~2
   seconds, force-closing the stalled request.
3. **Force close test**: `close({ force: true })` should behave identically
   to the current `close()` — immediate shutdown.
4. **No serve test**: A node that only calls `fetch()` (no serve loop) should
   close immediately regardless of options.
5. **`node.closed` promise**: Should still resolve correctly in all three
   cases (graceful drain, timeout, force).
