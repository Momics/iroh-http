---
status: reported
source: guidelines.md, crates/iroh-http-core
date: 2026-04-11
---

# Code Architecture Review — Composition, Build-vs-Buy, Rust Quality

Deep audit of `iroh-http-core` (3,912 lines across 9 files) examining code
composition, the build-vs-buy balance, and Rust code quality.

---

## 1. Is Compression Middleware?

**No.** Compression is wired as inline `#[cfg(feature = "compression")]` blocks
inside `client.rs` and `server.rs`. It interposes a new body channel between
the wire and the handler:

```
[QUIC stream] → [decompress_body] → [new BodyReader] → handler sees plain bytes
handler writes → [compress_body] → [new BodyReader] → [QUIC stream]
```

There is no middleware abstraction, no composable layer system, and no trait
that compression, rate limiting, or logging could implement. Each concern is
baked into the monolithic `fetch()` and `handle_stream()` functions.

### Is this the right design?

**For the Rust core: yes, mostly.** Guideline §3 ("Primitives, not policies")
says:

> If a feature requires intercepting bytes before they cross the FFI boundary
> (compression, framing, trailers), it belongs in core.

Compression must intercept the byte stream inside Rust, before JS ever sees it.
A Tower-style middleware stack would add abstraction cost with no benefit — the
only consumer of this Rust API is the FFI boundary, not end users.

**However**, the code should be better separated. Currently `handle_stream()`
in `server.rs` is ~160 lines doing: parse head → allocate channels → check
compression → call callback → encode head → check compression again → pump
body. These should be extracted into smaller composable functions:

```
read_and_decode_head() → apply_request_decompression() → dispatch_to_handler()
  → encode_response_head() → apply_response_compression() → pump_response()
```

**For the JS layer**: the design is correct. JS developers configure compression
via `createNode()` options. They don't compose it as middleware — it's a
transport-level concern that should be invisible. The current API design
(`compression: true | { level, minBodyBytes }`) is well-considered.

---

## 2. Are We Reinventing the Wheel?

Guideline §5 says:

> Prefer adopting well-maintained implementations (crates, libraries) over
> building equivalents from scratch. The maintenance cost of custom code must
> be justified by a real constraint.

### What's correctly delegated

| Concern | Crate | Verdict |
|---------|-------|---------|
| QUIC transport | `iroh` | Correct — this is the whole point |
| HTTP head parsing | `httparse` (via framing) | Correct — battle-tested, `no_std` |
| QPACK compression | `qpack` | Correct — extracted from `h3` |
| Zstd compression | `zstd` | Correct crate, wrong usage (bulk) |
| Async runtime | `tokio` | Correct |
| Handle management | `slab` | Correct |
| Ed25519 signing | `iroh::SecretKey` | Correct — uses iroh's existing impl |

### What's hand-rolled and shouldn't be

| Concern | Lines | Should use |
|---------|-------|------------|
| **Base32 encode/decode** | ~40 | `data-encoding` or `base32` crate |
| **Error classification** | ~50 | Typed error enum instead of string matching |
| **Streaming compression wrapper** | NA | `async-compression` crate (provides `ZstdEncoder<AsyncRead>`, `ZstdDecoder<AsyncRead>`) |

### Should we use Hyper or H3?

**No. This is the right call.** Here's why:

The wire protocol is **not standard HTTP/1.1, HTTP/2, or HTTP/3**. It's a bespoke
protocol that uses:
- QPACK-encoded headers (borrowed from HTTP/3) as a binary block with a 2-byte
  length prefix — NOT the HTTP/3 QPACK stream format
- HTTP/1.1 chunked body encoding (borrowed from HTTP/1.1)
- Custom ALPN identifiers (`iroh-http/1`, `iroh-http/1-full`, etc.)
- QUIC connections identified by Ed25519 public key, not TLS certificates
- No `:scheme` or `:authority` pseudo-headers — uses `:method` and `:path` only

**Hyper** handles HTTP/1.1 and HTTP/2 over TCP. It expects a TCP-like
`AsyncRead`/`AsyncWrite` transport. Iroh QUIC connections are stream-multiplexed
(one bidi stream per request), which doesn't map to hyper's connection model.
You'd fight hyper's connection management the entire way.

