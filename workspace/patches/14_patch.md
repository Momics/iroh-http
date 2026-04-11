---
status: done
---

# iroh-http — Patch 14: P2P Security Hardening

## Problem

The server accept path has no protection against hostile or misbehaving peers.
In a P2P network, **any node with your public key can connect** — you don't
control who your peers are. The current code is vulnerable to several
resource-exhaustion attacks:

### 1. Unbounded header buffering

`read_request_head()` in `server.rs` loops appending to a `Vec<u8>` until
it finds `\r\n\r\n`:

```rust
loop {
    let chunk = recv.read_chunk(READ_BUF).await?;
    buf.extend_from_slice(&chunk.bytes);
    match parse_request_head(&buf) { ... }
}
```

A peer can send bytes that never contain `\r\n\r\n`, growing the buffer
until the process runs out of memory. The same applies to `read_head()` in
`client.rs` for response headers.

### 2. No request timeout

A peer can open a bidi stream and then go silent — never sending a complete
request, never closing the stream. The stream holds a semaphore permit
indefinitely, eventually exhausting the concurrency limit (default 64) and
preventing legitimate requests.

Similarly, after `on_request` notifies JS and the handler sends a response
head, a slow peer can stall body consumption indefinitely.

### 3. No per-peer connection limit

A single peer can open unlimited connections, each consuming memory and a
slot in the accept loop. With no limit, one hostile peer can crowd out all
others.

### 4. Uncapped request body size

A peer can send a multi-gigabyte chunked body. Even though backpressure
exists on the channel, the body pump task runs indefinitely and the
semaphore permit is held until the body is fully consumed.

---

## Design

### Max header size

Add a constant and an optional `NodeOptions` field:

```rust
const DEFAULT_MAX_HEADER_SIZE: usize = 64 * 1024; // 64 KB
```

In `read_request_head()` and `read_head()`, check `buf.len()` after each
append:

```rust
buf.extend_from_slice(&chunk.bytes);
if buf.len() > max_header_size {
    return Err("request head too large".into());
}
```

When the limit is hit, the stream is reset with an error and the connection
is unaffected (other streams on the same connection continue normally).

### Request timeout

Wrap the entire `handle_stream()` call in `tokio::time::timeout`:

```rust
tokio::spawn(async move {
    let _permit = permit;
    let timeout = Duration::from_secs(request_timeout_secs);
    match tokio::time::timeout(timeout, handle_stream(send, recv, ...)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!("iroh-http: stream error: {e}"),
        Err(_) => tracing::warn!("iroh-http: request timed out"),
    }
});
```

Default: **60 seconds**. Configurable via `ServeOptions.request_timeout_secs`.

This covers the full lifecycle: header read, JS handler execution, and
response body pump. If anything stalls beyond the timeout, the stream is
dropped and the semaphore permit is released.

For long-lived streaming responses (SSE, file transfers), the timeout should
be disabled or set high. Add `None` as the "no timeout" option:

```rust
pub struct ServeOptions {
    // ... existing fields ...
    /// Per-request timeout in seconds. `None` disables the timeout.
    /// Default: 60.
    pub request_timeout_secs: Option<u64>,
}
```

### Per-peer connection limit

Track active connections per `NodeId` using a `HashMap<NodeId, usize>`
protected by a `Mutex`. In the accept loop, after `incoming.await`:

```rust
let remote_id = conn.remote_id();
let count = peer_counts.lock().unwrap().entry(remote_id).or_insert(0);
if *count >= max_connections_per_peer {
    tracing::warn!("iroh-http: peer {remote_id} exceeded connection limit");
    conn.close(0u32.into(), b"too many connections");
    continue;
}
*count += 1;
```

Decrement on connection close (when `handle_connection` returns).

Default: **8 connections per peer**. Configurable via
`ServeOptions.max_connections_per_peer`.

### Optional max body size

For non-streaming use cases, an optional body size limit:

```rust
pub struct ServeOptions {
    // ... existing fields ...
    /// Maximum request body size in bytes. `None` means unlimited.
    /// When exceeded, the stream is reset. Default: None.
    pub max_request_body_bytes: Option<usize>,
}
```

Enforced in `pump_recv_to_body` by tracking total bytes received. When
exceeded, the stream is reset and the body channel is closed with an error.
This is opt-in because streaming use cases (file upload, SSE) need unlimited
bodies.

---

## Scope of changes

| Layer | Change |
|---|---|
| `iroh-http-core/src/server.rs` | Add header size check in `read_request_head()`. Wrap `handle_stream()` in `tokio::time::timeout`. Add per-peer connection tracking in accept loop. Add body size check in `pump_recv_to_body`. |
| `iroh-http-core/src/client.rs` | Add header size check in `read_head()` (protects against malicious response headers). |
| `iroh-http-core/src/endpoint.rs` | Add `max_header_size`, `request_timeout_secs`, `max_connections_per_peer`, `max_request_body_bytes` to `NodeOptions`. |
| `iroh-http-core/src/server.rs` (`ServeOptions`) | Surface new fields. |
| Bridge / JS layers | **No changes.** All limits are enforced at the Rust level. |

---

## Configuration summary

| Option | Default | Location |
|--------|---------|----------|
| `max_header_size` | 64 KB | `NodeOptions` |
| `request_timeout_secs` | 60 | `ServeOptions` |
| `max_connections_per_peer` | 8 | `ServeOptions` |
| `max_request_body_bytes` | None (unlimited) | `ServeOptions` |

---

## Verification

1. **Header bomb test**: Send 1 MB of `A` bytes without `\r\n\r\n`. Server
   should reject after 64 KB with a clear error, not OOM.
2. **Stall test**: Open a stream, send partial headers, go silent. Server
   should time out after 60s and release the permit.
3. **Connection flood test**: One peer opens 20 connections. Connections 9+
   should be closed immediately with "too many connections".
4. **Body limit test**: With `max_request_body_bytes: 1MB`, send 2 MB body.
   Stream should be reset after 1 MB.
5. **Legitimate traffic test**: Normal fetch/serve round-trips work unchanged
   with default settings.
