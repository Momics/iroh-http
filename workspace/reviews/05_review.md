---
status: reported
source: patches/12-28, features/*, guidelines.md
date: 2026-04-11
---

# Full Codebase Review — Patches 12–28 & Feature Specs

Comprehensive audit of the current implementation against every patch from 12
onwards, all feature specs, and the design guidelines. Each item is tagged with
its source patch/feature and a priority.

---

## Legend

- **P0** — Blocks correctness or shipping. Fix before any release.
- **P1** — Significant gap against the spec. Fix soon.
- **P2** — Quality / parity issue. Fix before open-source.
- **P3** — Nice-to-have, low risk.
- ✅ — Done correctly, no action needed.
- ⚠️ — Partially done, needs finishing.
- ❌ — Not done or broken.

---

## Patch-by-Patch Status

### Patch 12 — Connection Pool & Stream Multiplexing ✅

- [x] `ConnectionPool` in `pool.rs` with `(NodeId, ALPN)` keying
- [x] Connect-storm prevention via `watch::channel`
- [x] Stale connection eviction
- [x] `max_pooled_connections` in `NodeOptions`
- [x] `fetch()` and `raw_connect()` both use pool
- [x] Server side unchanged (already multiplexed)

No issues found. Well implemented.

---

### Patch 13 — QPACK Header Compression ✅

- [x] Phase 1 stateless encode/decode in `qpack_bridge.rs`
- [x] Wire format: 2-byte BE length prefix + QPACK block
- [x] Pseudo-headers (`:method`, `:path`, `:status`)
- [x] `QpackCodec` struct ready for future Phase 2
- [x] 7 unit tests (roundtrip, incomplete, missing pseudo-header, wire format)
- [x] `iroh-http-framing` unchanged (stays `no_std`)

No issues found. Phase 2 (dynamic table) deferred as designed.

---

### Patch 14 — P2P Security Hardening ✅

- [x] `max_header_size` (default 64 KB) enforced in QPACK read paths
- [x] `request_timeout_secs` (default 60s) via `tokio::time::timeout`
- [x] `max_connections_per_peer` (default 8) with HashMap counting
- [x] `max_request_body_bytes` (optional) with stream reset
- [x] Header size check in both server and client paths

No issues found. All limits working at Rust level.

---

### Patch 15 — Graceful Shutdown ⚠️

- [x] `ServeHandle` with `shutdown()`, `drain()`, `abort()` in `server.rs`
- [x] Semaphore-based drain logic
- [x] `drain_timeout_secs` configurable (default 30s)
- [x] `close()` and `close_force()` on `IrohEndpoint`
- [ ] **P1 — `node.close()` in JS does not accept `CloseOptions`**
  - Rust has `close(drain_timeout)` and `close_force()`
  - JS `close()` takes no arguments, always does hard close via `closeEndpoint`
  - No way for JS/Python developers to do graceful drain
  - Patch 15 spec: `close({ force?: boolean, drainTimeout?: number })`
- [ ] **P2 — No integration test for graceful shutdown behavior**

---

### Patch 16 — Integration Test Suite ⚠️

- [x] `integration.rs` — 11 tests (basic GET, POST, headers, trailers, concurrent, cancel)
- [x] `bidi_stream.rs` — 4 tests
- [x] `session_webtransport.rs` — 4 tests (uni streams, datagrams, close info)
- [x] `sign_verify.rs` — 3 tests
- [x] `ticket.rs` — 3 tests
- [x] Framing inline tests — ~22 tests
- [x] QPACK inline tests — 7 tests
- [x] Stream/pool/compress inline tests — ~24 tests
- [x] **P0 — CI does not run `cargo test`** ✅ FIXED — added `cargo test --workspace` to ci.yml
  - `.github/workflows/ci.yml` runs `cargo check`, `clippy`, `fmt` only
  - 85+ tests exist but never execute in CI
  - A regression can ship completely unnoticed
  - Fix: add `cargo test --workspace` step to `rust-check` job
- [ ] **P1 — Missing tests from patch 16 spec:**
  - `test_fetch_json` — no JSON content-type specific test
  - `test_fetch_large_body` — no large body fetch test (1 MB+)
  - `test_serve_concurrency_limit` — semaphore enforcement never tested
  - `test_mutual_fetch` — no bidirectional fetch test (A↔B)
  - `test_unknown_peer` — no unreachable peer test
  - `test_node_close` — no graceful shutdown behavior test
- [ ] **P1 — No server limits tests (patch 28)**
  - No test for 413 (body too large)
  - No test for 408 (request timeout)
  - No test for 503 (concurrency exceeded)
- [ ] **P2 — No platform smoke tests**
  - No `packages/iroh-http-node/test/smoke.mjs`
  - No Python tests at all
  - No Deno test script

---

### Patch 17 — NodeOptions Configurability ✅

- [x] `node.addr()` returning `NodeAddrInfo`
- [x] `node.homeRelay()` shorthand
- [x] `NodeAddr` as typed value with relay + direct addresses
- [x] `relayMode` (default / staging / disabled / custom) in `NodeOptions`
- [x] `bindAddrs` in `NodeOptions`
- [x] `proxyUrl` / `proxyFromEnv` in `NodeOptions`
- [x] `node.peerInfo(peer)` returns `NodeAddr | null`
- [x] `node.peerStats(peer)` returns `PeerStats | null`
- [ ] **P2 — DNS discovery URL override not applied**
  - `NodeOptions.dns_discovery` field exists
  - `bind()` always uses `n0_dns()` defaults — custom URL is stored but never
    used in endpoint construction
- [ ] **P2 — mDNS `serviceName` filtering**
  - `MdnsOptions.serviceName` accepted by `browse()`/`advertise()` but needs
    verification that the iroh mDNS implementation actually filters by it

---

### Patch 18 — Documentation & JSDoc Audit ✅

- [x] Shared layer JSDoc is excellent — near MDN quality
- [x] All exported types, interfaces, functions have JSDoc
- [x] `@example` blocks on key APIs
- [x] `@param` / `@returns` / `@throws` annotations
- [x] Error classes documented with usage examples
- [x] Rust `///` doc comments on public items in `iroh-http-core`
- [ ] **P2 — Python `create_node()` docstring is stale**
  - Only documents 4 of 16 parameters
  - Missing: proxy, compression, all server limits, lifecycle options
- [ ] **P2 — No `.pyi` type stubs for Python**
  - `py.typed` marker exists but no actual stub files
  - IDE users (Pylance, mypy) get no parameter signatures for native types
- [ ] **P2 — Rust napi doc comments not verified**
  - napi-rs copies `///` comments into generated `.d.ts`
  - Since `index.d.ts` is stale (see below), these comments are not reaching
    npm consumers

---

### Patch 19 — Body Compression ⚠️

- [x] `compression` Cargo feature flag (off by default)
- [x] `CompressionOptions` struct with `level` and `min_body_bytes`
- [x] Auto-negotiation: client injects `Accept-Encoding: zstd`, server detects
  and compresses response
- [x] Server decompresses inbound `Content-Encoding: zstd` request bodies
- [x] JS never sees compressed bytes
- [x] Configuration exposed in JS: `createNode({ compression: true | { level, minBodyBytes } })`
- [x] Configuration exposed in Python: `create_node(compression_level=3, compression_min_body_bytes=1024)`
- [x] **P0 — Compression is BULK, not streaming** ✅ FIXED — rewritten with `async-compression` ZstdEncoder/ZstdDecoder, both sides fully streaming
  - `compress_body()` and `decompress_body()` accumulate the entire body into
    memory, then compress/decompress in one shot
  - This completely defeats the streaming architecture
  - A 100 MB file transfer will buffer 100 MB before any compressed bytes flow
  - The feature spec and patch 19 both say: "streaming bodies compress
    incrementally — the QUIC send buffer is not stalled waiting for the full
    body"
  - The compression feature doc says: "compression happens inline in the body
    channel with a fixed-size ring buffer — no full-body buffering required"
  - **Neither of these is true in the current code**
  - Fix: use `zstd::stream::Encoder` / `zstd::stream::Decoder` wrapping an
    `AsyncRead` adapter, or use `async-compression` crate which provides
    `ZstdEncoder<R: AsyncRead>` / `ZstdDecoder<R: AsyncRead>` out of the box
  - This is the single most critical implementation bug — it turns a streaming
    transport into a buffering one for any compressed traffic
- [ ] **P1 — `min_body_bytes` threshold is never enforced**
  - `CompressionOptions.min_body_bytes` field exists (default 512)
  - `compress_body()` never reads or checks it
  - Small bodies (< 512 bytes) get compressed anyway, likely expanding them
  - Fix: check `Content-Length` against threshold before entering compress path;
    for streaming bodies without `Content-Length`, compress after first chunk
    exceeds threshold (or skip if stream ends before threshold)
- [ ] **P3 — Compression design: node-level config is correct**
  - Node-level on/off + level + threshold is the right granularity
  - Per-request override adds complexity with no P2P benefit (both sides run
    this library)
  - Developers can disable per-request by setting `Content-Encoding` explicitly
    (the library already respects this and skips auto-compression)
  - Current API surface is well-designed — just needs the implementation fixed

---

### Patch 20 — `serve` API Ergonomics ✅

- [x] `serve(handler)` — handler-only overload
- [x] `serve(options, handler)` — options + handler
- [x] `serve({ handler, ...options })` — single-arg form
- [x] `ServeHandle` with `finished: Promise<void>` returned
- [x] `onError` in `ServeOptions` — catches handler throws/rejects
- [x] `onListen` in `ServeOptions` — fires with `{ nodeId }`
- [x] `signal` in `ServeOptions` — wired to `stopServe` FFI

No issues found. Matches Deno.serve patterns well.

---

### Patch 21 — Discovery: `browse()` and `advertise()` ⚠️

- [x] `node.browse(options?, signal?)` returns `AsyncIterable<PeerDiscoveryEvent>`
- [x] `node.advertise(options?, signal?)` returns `Promise<void>`
- [x] `PeerDiscoveryEvent` with `isActive`, `nodeId`, `addrs`
- [x] `AbortSignal` cancellation on both
- [x] `onPeerDiscovered` callback removed from `IrohNode`
- [x] Rust: `BrowseSession` + `AdvertiseSession` in `iroh-http-discovery`
- [x] Wired in Node, Deno, Tauri adapters
- [ ] **P1 — Python has no browse/advertise**
  - `iroh-http-discovery` is not a dependency in `packages/iroh-http-py/Cargo.toml`
  - No `browse()` or `advertise()` methods on Python `IrohNode`
  - Python users cannot discover peers on local network
- [ ] **P2 — `NodeOptions.discovery.mdns` removal**
  - Patch 21 says to remove `discovery.mdns` from `NodeOptions`
  - Need to verify this was actually removed from all adapters and not just
    shadowed by the new methods

---

### Patch 22 — Bidirectional Streams on `IrohSession` ✅

- [x] `createBidirectionalStream` removed from `IrohNode`
- [x] Lives only on `IrohSession` (via `node.connect(peer)`)
- [x] `session.incomingBidirectionalStreams` as `ReadableStream`
- [x] 4 integration tests (roundtrip, multiple streams, backpressure, clean close)
- [x] All four JS adapters wired

No issues found. Clean WebTransport alignment.

---

### Patch 25 — Sign / Verify Helpers ⚠️

- [x] `secret_key_sign()`, `public_key_verify()`, `generate_secret_key()` in Rust
- [x] `SecretKey.sign()`, `PublicKey.verify()` on JS classes
- [x] `SecretKey.generate()` static method
- [x] 3 Rust tests (roundtrip, bad sig, unique keys)
- [x] Wired in Node, Deno, Tauri adapters
- [ ] **P1 — Python `__init__.py` does not export sign/verify functions**
  - `secret_key_sign`, `public_key_verify`, `generate_secret_key` exist in
    native module but are missing from `__all__`
  - `from iroh_http import *` and IDE autocomplete won't find them
  - Fix: add to `__all__` list in `__init__.py`

---

### Patch 26 — Node Tickets ✅

- [x] `node.ticket()` generates ticket string
- [x] `parse_node_addr()` accepts bare node ID or ticket
- [x] `ticketNodeId()` pure TS helper
- [x] `fetch` and `connect` accept ticket strings
- [x] 3 Rust tests
- [x] Wired in all four adapters including Python

No issues found.

---

### Patch 27 — WebTransport Compatibility ⚠️

- [x] `IrohSession` with full WebTransport interface
- [x] `session.ready` (instant, API compat)
- [x] `session.closed` → `Promise<WebTransportCloseInfo>`
- [x] `session.createBidirectionalStream()` + `incomingBidirectionalStreams`
- [x] `session.createUnidirectionalStream()` + `incomingUnidirectionalStreams`
- [x] `session.datagrams` (WebTransportDatagramDuplexStream)
- [x] `session.close({ closeCode, reason })`
- [x] `node.closed` shape changed to `Promise<WebTransportCloseInfo>`
- [x] Rust: full session slab with all operations
- [x] 4 integration tests (uni stream, multiple uni, datagram, close info)
- [x] All three JS adapters wired
- [ ] **P1 — Python `IrohSession` missing `ready` and `closed`**
  - Core exposes `session_ready()` and `session_closed()` → `CloseInfo`
  - Node.js exposes both
  - Python `IrohSession` has neither
  - Cannot await session readiness or get close code/reason in Python
- [ ] **P1 — Python missing incoming uni stream receive**
  - `create_unidirectional_stream()` (send-only) is exposed
  - No `next_uni_stream()` or equivalent for receiving incoming uni streams
  - Core has `session_next_uni_stream()` but Python doesn't call it

---

### Patch 28 — Expose Server Limits in TypeScript `ServeOptions` ⚠️

- [x] `maxConcurrency` in JS `NodeOptions` → Rust `ServeOptions`
- [x] `maxConnectionsPerPeer` in JS `NodeOptions` → Rust
- [x] `requestTimeout` in JS `NodeOptions` → Rust
- [x] `maxRequestBodyBytes` in JS `NodeOptions` → Rust
- [x] All four JS adapters wire these to `ServeOptions`
- [ ] **P2 — `maxHeaderBytes` not exposed in TypeScript `NodeOptions`**
  - Rust has `max_header_size` (64 KB default) and enforces it
  - Feature spec lists it as one of the five server limits
  - JS surface has no way to configure it — always uses default
  - Fix: add `maxHeaderBytes?: number` to `NodeOptions` in `bridge.ts`
- [ ] **P1 — No integration tests for server limit enforcement**
  - No test for 413 when body exceeds `maxRequestBodyBytes`
  - No test for 408 when request exceeds timeout
  - No test for 503 when concurrency exceeded
  - No test that connections beyond `maxConnectionsPerPeer` are rejected

---

## Feature Spec Compliance

### compression.md ⚠️

- [x] zstd only, behind Cargo feature flag
- [x] `Accept-Encoding` / `Content-Encoding` negotiation
- [x] JS handler never sees compressed bytes
- [x] `createNode({ compression: { level, minBodyBytes } })` in JS
- [x] `create_node(compression_level=..., compression_min_body_bytes=...)` in Python
- ❌ **Streaming compression — spec says "incrementally", code does bulk**
- ❌ **`minBodyBytes` threshold — spec says enforced, code ignores it**

### default-headers.md ✅

- [x] `iroh-node-id` stripped on parse, re-injected from QUIC state
- [x] Unforgeable identity header

### discovery.md ⚠️

- [x] DNS discovery enabled by default
- [x] mDNS via `browse()` / `advertise()` with `AbortSignal`
- [x] `PeerDiscoveryEvent` with `isActive`, `nodeId`, `addrs`
- ❌ **Python: no mDNS at all**
- ⚠️ **Custom DNS resolver URL stored but not applied**

### observability.md ✅

- [x] `node.peerStats(nodeId)` returns `PeerStats`
- [x] `node.addr()` returns `NodeAddrInfo`
- [x] Returns null for disconnected peers (no throw)

### rate-limiting.md ⚠️

- [x] Rust-level `maxConnectionsPerPeer` enforced
- ❌ **No TS middleware (`rateLimit()`) — spec describes token bucket + compose**
- This may be intentionally deferred — it's a higher-level concern

### server-limits.md ⚠️

- [x] All five limits implemented in Rust
- [x] Four of five exposed in JS NodeOptions
- ❌ **`maxHeaderBytes` not in JS surface**
- ❌ **No tests for enforcement behavior**

### sign-verify.md ⚠️

- [x] Ed25519 sign/verify/generate in Rust
- [x] Full JS surface on `SecretKey` / `PublicKey` classes
- ❌ **Python: functions exist but not exported in `__all__`**

### streaming.md ✅

- [x] `ReadableStream` / `WritableStream` for bodies
- [x] Backpressure via bounded channels
- [x] `AbortSignal` cancellation propagated to Rust
- [x] `duplex: 'half'` support
- [x] Chunk splitting for oversized sends

### tickets.md ✅

- [x] `node.ticket()` generates shareable string
- [x] `ticketNodeId()` decodes without network I/O
- [x] `fetch()` and `connect()` accept tickets directly

### trailer-headers.md ✅

- [x] `req.trailers` as `Promise<Headers>`
- [x] Response trailers via `sendTrailers`
- [x] ALPN negotiation for trailer support
- [x] Framing: `serialize_trailers` / `parse_trailers`

### webtransport.md ⚠️

- [x] `IrohSession` implements WebTransport interface
- [x] Bidi streams, uni streams, datagrams all present
- [x] `close(info?)` with code + reason
- [x] `node.closed` returns `WebTransportCloseInfo`
- ❌ **Python: missing `ready`, `closed`, incoming uni streams**

---

## Cross-Cutting Issues

### 1. CI / Testing Infrastructure

| Item | Status | Priority |
|------|--------|----------|
| `cargo test --workspace` in CI | ❌ Not run | **P0** |
| 85+ Rust tests exist but only run locally | ⚠️ | — |
| Node.js smoke test | ❌ Not created | P2 |
| Python tests | ❌ None exist | P2 |
| Deno test script | ❌ Not created | P2 |
| 6 of 12 Patch 16 tests missing | ⚠️ | P1 |
| Server limits tests | ❌ None | P1 |

### 2. Stale Build Artifacts

| Item | Status | Priority |
|------|--------|----------|
| `iroh-http-node/index.d.ts` (napi-generated) | ❌ Severely stale — missing sessions, addr, discovery, correct arity | **P0** |
| Missing: all session FFI types | ❌ | — |
| Missing: `rawFetch` has 7 params (should be 8) | ❌ | — |
| Missing: `JsNodeOptions` fields (14+ missing) | ❌ | — |
| Fix: run `napi build --platform` to regenerate | — | — |

### 3. Python Parity Gaps

| Gap | Priority |
|-----|----------|
| `secret_key_sign`, `public_key_verify`, `generate_secret_key` not in `__all__` | **P1** |
| No `session.ready` or `session.closed` on `IrohSession` | **P1** |
| No incoming uni stream receive (`next_uni_stream`) | **P1** |
| No mDNS `browse()` / `advertise()` — discovery crate not wired | **P1** |
| `create_node()` docstring stale (4 of 16 params documented) | P2 |
| No `.pyi` type stubs | P2 |
| No `__aenter__` / `__aexit__` on resource classes | P2 |

### 4. Guideline Compliance (from review 04)

| Issue | Priority |
|-------|----------|
| Public JS API leaks FFI types (`FfiRequest`, `FfiResponse`, `RequestPayload`) | P2 |
| Custom error hierarchy vs DOMException-first (guideline §1) | P2 |
| Tauri import path (`"iroh-http-shared"` vs `"@momics/iroh-http-shared"`) | P3 |
| Custom base32 codec in both Rust and TS (vs using a crate/package) | P3 |

---

## Consolidated Fix List (Priority Order)

### P0 — Fix immediately

- [x] **Add `cargo test --workspace` to CI** (`ci.yml` `rust-check` job) ✅ DONE
- [x] **Streaming zstd** — replaced with `async-compression` `ZstdEncoder`/`ZstdDecoder` ✅ DONE
- [ ] **Regenerate `index.d.ts`** for `iroh-http-node` via `napi build --platform`.
  Current file is dangerously out of sync with actual napi exports

### P1 — Fix before release

- [ ] **Wire `CloseOptions` through `node.close()`** — pass `{ force, drainTimeout }`
  to Rust `close(drain_timeout)` / `close_force()`. All four adapters need updating
- [ ] **Enforce `min_body_bytes`** in compression path — check `Content-Length` or
  first-chunk size before entering compress/decompress
- [ ] **Python: export sign/verify** — add `secret_key_sign`, `public_key_verify`,
  `generate_secret_key` to `__all__` in `__init__.py`
- [ ] **Python: add `session.ready` and `session.closed`** — wire
  `session_ready()` and `session_closed()` from core
- [ ] **Python: add incoming uni stream** — wire `session_next_uni_stream()` from core
- [ ] **Python: add mDNS browse/advertise** — add `iroh-http-discovery` dependency to
  `Cargo.toml`, expose `browse()` and `advertise()` methods
- [ ] **Add missing integration tests:**
  - `test_fetch_json` (POST JSON, verify content-type and parsed body)
  - `test_fetch_large_body` (1 MB+ body round-trip)
  - `test_serve_concurrency_limit` (max_concurrency=2, 3rd request queued)
  - `test_mutual_fetch` (A serves + fetches from B, B serves + fetches from A)
  - `test_unknown_peer` (fetch to non-existent NodeId → connection error)
  - `test_node_close` (graceful shutdown drains in-flight)
- [ ] **Add server limits tests:**
  - Body exceeds `max_request_body_bytes` → 413
  - Request exceeds timeout → 408
  - Concurrency exceeded → 503 or queued
  - Connections beyond `max_connections_per_peer` → rejected

### P2 — Fix before open-source

- [ ] **Expose `maxHeaderBytes`** in TypeScript `NodeOptions`
- [ ] **Apply custom DNS resolver URL** — `NodeOptions.dns_discovery` field is stored
  but never used in `bind()`
- [ ] **Update Python `create_node()` docstring** — document all 16 parameters
- [ ] **Generate `.pyi` type stubs** for Python package
- [ ] **Add `__aenter__` / `__aexit__`** to Python `IrohNode` and `IrohSession`
- [ ] **Create Node.js smoke test** (`packages/iroh-http-node/test/smoke.mjs`)
- [ ] **Remove FFI type leakage** from `iroh-http-shared` public exports
- [ ] **Fix Tauri import path** to use `@momics/iroh-http-shared` consistently

### P3 — Nice-to-have

- [ ] QPACK Phase 2 (dynamic table) — deferred by design, revisit with profiling data
- [ ] Rate-limiting middleware (`rateLimit()`, `compose()`) — may be a separate package
- [ ] `pathChanges(nodeId)` async iterable (observability feature spec)
- [ ] Replace custom base32 implementations with maintained crate/package
- [ ] Per-connection idle timeout in pool (currently relies on max_idle count only)
