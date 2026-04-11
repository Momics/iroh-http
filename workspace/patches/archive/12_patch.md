---
status: done
---

# iroh-http — Patch 12: Connection Pool & Stream Multiplexing

## Problem

The brief states:

> Connection reuse between streams to the same peer is managed transparently
> by Iroh.

This is **not what the code does**. In `crates/iroh-http-core/src/client.rs`,
every `fetch()` call executes:

```rust
let conn = endpoint.raw().connect(addr, ALPN).await?;
```

This opens a **new QUIC connection** for every request. Iroh's
`Endpoint::connect()` does not automatically reuse existing connections to the
same peer. The result:

1. **Repeated QUIC handshakes** — each fetch pays the full TLS + QUIC setup
   cost, even when talking to a peer it already has a live connection to.
2. **No stream multiplexing on the client side** — the entire point of
   HTTP-over-QUIC is that many requests share one connection via multiplexed
   bidi streams. The current code never does this.
3. **Wasted resources** — each connection holds its own congestion window,
   flow-control state, and keepalive timers. Hundreds of connections to the
   same peer waste memory and UDP sockets.

The **server side is already correct**: `handle_connection()` loops on
`conn.accept_bi()`, so it naturally handles multiplexed streams within a single
connection. Only the client side needs fixing.

---

## Design

### Connection pool in `iroh-http-core`

Add a `ConnectionPool` to `IrohEndpoint` (or as a standalone struct held by
the endpoint). The pool maps `NodeId → Connection` and is checked before
calling `connect()`.

```
fetch(peer, "/api")
  │
  ├─ pool.get(peer) ──► found live Connection ──► conn.open_bi()
  │
  └─ pool.get(peer) ──► miss or stale ──► endpoint.connect(peer)
                                              │
                                              ├─ cache in pool
                                              └─ conn.open_bi()
```

### Key behaviours

| Behaviour | Detail |
|---|---|
| **Lookup** | Before `connect()`, check the pool for an existing connection to the target `NodeId`. |
| **Liveness check** | A cached `Connection` may have been closed by the remote side or timed out. Use Quinn's `Connection::close_reason()` (returns `None` if still open) to detect stale entries. If stale, remove and reconnect. |
| **Insertion** | After a successful `connect()`, insert the new `Connection` into the pool. |
| **Eviction** | Connections are evicted when they go stale (closed/timed out). Optionally, a max-pool-size limit can cap the number of cached connections (default: no limit — Iroh's idle timeout handles cleanup). |
| **Concurrency** | The pool must handle concurrent `fetch()` calls to the same peer. Use a `tokio::sync::Mutex` or a lock-free concurrent map. The critical section is small: lookup + optional insert. |
| **Thread safety** | `Connection` is `Clone` (cheap Arc). Handing out clones is safe. |
| **Idle cleanup** | No background sweeper needed initially. Iroh's QUIC idle timeout naturally closes unused connections. The next `fetch()` finds them stale and reconnects. |
| **ALPN matching** | Pool entries should be keyed on `(NodeId, ALPN)` if different ALPN protos are used for the same peer. For most cases the base `ALPN` is sufficient. |

### Connection storm prevention

When many `fetch()` calls arrive simultaneously for the same peer and no
pooled connection exists, only **one** should call `connect()` while the
others wait. This avoids a "connection storm" of N parallel handshakes.

Use a per-peer `tokio::sync::OnceCell` or a `Notify`-based waiter pattern:

```
fetch(peer, ...) x 10 concurrent
  │
  ├─ first caller: locks slot, calls connect(), caches result, wakes waiters
  └─ callers 2-10: wait on slot, then use the cached Connection
```

### `raw_connect()` (duplex streams)

The existing `raw_connect()` function (used by
`createBidirectionalStream()`) should also go through the pool. A duplex
stream is just `conn.open_bi()` — same as fetch, just without HTTP framing
on top.

### Relationship to Patch 04 (`PeerSession`)

Patch 04 proposes a future `PeerSession` JS object that represents a
connection to a specific peer. The connection pool is the Rust-level
implementation that makes `PeerSession` possible: `PeerSession` would hold
a reference to the pooled `Connection` and call `open_bi()` for each new
stream.

This patch does **not** add the `PeerSession` JS API — it only adds the
Rust-side pool that `fetch()` and `raw_connect()` use internally. The JS
API remains unchanged.

---

## Scope of changes

| Layer | Change |
|---|---|
| `iroh-http-core/src/client.rs` | Replace direct `endpoint.raw().connect()` with pool lookup + fallback connect. Add connection-storm prevention. |
| `iroh-http-core/src/endpoint.rs` | Add `ConnectionPool` field to `IrohEndpoint`. Initialize on `bind()`. Expose `pool()` accessor for internal use. |
| `iroh-http-core/src/pool.rs` (new) | `ConnectionPool` struct: `get()`, `get_or_connect()`, `remove()`. Keyed on `NodeId`. Handles liveness checks and storm prevention. |
| `iroh-http-core/src/lib.rs` | Re-export pool if needed for tests. |
| Bridge / JS layers | **No changes.** The pool is entirely internal to the Rust core. `fetch()` and `createBidirectionalStream()` signatures stay the same. |

---

## Configuration

Add optional fields to `NodeOptions`:

```rust
pub struct NodeOptions {
    // ... existing fields ...

    /// Maximum number of idle connections to keep in the pool.
    /// `None` means no limit (rely on Iroh's idle timeout for cleanup).
    pub max_pooled_connections: Option<usize>,
}
```

No JS API changes — this is a Rust-level tuning knob.

---

## Interoperability

This patch changes **nothing** on the wire. The same HTTP/1.1 framing over
the same QUIC bidi streams. The only difference is that multiple streams now
share a single QUIC connection instead of each getting their own.

The server side already handles this correctly — `handle_connection()` loops
on `accept_bi()` and processes streams concurrently. An ESP or any other peer
running the lighter path will work identically.

---

## Verification

1. **Unit test**: Two sequential `fetch()` calls to the same peer should reuse
   the same connection (assert connection count = 1).
2. **Concurrency test**: 10 parallel `fetch()` calls to the same peer should
   produce exactly 1 `connect()` call (storm prevention).
3. **Stale connection test**: Close a connection remotely, then fetch again —
   should reconnect and cache the new connection.
4. **Cross-peer test**: Fetches to different peers should use separate
   connections.
5. **Benchmark**: Measure latency of 100 sequential small fetches to the same
   peer, before vs. after. Expect significant improvement from eliminating
   repeated handshakes.
