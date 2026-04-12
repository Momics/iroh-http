# Architecture

iroh-http is a thin integration layer that sits between platform-native FFI adapters and the hyper/tower HTTP engine running over Iroh's QUIC transport. It has no custom HTTP framing code — all framing, header parsing, chunked encoding, and trailer handling is delegated to hyper v1.

## Layer diagram

```
┌──────────────────────────────────────────────────────────┐
│  Platform adapters                                        │
│  Node.js (napi-rs) · Python (PyO3) · Deno (FFI) · Tauri  │
│                                                          │
│  Consume pub FFI functions only.                         │
│  All handle types are u64 (generational slotmap keys).   │
└────────────────────────┬─────────────────────────────────┘
                         │  fetch / serve / respond / next_chunk / …
┌────────────────────────▼─────────────────────────────────┐
│  iroh-http-core                                           │
│                                                          │
│  client.rs   — connect, hyper Request, pump response     │
│  server.rs   — accept loop, RequestService, drain        │
│  pool.rs     — moka-backed single-flight connection pool │
│  stream.rs   — global slotmap registries, all handles    │
│  session.rs  — WebTransport-style session API            │
│  endpoint.rs — IrohEndpoint, NodeOptions, ServeOptions   │
│  io.rs       — IrohStream: AsyncRead+AsyncWrite adapter  │
└──────┬──────────────────────────────┬────────────────────┘
       │                              │
┌──────▼──────────┐        ┌──────────▼───────────────────┐
│  tower-http     │        │  hyper v1                     │
│                 │        │                               │
│  Compression    │        │  HTTP/1.1 framing             │
│  Layer (zstd)   │        │  Header parsing               │
│  (feature-gated)│        │  Chunked encoding             │
│                 │        │  Trailer delivery             │
└─────────────────┘        │  Upgrade / duplex handshake   │
                           │  Body streaming (StreamBody)  │
                           └──────────┬────────────────────┘
                                      │
┌─────────────────────────────────────▼───────────────────┐
│  Iroh 0.96  (iroh-quinn → Quinn 0.11)                    │
│  SendStream / RecvStream                                  │
│  (implement AsyncWrite / AsyncRead — hyper drives them   │
│   directly via IrohStream)                               │
└──────────────────────────────────────────────────────────┘
```

## Components

### `iroh-http-core`

The core library. Owns all Rust-side logic. The platform adapters only call `pub` functions exported from this crate — they have no direct dependency on hyper, tower, or iroh internals.

| File | Responsibility |
|------|----------------|
| `client.rs` | `fetch()` and `raw_connect()`. Connects via pool, builds hyper request, pumps response body into handles. |
| `server.rs` | `serve()`. Accept loop, per-connection `RequestService`, drain semaphore, timeout middleware. |
| `pool.rs` | `ConnectionPool`. moka cache + single-flight via `try_get_with`. Stale-connection invalidation. |
| `stream.rs` | All resource handles (readers, writers, trailer channels, fetch tokens, sessions, request heads). Global slotmap registries. |
| `session.rs` | WebTransport-style session operations (bi/uni streams, datagrams). |
| `endpoint.rs` | `IrohEndpoint` (cheap `Arc` clone), `NodeOptions`, `ServeOptions`, `CompressionOptions`. |
| `io.rs` | `IrohStream`: bridges `iroh::SendStream`/`RecvStream` to `AsyncRead + AsyncWrite` for hyper's IO trait. |
| `lib.rs` | Public re-exports, `CoreError`/`ErrorCode`, `FfiResponse`, `RequestPayload`, `classify_error_json` compat shim. |

### Platform adapters

Each adapter crate is a thin FFI shim:

| Crate | FFI mechanism | Language |
|-------|---------------|----------|
| `iroh-http-node` | napi-rs v2 | Node.js / Bun |
| `iroh-http-deno` | Deno FFI (dlopen) | Deno |
| `iroh-http-py` | PyO3 | Python |
| `iroh-http-tauri` | Tauri invoke | Tauri (desktop/mobile) |

Adapters do not contain logic. They translate between platform types (e.g. `BigInt` ↔ `u64`) and call into iroh-http-core.

### `iroh-http-shared`

TypeScript types and shared logic used by the JS/TS adapters. Contains the `Bridge` interface that every adapter implements, and the `buildNode` function that composes the user-facing API from that bridge.

---

## Per-stream-per-request model

Each QUIC bidirectional stream carries exactly one HTTP/1.1 request-response exchange. Multiplexing is at the QUIC layer.

hyper's `http1::Builder` initializes a codec per stream — this is not a TCP handshake, just a few bytes of parser state. `keep_alive` is disabled: each QUIC stream is one exchange.

Benefits from QUIC that make this work well:
- Stream multiplexing (many concurrent requests, one QUIC connection)
- Per-stream flow control and backpressure
- 0-RTT connection resumption
- No head-of-line blocking between streams
- Encryption via the node keypair

---

## Concurrency model

The drain semaphore in `server.rs` is the central concurrency gate:

- `max_concurrency` (default: 64) = initial semaphore permits
- One permit acquired **per QUIC bi-stream** (= per HTTP request)
- Permit dropped when hyper finishes serving the request
- `drain()` acquires all `max_concurrency` permits → blocks until every in-flight request is done

This means `max_concurrency` limits simultaneous in-flight requests across all peers, not per-connection or per-peer.

---

## Key design decisions

**No custom HTTP framing.** Custom QPACK encoding and chunked-body logic was replaced by hyper v1. ~1,400 lines of custom Rust eliminated.

**Generational u64 handles.** All resource handles are `u64` slotmap keys (`KeyData::as_ffi()`). The generation counter prevents stale-handle use-after-free without any runtime bookkeeping. See [internals/resource-handles.md](internals/resource-handles.md).

**Single-flight pool.** `moka::future::Cache` with `try_get_with` ensures only one QUIC connection is established per `(node_id, alpn)` pair even under concurrent fetch pressure. See [internals/connection-pool.md](internals/connection-pool.md).

**Compression is opt-in.** The `compression` feature gates all of `tower-http`'s compression/decompression code. Disabled by default to avoid binary size impact on embedded targets.

**ALPN versions.** The wire format change from custom QPACK framing to standard HTTP/1.1 is a hard break. ALPN strings changed from `iroh-http/1*` to `iroh-http/2` and `iroh-http/2-duplex`. Old and new builds refuse to connect to each other. See [internals/wire-format.md](internals/wire-format.md).
