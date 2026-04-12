# Change 04 — Connection Pool Strategy (ecosystem-first)

## Risk: Medium — concurrency semantics must remain exact

## Why this change exists

We want to reduce bespoke concurrency code in `pool.rs` while preserving:

1. single-flight connects per `(peer, ALPN)`
2. liveness-aware reuse
3. bounded pool behavior

## What to use from the ecosystem

- `moka::future::Cache` for async, concurrent, bounded caching and eviction
- Keep minimal custom logic only for QUIC-liveness checks and ALPN-specific keying

Rationale: this removes most manual map/lock orchestration while avoiding a
full custom `DashMap + OnceCell + retry` protocol that is easy to get subtly
wrong.

## Proposed shape

```rust
pub(crate) struct ConnectionPool {
    cache: moka::future::Cache<PoolKey, PooledConnection>,
    idle_timeout: Option<Duration>,
}
```

- `PoolKey` remains `(node_id, alpn)`.
- Use `cache.get_with(key, async { ...connect... })` for single-flight init.
- On read, verify `conn.close_reason().is_none()`; if stale, invalidate and retry once.
- Keep explicit metrics/logging around hit/miss/stale-evict to aid debugging.

## Why not pure off-the-shelf only

No cache crate knows QUIC connection liveness semantics for us. We still need
small custom checks (stale connection invalidation), but the heavy concurrency
machinery should be library-owned.

## Files changed

| File | Change |
|---|---|
| `iroh-http-core/Cargo.toml` | Add `moka = { version = "0.12", features = ["future"] }` |
| `iroh-http-core/src/pool.rs` | Replace custom slot/watch logic with cache-backed implementation |

## Validation

Existing tests plus additions:

```bash
cargo test -p iroh-http-core pool
cargo test --test integration --features compression
```

Add/keep:

- `pool_reuses_connection`
- `pool_evicts_closed`
- `pool_single_flight`
- `pool_retry_on_stale_connection`
- `pool_respects_capacity`

## Security/behavior parity gates

- No handshake storm under concurrent callers.
- Closed connections are never reused.
- Pool errors map into stable core error codes.
