# Design Decisions — iroh-http internals

This document captures the *why* behind major architectural choices in
`iroh-http-core`. It is intended for contributors who want to understand
trade-offs, not just what the code does.

---

## 1. Why hyper instead of custom framing?

`iroh-http-core` previously hand-rolled HTTP/1.1 framing on top of Iroh QUIC
streams via a dedicated `iroh-http-framing` crate and a suite of custom pump
functions (~1,400 lines total):

| Custom file | What it did | Replaced by |
|---|---|---|
| `iroh-http-framing` | Chunked transfer encoding, byte scanner | hyper v1 |
| `qpack_bridge.rs` | Stateless QPACK header encode/decode | hyper (standard HTTP/1.1 headers) |
| `compress.rs` | Streaming zstd (255 lines) | `tower-http` `CompressionLayer` |
| `pool.rs` | `Slot` enum + watch-channel single-flight | `moka` async cache |
| `stream.rs` (slabs) | `HashMap<u32,T>` + `AtomicU32` per-type | `slotmap` generational keys |

The QPACK usage was stateless (dynamic table never enabled), so it provided
negligible compression benefit over plain HTTP/1.1 headers while adding a
custom encoding layer with no ecosystem support.

Iroh's `SendStream` and `RecvStream` implement `tokio::io::AsyncWrite` and
`AsyncRead`. hyper v1 can drive them directly via `hyper_util::rt::TokioIo`
with a thin stream-pair wrapper (`IrohStream`). This makes the entire HTTP
machinery hyper's responsibility, not ours.

---

## 2. Wire format: HTTP/1.1 over raw QUIC streams

Each QUIC bidirectional stream carries exactly **one** HTTP/1.1 request–response
exchange. Multiplexing is provided by the QUIC layer; HTTP framing is not used
for multiplexing.

```
Request:
  GET /path HTTP/1.1\r\n
  Host: <node-id>\r\n
  <headers>\r\n
  \r\n
  [HTTP/1.1 chunked body]

Response:
  HTTP/1.1 200 OK\r\n
  <headers>\r\n
  \r\n
  [HTTP/1.1 chunked body]
```

**ALPN versioning**: wire-format breaks require a new ALPN string. The current
ALPN strings (`iroh-http/2`, `iroh-http/2-duplex`) bump to version 2 after the
hyper migration. Old and new builds refuse to connect to each other because the
ALPN won't match — this is intentional.

The `-duplex` variant is preserved for `raw_connect` (uses HTTP Upgrade:
`CONNECT` + `Upgrade: iroh-duplex` → 101 Switching Protocols → raw stream).

**hyper configuration notes** (non-obvious):
- `keep_alive(false)` is mandatory. Each QUIC stream is one exchange; hyper
  must not attempt to reuse it.
- `max_buf_size` panics if set below 8192. Always clamp: `.max(8192)`. The
  actual configured limit is enforced post-parse on the byte count.
- `TowerToHyperService` requires the `service` feature in `hyper-util`
  (not enabled by default).

---

## 3. Per-stream concurrency model

One `tokio::sync::Semaphore` permit is acquired **per QUIC bi-stream** (= per
HTTP request), not per connection. This is the correct granularity because one
connection can carry many concurrent streams.

```rust
// Inside the accept loop — per-stream, not per-connection
let permit = conn_drain.clone().acquire_owned().await?;
// permit is moved into the spawned task
```

The semaphore serves dual purpose: concurrency cap and graceful drain
(waiting for all in-flight requests to finish before shutdown). `clone()` is
needed inside the loop because `acquire_owned()` takes ownership.

---

## 4. Why slotmap for resource handles?

The previous handle model used `HashMap<u32, T>` + `AtomicU32` counters per
resource type. A `u32` handle is a raw incrementing counter — it wraps after
4 billion allocations and can alias: a stale handle from a previous resource
might accidentally access a new one occupying the same slot.

`slotmap` generational keys (u64) eliminate this structurally:
- Lower 32 bits: slot index (position in the arena)
- Upper 32 bits: generation counter (incremented on each remove+reuse)

A stale handle will not match the current generation and returns
`Err("invalid handle")` rather than silently accessing the wrong resource.

Handles cross the FFI boundary as `u64`. JSON cannot round-trip `u64` values
above 2⁵³ safely through `Number`, so all handle values are transmitted as
`BigInt` in JavaScript adapters. The JSON serializer in the Deno adapter uses
a bigint→Number replacer (safe because slotmap indices fit well within f64),
and all returned handle values are wrapped in `BigInt()` at the boundary.

### FFI handle invariant: never truncate to `u32`

The generation counter lives in the **upper** 32 bits of the handle, so any
truncation to `u32` along the FFI path silently strips it and re-introduces
exactly the ABA bug slotmap was adopted to prevent.

