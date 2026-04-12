# Connection Pool

iroh-http-core maintains a pool of QUIC connections so that multiple `fetch()` calls to the same peer reuse one connection rather than performing a new QUIC handshake each time.

---

## Design

The pool is a `moka::future::Cache<PoolKey, PooledConn>` with the following properties:

| Property | Value |
|----------|-------|
| Key | `(node_id: String, alpn: &'static [u8])` |
| Value | `PooledConn { conn: iroh::endpoint::Connection }` |
| Concurrency primitive | `try_get_with` (single-flight) |
| Stale detection | `conn.close_reason().is_none()` check before use |
| Eviction | Time-to-idle (configurable via `NodeOptions::pool_idle_timeout_ms`, default 60 s) |
| Max entries | Configurable via `NodeOptions::max_pooled_connections` |

---

## Single-flight via `try_get_with`

When two concurrent `fetch()` calls target the same peer and no connection exists yet, only one QUIC handshake should happen. `moka::future::Cache::try_get_with` provides this guarantee:

```rust
cache.try_get_with(key, async {
    ep.connect(addr, alpn).await.map_err(|e| format!("connect: {e}"))
}).await
```

- If the key is not in cache: the initializer runs once; all concurrent callers for that key wait on the same future.
- If the key is in cache: the cached value is returned immediately.
- If the initializer fails: the error is returned (as `Arc<String>`) to all waiters. The failure is **not** cached — the next call retries.

> Use `try_get_with`, not `get_with`. `get_with` is for infallible initializers; with connection errors, `get_with` would panic or misbehave.

---

## Stale connection handling

A pooled QUIC connection can become stale (remote closed, idle timeout, network change) while sitting in the cache. Before using a pooled connection, iroh-http-core checks:

```rust
if conn.close_reason().is_some() {
    pool.invalidate(&key).await;
    // retry: connect fresh
}
```

`close_reason()` is a non-blocking check on the connection's internal state. If it returns `Some`, the connection is dead and the cache entry is removed. The retry logic then falls through to a fresh `try_get_with`.

---

## ALPN segregation

The pool keys on `(node_id, alpn)` — not just `node_id`. This means:

- Regular HTTP requests use ALPN `b"iroh-http/2"` — pooled separately
- Duplex (`raw_connect`) uses ALPN `b"iroh-http/2-duplex"` — its own pool slot
- WebTransport sessions use a session-specific ALPN — their own pool slots

A peer can have multiple live connections in the pool, one per ALPN.

---

## Pool lifecycle

The pool is owned by `IrohEndpoint` (inside `EndpointInner`). Cloning an `IrohEndpoint` is a cheap `Arc` clone — all clones share the same pool.

When `IrohEndpoint::close()` is called, the pool is dropped. moka's cache holds weak references to its values by default — dropping the pool allows connections to be closed by the QUIC layer's normal drop path.

---

## Configuration

| Option | Default | Description |
|--------|---------|-------------|
| `NodeOptions::max_pooled_connections` | 128 | Maximum entries in the cache |
| `NodeOptions::pool_idle_timeout_ms` | 60 000 | Evict connections idle this long (ms) |

These are set when creating the endpoint:

```rust
IrohEndpoint::bind(NodeOptions {
    max_pooled_connections: Some(64),
    pool_idle_timeout_ms: Some(30_000),
    ..Default::default()
}).await?
```
