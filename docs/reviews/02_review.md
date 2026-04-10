# Reference Pattern Analysis

Comparison of old reference implementations (`.old_references/http-tauri`, `iroh`, `iroh-tauri`, `iroh-deno`) against the current `iroh-http` packages. Findings are grouped by priority.

---

## Critical — Performance

### 1. Base64 binary IPC (Tauri + Deno)

**Old pattern:** `http-tauri` used base64 encoding for all binary payloads crossing the JS↔Rust boundary. Benchmarks showed **~150× faster** throughput than JSON number arrays.

**Current code:** Both Tauri (`Array.from(chunk)`) and Deno (`Array.from(chunk)` / `number[]`) serialize every body chunk as a JSON array of numbers. A 64 KB chunk becomes a 300+ KB JSON array of integers, parsed element-by-element.

**Recommendation:** Switch to base64 encoding for `sendChunk` and `nextChunk` payloads in both Tauri and Deno adapters. The shared bridge interface can stay typed as `Uint8Array` — the encoding is an adapter-internal concern.

**Affected files:**
- `packages/iroh-http-tauri/guest-js/index.ts` — `sendChunk`, `nextChunk`
- `packages/iroh-http-tauri/src/commands.rs` — chunk serialization
- `packages/iroh-http-deno/guest-ts/adapter.ts` — `sendChunk`, `nextChunk`
- `packages/iroh-http-deno/src/lib.rs` or dispatch — chunk serialization

---

## High — Reliability

### 2. Streaming backpressure controls

**Old pattern:** `http-tauri` had configurable backpressure for body streaming:
- `maxChunkSize` (default 64 KB) — caps individual chunk size
- `maxInFlightChunks` (default 4) — limits how many chunks can be in-flight between JS and Rust
- Drain timeout — if the consumer doesn't read fast enough, the stream errors instead of leaking memory

**Current code:** `iroh-http-core` uses a fixed `mpsc::channel(32)` with no chunk-size cap and no drain timeout. A slow consumer can stall the channel silently while a fast producer fills 32 × unlimited-size chunks in memory.

**Recommendation:** Add configurable backpressure at the `iroh-http-core` level:
- Cap chunk size in `send_chunk` (or enforce in the shared layer)
- Expose the channel capacity in `NodeOptions`
- Add a drain timeout so stalled streams eventually error

### 3. Consecutive error resilience in accept loops

**Old pattern:** `iroh` / `iroh-tauri` tracked consecutive accept errors. Isolated errors (network hiccups) were logged and continued. **5 consecutive errors** triggered a fatal shutdown of the accept loop.

**Current code:** `iroh-http-core/src/server.rs` — the accept loop either succeeds or presumably panics/exits on error. There's no graduated error tolerance.

**Recommendation:** Add a consecutive-error counter to the serve accept loop. Log transient errors and continue; abort after N consecutive failures (configurable, default 5).

### 4. Bounded serve queue with eviction (Deno)

**Old pattern:** `iroh-tauri`'s `TauriAdapter` used a bounded queue (`MAX_QUEUE = 256`) for incoming requests, with oldest-item eviction when full — preventing memory growth under load.

**Current code:** The Deno adapter's `serve_registry.rs` uses an unbounded `mpsc` queue. Under sustained load or a slow JS handler, this queue grows without bound.

**Recommendation:** Cap the serve queue in `serve_registry.rs` (use `mpsc::channel(256)` instead of `mpsc::unbounded_channel()`). When full, either drop the oldest request with a 503 response or block the accept loop (backpressure).

---

## High — API Quality

### 5. `Symbol.asyncDispose` on IrohNode

**Old pattern:** Both `iroh` (`IrohEndpoint`, `IrohConnection`, `IrohRouter`) and `iroh-tauri` implemented `Symbol.asyncDispose`, enabling:
```ts
await using node = await createNode();
// automatically closed when scope exits
```

**Current code:** `IrohNode` has a `close()` method but no `Symbol.asyncDispose`. Users must manually call `close()` in a `finally` block.

**Recommendation:** Add `[Symbol.asyncDispose]` to `IrohNode` in `iroh-http-shared/src/index.ts` (or `bridge.ts`), delegating to `close()`. This is a one-line addition with significant ergonomic benefit.

### 6. Construction guard

**Old pattern:** `iroh` used a private `Symbol` to prevent direct `new IrohEndpoint()` construction — forcing users through `IrohEndpoint.create()` or the adapter factory. This prevented misuse and ensured proper initialization.