**H3** (`h3` crate) handles HTTP/3 over QUIC. It expects a full HTTP/3 framing
layer with QPACK encoder/decoder streams, SETTINGS frames, GOAWAY, and standard
ALPN (`h3`). iroh-http deliberately avoids all of this complexity — the protocol
is HTTP-semantics-over-QUIC-streams, not HTTP/3.

**Tower** could be useful for composable middleware (rate limiting, logging,
compression) on the serve path. But the FFI callback design
(`on_request: Fn(RequestPayload)`) is fundamentally incompatible with Tower's
`Service` trait, which expects `async fn(Request) -> Response`. Adopting Tower
would require redesigning the entire serve/respond flow. Not worth it given that
the only consumer is the FFI boundary.

### The protocol is intentionally bespoke

This is documented in guideline §5:

> This is a new protocol on new transport. We are not bound by backward
> compatibility with any existing HTTP stack.

The choice to use HTTP semantics (methods, status codes, headers, chunked
encoding, trailers) while using a custom binary wire format is deliberate.
It gives the project freedom to:
- Use QPACK without the full HTTP/3 framing overhead
- Skip HTTP/2's stream multiplexing (QUIC already does this)
- Use iroh's Ed25519 identity instead of TLS certificates
- Support embedded/`no_std` targets via the framing crate

**This is a valid architectural decision.** The codebase correctly identifies
what to borrow from standards and what to build itself.

---

## 3. Rust Code Quality Concerns

### P0 — Fix immediately

- [x] **Bulk compression must become streaming** ✅ FIXED — rewritten with `async-compression` `ZstdEncoder`/`ZstdDecoder` wrapping a custom `BodyAsyncRead` adapter. Both compress and decompress are now truly streaming with 64KB output buffers. No full-body accumulation. Removed direct `zstd` and `tokio-util` deps.

### P1 — Significant quality issues

- [ ] **Global mutable singletons everywhere**
  - 6+ `OnceLock<Mutex<Slab<...>>>` global statics (reader, writer, trailer_tx,
    trailer_rx, pending_readers, pending_responses, in_flight_tokens, sessions)
  - A second `IrohEndpoint::bind()` silently reconfigures backpressure for ALL
    existing channels
  - Fix: move slab ownership into `IrohEndpoint`. Each endpoint gets its own
    slab set. This also enables proper cleanup on endpoint close
  - This is the single biggest Rust quality problem — global mutable state is
    antithetical to Rust's ownership model

- [ ] **Error classification via string matching**
  - `classify_error_code()` in `lib.rs` does
    `.to_lowercase().contains("timeout")` on error messages
  - `session.rs` does `msg.contains("closed") || msg.contains("reset")`
  - These break silently when upstream `iroh` changes its error message text
  - Fix: match on error types, not messages. `iroh` errors implement
    `std::error::Error` with typed variants — use `downcast_ref::<>()` or
    exhaustive pattern matching

- [ ] **Duplicated pump functions**
  - `client.rs` has `pump_duplex_recv` / `pump_duplex_send`
  - `session.rs` has `pump_recv` / `pump_send`
  - These are nearly identical (~30 lines each)
  - Fix: extract into a shared `fn pump_bidi(recv, writer) + pump_bidi_send(reader, send)`
    in `stream.rs`

- [ ] **`.lock().unwrap()` on ~20 mutex operations**
  - Standard Rust practice but means any panic in a slab operation poisons the
    mutex and cascades across all operations
  - In a server handling hostile peers, a panic in one request handler kills the
    mutex for all future requests
  - Fix: use `.lock().unwrap_or_else(|e| e.into_inner())` to recover from
    poisoning, or use `parking_lot::Mutex` which doesn't poison

- [ ] **`handle_stream()` is ~160 lines**
  - Does: parse head → allocate channels → check compression → call callback →
    await response → encode head → check compression → pump body
  - Should be split into smaller functions with clear responsibility boundaries

### P2 — Quality improvements

- [ ] **Slab handle `as u32` cast**
  - `slab::Slab` uses `usize` keys, cast to `u32` with no overflow check
  - In practice slab sizes are small, but a `TryFrom` with error would be
    defensive
  - Fix: `u32::try_from(key).expect("slab overflow")`

- [ ] **`eprintln!` in sweep task**
  - `stream.rs` uses `eprintln!` for GC logging instead of `tracing`
  - Inconsistent with rest of codebase