Issue [#161](https://github.com/Momics/iroh-http/issues/161) was a textbook
instance: the Deno dispatch declared `endpoint_handle: u32` in several
`extern "C"` symbols and dashmap key types. The first endpoint and a
freshly-rebound second endpoint both occupied slot index 0; their full
`u64` handles differed only in the upper 32 bits; the truncation collapsed
them and the second test in a pair received the closed endpoint's state.

Rules every adapter must follow:

- All FFI symbols accepting an endpoint handle declare it `u64`.
- All Rust-side maps keyed by handle use `u64` (or `(u64, …)`).
- All JS callers wrap the handle in `BigInt()` before passing it across the
  FFI boundary.
- The unit test `registry::tests::handle_round_trip_changes_after_reuse`
  documents the failure mode and asserts that truncating to `u32` would
  alias two distinct handles.

---

## 5. Connection pool: moka + try_get_with

The pool uses `moka::future::Cache` as a single-flight concurrent map. The
critical API choice is `try_get_with` (not `get_with`):

```rust
cache.try_get_with(key, async { connect().await }).await
```

`get_with` is for infallible initialization — it accepts a plain future and
caches whatever it returns. If connection setup fails, `get_with` would cache
the error or panic at the type level.

`try_get_with` accepts a fallible future (`Result<V, E>`). On error, it does
not cache the failure — the next caller will retry the connection attempt.
This is essential for a connection pool where transient network errors should
not permanently poison a peer's entry.

---

## 6. Compression policy: zstd-only

`tower-http`'s `DecompressionLayer::new()` and the `decompression-full` Cargo
feature enable all supported encodings (gzip, br, deflate, zstd) by default.
iroh-http's policy is **zstd-only** — other encodings are not negotiated:

```rust
// Correct: explicit zstd-only
tower_http::decompression::DecompressionLayer::new()
    .gzip(false)
    .br(false)
    .deflate(false)
    .zstd(true)
```

Use the `decompression-zstd` Cargo feature (not `decompression-full`) so other
codecs are not compiled in. This makes the policy visible in the dependency
graph, not just runtime configuration.

---

## 7. `remote_node_id` threading

The peer's identity is a QUIC connection-level fact, not an HTTP request-level
fact. `RequestService` is a `tower::Service<Request<Incoming>>` and only sees
the HTTP request. The solution is to **clone a fresh `RequestService` per
connection** with the peer identity baked in before calling `serve_connection`:

```rust
let remote_id = base32_encode(conn.remote_id().as_bytes());
let mut peer_svc = base_svc.clone();
peer_svc.remote_node_id = Some(remote_id);
// serve_connection takes ownership of peer_svc
```

`RequestService::call` uses `.unwrap_or_default()` (not `.unwrap()`) when
reading `remote_node_id`. A missing identity due to a wiring regression
produces an observable empty string rather than a hard panic in the request
task.

---

## 8. Duplex / raw_connect upgrade path

`raw_connect` uses HTTP Upgrade semantics rather than a separate QUIC stream
type:

```
Client:  CONNECT /path HTTP/1.1 + Upgrade: iroh-duplex
Server:  HTTP/1.1 101 Switching Protocols
After 101: raw bidirectional byte stream, no HTTP framing
```

hyper's `upgrade::on` handles the 101 handshake. After upgrade, the
`hyper::upgrade::Upgraded` value gives back the raw IO, which is split into
the existing `BodyReader`/`BodyWriter` channel handles.

Two concurrent pump tasks run after upgrade:
1. `recv_io → req_body_writer` (inbound bytes from the remote side)
2. `res_body_reader → send_io` (outbound bytes to the remote side)

Both must be live simultaneously. Dropping `req_body_writer` before the
upgrade resolves causes the remote to see an immediate EOF. The body writer
must be kept alive as an `Option` in the duplex branch and moved into the
upgrade spawn.

---

## 9. Header size enforcement

hyper enforces `max_buf_size` as a parse-level limit (bytes in the read
buffer), but it panics for values below 8192. The actual user-configured limit
is enforced post-parse by summing header name and value byte lengths:

```rust
let measured: usize = req.headers()
    .iter()
    .map(|(k, v)| k.as_str().len() + v.as_bytes().len())
    .sum();
```

Note: `v.as_bytes().len()` is used (not `v.to_str().unwrap_or("").len()`)
because `to_str()` fails for non-UTF8 header values, which would undercount
the actual bytes on the wire.

---

## 10. Security invariants

These are the non-negotiable gates for any change to the HTTP engine:

1. **Resource bounds**: max request-header bytes, max body bytes, per-request
   timeout, global concurrency limit, and per-peer connection limit are all
   enforced.
2. **Cancellation**: fetch cancellation aborts transport work and body readers
   deterministically. Stream cancellation paths do not silently convert errors
   to EOF.
3. **Drain**: serve stop/drain does not orphan tasks or leave unresolved
   requests. All in-flight requests complete or time out before the serve handle
   resolves.
4. **Trailer completion**: trailer send/receive paths resolve exactly once,
   including error paths.
5. **Protocol**: `httpi://` scheme is enforced at all public API boundaries.
   Method validation allows extension methods (valid token syntax, not just
   common verbs). Duplex upgrade is deterministic.
6. **Error taxonomy**: `CoreError` / `ErrorCode` enum is the canonical error
   model. No opaque string errors leak through public API where typed mapping
   is expected.

---

## 11. h3 upgrade path

Nothing in this architecture closes the door to HTTP/3. The application layer
(`tower::Service` and all business logic) would be unchanged — only the
transport wiring needs to swap.

**Current state of the stack:**

Iroh has shipped on [noq](https://www.iroh.computer/blog/noq-announcement)
("number 0 QUIC") since v0.96. noq is n0's hard fork of Quinn with first-class
multipath, NAT traversal, and QUIC Address Discovery built in. It exposes a raw
`noq::Connection` and a `WeakConnectionHandle`.

The h3 blocker is therefore not "Iroh doesn't expose the underlying connection"
anymore — it's that there is no `h3-noq` crate yet. The existing `h3-quinn`
crate is tightly coupled to `quinn::Connection` and won't work with
`noq::Connection`.

**The path to h3:**

1. A `h3-noq` crate is written (analogous to `h3-quinn`, but driving `noq`)
2. Iroh exposes `noq::Connection` from its public API
3. We swap `hyper::server::conn::http1::Builder` for `h3::server::Connection`
4. The `tower::Service` layer and all application logic are unchanged

The QUIC multiplexing model (one exchange per stream) maps cleanly to h3
streams. Step 3 and 4 are the easy part; steps 1 and 2 are upstream work.