**Current code:** `IrohNode` is an interface returned by `buildNode()`, which already prevents `new IrohNode()`. **No action needed** — the current factory pattern achieves the same goal more idiomatically.

---

## Medium — Platform-Specific

### 7. Mobile auto-resurrection (Tauri)

**Old pattern:** `http-tauri` detected mobile app backgrounding via `visibilitychange` events and:
- Probed the Rust side with a health check on foreground resume
- Re-created the server/endpoint if the OS had killed it
- Used exponential-backoff health probes to avoid thrashing

**Current code:** No visibility-change handling. On mobile (iOS/Android via Tauri), the OS may terminate background tasks, leaving `IrohNode` holding a dead endpoint handle.

**Recommendation:** Add a `visibilitychange` listener in the Tauri guest-js that calls a lightweight `ping` / `isAlive` command on resume. If the endpoint is dead, either auto-reconnect or emit a `close` event so the app can re-create.

### 8. Fetch abort support

**Old pattern:** `http-tauri` used `CancellationToken` in Rust + `AbortSignal` in JS to abort in-flight fetch requests. Aborting cancelled the QUIC stream immediately, freeing resources.

**Current code:** `makeFetch` in `iroh-http-shared/src/fetch.ts` checks `signal.aborted` before starting, and calls `cancelRequest` on abort — but only cancels the _body reader_. The underlying QUIC stream and any in-progress Rust future are not cancelled.

**Recommendation:** Thread abort propagation through to the Rust layer. When `cancelRequest` is called, the Rust side should drop/reset the QUIC send/recv streams, not just close the body channel.

### 9. Structured Rust-side error codes

**Old pattern:** `http-tauri` returned errors as `{ code: string, message: string, details?: object }` from Rust, giving JS precise structured errors without regex parsing.

**Current code:** All Rust functions return `Result<T, String>`. The JS `classifyError` function parses these strings with regexes to determine error type. This is fragile — any Rust error message change breaks classification.

**Recommendation:** Return errors as `{ code: string, message: string }` JSON from Rust. Map the code directly to error subclass in `classifyError`. This is a cross-cutting change that touches `iroh-http-core` and all three adapters.

---

## Low — Nice to Have

### 10. EventTarget / event emitter on IrohNode

**Old pattern:** `iroh`'s `IrohConnection` extended `EventTarget`, emitting `close`, `pathchange`, and `datagram` events. The endpoint emitted connection-state events.

**Current code:** `IrohNode` has a `closed` promise but no general event surface.

**Recommendation:** Consider adding EventTarget to IrohNode for events like `close`, `error`, and potentially `peerconnected`. Lower priority since the current use-cases (fetch/serve/connect) don't strictly require it, but it would align with Web platform conventions.

### 11. Body store TTL / memory cap

**Old pattern:** `http-tauri` had a `BodyStore` with:
- TTL-based cleanup (bodies expire after 60s if unreferenced)
- 32 MB total memory cap with LRU eviction
- Reference counting to safely share bodies across commands

**Current code:** Global slabs grow without bound. Handles are cleaned up when streams complete normally, but abnormal termination (JS crashes, unhandled errors) can leak entries.

**Recommendation:** Add a periodic sweep to the global slabs in `iroh-http-core/src/stream.rs` that removes entries older than a configurable TTL. A memory cap is more complex but worth considering for production use.

### 12. Lazy body loading

**Old pattern:** `http-tauri` didn't read request bodies until JS explicitly called `body()` or `arrayBuffer()`. Large bodies stayed on the Rust side until needed.

**Current code:** The serve path allocates body reader handles eagerly for every request, even if the handler never reads the body.

**Recommendation:** This is partially mitigated by the current design (handles are cheap — just a slab index), but the channel + reader task are also created eagerly. Consider deferring channel creation until `nextChunk` is first called.

---

## Summary by adapter

| # | Pattern | Node | Tauri | Deno | Shared |
|---|---------|------|-------|------|--------|
| 1 | Base64 binary IPC | n/a (napi = zero-copy) | **needed** | **needed** | — |
| 2 | Backpressure controls | — | — | — | **needed** |
| 3 | Error resilience | — | — | — | **needed** |
| 4 | Bounded serve queue | — | — | **needed** | — |
| 5 | Symbol.asyncDispose | — | — | — | **needed** |
| 7 | Mobile resurrection | — | **needed** | — | — |
| 8 | Fetch abort propagation | — | — | — | **needed** |
| 9 | Structured error codes | needed | needed | needed | **needed** |

**Suggested implementation order:** 1 → 5 → 4 → 3 → 2 → 8 → 9 → 7
(Biggest wins for least effort first.)