- [ ] **`node_ticket()` swallows serialization errors**
  - `serde_json::to_string(&info).unwrap_or_default()` returns empty string on
    failure instead of propagating the error

- [ ] **No LRU eviction in connection pool**
  - Pool evicts the first `Ready` connection found via iterator, not least
    recently used
  - May evict active connections under load

---

## 4. What About `async-compression`?

For the streaming zstd fix, `async-compression` (2.7M downloads, maintained by
the Tokio team) is the ideal choice:

```rust
use async_compression::tokio::bufread::{ZstdEncoder, ZstdDecoder};
use tokio::io::BufReader;

// Compress a stream:
let encoder = ZstdEncoder::with_quality(reader, Level::Default);
// Decompress a stream:
let decoder = ZstdDecoder::new(reader);
```

It wraps `AsyncBufRead` and produces `AsyncRead`, so it integrates directly
with tokio I/O. The body channel system would need a thin adapter from
`BodyReader.next_chunk()` to `AsyncRead` (a `StreamReader` from
`tokio-util`), but this is straightforward.

**This replaces the bulk `compress_body()` / `decompress_body()` entirely.**
No more `Vec<u8>` accumulation, no decompression bomb risk, and true
streaming compression as the spec requires.

---

## 5. Compression Developer API — Current State and Recommendation

### Current exposure

| Platform | How it's configured | What's available |
|----------|--------------------|--------------------|
| **JS/TS** | `createNode({ compression: true \| { level, minBodyBytes } })` | On/off, level (1–22), threshold |
| **Python** | `create_node(compression_level=3, compression_min_body_bytes=1024)` | Level, threshold |
| **Rust** | `NodeOptions { compression: Some(CompressionOptions { level, min_body_bytes }) }` | Level, threshold |

### Should devs control this per-request?

**No.** This is a transport-level optimization between peers that both run this
library. There's no interop scenario where one side needs gzip and the other zstd.
Per-request control adds API surface with no real benefit in peer-to-peer.

If a developer needs to skip compression for a specific response (e.g. an already-
compressed file), they can set `Content-Encoding` explicitly and the library
respects it — no automatic compression is applied. This is the standard HTTP
escape hatch and it already works.

### Should it always be on?

**Recommendation: default ON.** The Cargo feature flag (`compression`) should be a
default feature, not opt-in. Most traffic patterns (JSON APIs, structured data,
event streams) benefit significantly from zstd. Developers who don't want it can
`default-features = false`.

Currently `compression` is `default = []` (opt-in). This means most users will
never discover it. Switch to:

```toml
[features]
default = ["compression"]
compression = ["dep:zstd", "dep:async-compression"]
```

### Should they set thresholds?

**Yes — node-level config is the right granularity.** The current API is
well-designed:
- `compression: true` — sensible defaults (level 3, 512 B threshold)
- `compression: { level: 1, minBodyBytes: 4096 }` — tuning available
- `compression: false` — explicit opt-out

This mirrors how HTTP servers configure compression (nginx `gzip_min_length`,
`gzip_comp_level`). No changes needed to the API shape — just need the
implementation fixed (streaming + threshold enforcement).

---

## Summary

| Question | Answer |
|----------|--------|
| Is compression middleware? | No — inline in fetch/serve. Correct for Rust core, but should be better extracted into composable functions |
| Should we use Hyper? | **No** — protocol is intentionally bespoke, hyper's transport model doesn't fit |
| Should we use H3? | **No** — we borrow QPACK but not H3 framing; using h3 would force full HTTP/3 compliance that isn't needed |
| Should we use Tower? | **No** — FFI callback model is incompatible; only consumer is the bridge, not end users |
| Should we use `async-compression`? | **Yes** — replaces hand-rolled bulk compression with streaming, maintained by Tokio team |
| Should we replace custom base32? | **Yes** — `data-encoding` crate is well-maintained and replaces ~40 lines of custom code |
| Is the protocol design sound? | **Yes** — HTTP-semantics-over-QUIC with QPACK is a justified bespoke protocol |
| Is the Rust code quality sound? | **Mixed** — good architecture, but global mutable state, string-matching errors, bulk compression, and code duplication need fixing |
| Should compression be on by default? | **Yes** — switch `compression` to a default feature |
| Should devs control compression per-request? | **No** — node-level config is right; per-request escape hatch (explicit `Content-Encoding`) already works |
