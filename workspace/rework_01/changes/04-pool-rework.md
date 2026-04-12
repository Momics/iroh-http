# Change 04 — Connection pool: dashmap + tokio::sync::OnceCell

## Risk: Medium — pool rewrite, subtle concurrency

## Problem

`pool.rs` implements connection-storm prevention (single-flight connect) with
a bespoke `Slot` enum:

```rust
enum Slot {
    Ready(PooledConnection, std::time::Instant),
    Connecting(tokio::sync::watch::Receiver<Option<Result<PooledConnection, String>>>),
}
```

A `Mutex<HashMap<PoolKey, Slot>>` protects the map. When many concurrent
callers want the same peer and no connection exists, one performs the QUIC
handshake while others spin on a `watch::Receiver`.

This requires:
- A three-phase lock-unlock-relock sequence to avoid holding `Mutex` across
  an `await` (QUIC handshake can take hundreds of milliseconds)
- The `wait_for_connection` loop over `watch::Receiver` (which has a "missed
  wake" edge case — currently handled correctly, but fragile)
- The `evict_if_needed` LRU scan that walks the HashMap

`tokio::sync::OnceCell::get_or_try_init` is the exact "run once, all others
wait" primitive. It eliminates the bespoke `Slot` enum entirely.

`dashmap` is a lock-free concurrent HashMap — no `Mutex` is needed for reads,
and insertions are shard-locked rather than globally locked.

## Solution

```rust
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::OnceCell;

pub(crate) struct ConnectionPool {
    map:          DashMap<PoolKey, Arc<OnceCell<PooledConnection>>>,
    last_used:    DashMap<PoolKey, std::time::Instant>,
    idle_timeout: Option<std::time::Duration>,
    max_idle:     Option<usize>,
}
```

### get_or_connect

```rust
pub async fn get_or_connect<F, Fut>(
    &self,
    node_id: iroh::PublicKey,
    alpn: &[u8],
    connect_fn: F,
) -> Result<PooledConnection, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<iroh::endpoint::Connection, String>>,
{
    let key = PoolKey { node_id, alpn: alpn.to_vec() };
    self.evict_stale();

    for _ in 0..2 {  // max 2 iterations for stale-connection retry
        let cell = self.map
            .entry(key.clone())
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();

        let pooled = cell.get_or_try_init(|| async {
            connect_fn().await.map(PooledConnection::new)
        }).await.map_err(|e| e.to_string())?;

        if pooled.conn.close_reason().is_none() {
            self.last_used.insert(key, std::time::Instant::now());
            return Ok(pooled.clone());
        }

        // Connection has closed since it was cached. Remove the stale cell
        // and retry — the next iteration will insert a fresh OnceCell.
        self.map.remove(&key);
    }

    Err("connection closed immediately after connect".into())
}
```

### Stale eviction

```rust
fn evict_stale(&self) {
    // Remove entries whose connection is closed
    self.map.retain(|k, cell| {
        cell.get()
            .map(|c| c.conn.close_reason().is_none())
            .unwrap_or(true)  // still initializing — keep
    });

    // LRU eviction if over capacity
    if let Some(max) = self.max_idle {
        while self.map.len() >= max {
            if let Some(oldest_key) = self
                .last_used
                .iter()
                .min_by_key(|e| *e.value())
                .map(|e| e.key().clone())
            {
                self.map.remove(&oldest_key);
                self.last_used.remove(&oldest_key);
            } else {
                break;
            }
        }
    }
}
```

### Why OnceCell cannot be reset and the retry loop is safe

`tokio::sync::OnceCell` stores its value permanently once set. When a cached
connection closes, we must remove the entire `OnceCell` from the map and
reinsert a fresh one. The `DashMap::entry` API is atomic — the next caller
after removal will insert a new `OnceCell` and be the one to perform the
handshake. At most 2 iterations are needed: the first sees the stale
connection, removes it; the second inserts fresh and connects.

## Files changed

| File | Change |
|---|---|
| `iroh-http-core/Cargo.toml` | Add `dashmap = "6"` |
| `iroh-http-core/src/pool.rs` | Full rewrite (keep public interface: `new`, `get_or_connect`) |

No callers change — `get_or_connect` signature is identical.

## Validation

All existing pool tests must pass:

```
cargo test -p iroh-http-core pool
```

- `pool_reuses_connection` — second call returns same connection
- `pool_evicts_closed` — closed connection is not reused
- `pool_single_flight` — concurrent callers to same peer issue one handshake

Add:
- `pool_retry_on_immediate_close` — connection closes between init and return;
  verify the retry loop reconnects successfully

## Notes

- The `watch` channel dependency is removed from pool.rs.
- The `Slot` enum is removed entirely.
- The `wait_for_connection` function is removed.
- The three-phase lock sequence is gone — `DashMap` handles concurrent access.
- Guidelines doc (`docs/guidelines-rust.md`) describes the pool as
  `DashMap<NodeId, Notify>` — this should be updated to `DashMap<PoolKey,
  Arc<OnceCell<…>>>` once the implementation lands.
