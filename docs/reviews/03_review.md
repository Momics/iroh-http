---
status: open
---

# Performance Review: IPC & Streaming Patterns

Cross-platform audit of binary transfer, buffering, and serialization in the
current Node, Tauri, and Deno adapters. Compared against old reference
implementations and ideal patterns.

---

## Positives (already better than old references)

1. **Handle-based slab design** — integer slab indices instead of string
   HashMap keys behind `tokio::Mutex`. Zero contention on lookups.

2. **Base64 for binary IPC** — both Tauri and Deno already use base64 for
   chunk payloads, matching the old references' approach.

3. **Configurable backpressure** — `channel_capacity` and `max_chunk_size`
   are plumbed through `NodeOptions` to `configure_backpressure()`. The old
   references had similar controls but they were added late.

4. **Fetch abort propagation** — `alloc_fetch_token` + `cancel_in_flight` +
   `tokio::select!` cleanly cancels the Rust future and drops QUIC streams.
   The old `http-tauri` broadcast N individual IPC calls for N active tokens.

5. **Pull-based streaming** — `ReadableStream.pull()` naturally serializes
   reads and avoids push-side buffering. The old `http-tauri` used a push
   model (`Channel.onmessage`) with no backpressure from JS to Rust.

---

## Finding 1: O(n²) base64 encoding — Tauri + Deno

**Severity: P0 — critical performance bug**

**Files:**
- `packages/iroh-http-tauri/guest-js/index.ts` lines 20–25
- `packages/iroh-http-deno/guest-ts/adapter.ts` lines 57–62

Both adapters use identical byte-by-byte string concatenation:

```ts
function encodeBase64(u8: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < u8.length; i++) bin += String.fromCharCode(u8[i]);
  return btoa(bin);
}
```

JavaScript strings are immutable. Each `+=` allocates a new string and copies
all previous characters. For a 64 KB chunk, this performs ~64,000 string
allocations and copies `0 + 1 + 2 + ... + 65535 ≈ 2 GB` of character data
internally. The old references had this same bug and never fixed it.

**Fix:** Use a chunked approach that avoids O(n²) concatenation:

```ts
function encodeBase64(u8: Uint8Array): string {
  const CHUNK = 0x8000; // 32 KB — safe for String.fromCharCode spread
  const parts: string[] = [];
  for (let i = 0; i < u8.length; i += CHUNK)
    parts.push(String.fromCharCode(...u8.subarray(i, i + CHUNK)));
  return btoa(parts.join(""));
}
```

This is O(n) with a single `join` at the end. The `decodeBase64` helpers are
fine — `atob` returns a string and the byte-by-byte loop has no concatenation.

---

## Finding 2: Unnecessary memory copy per chunk — Node

**Severity: P1 — measurable throughput impact**

**File:** `packages/iroh-http-node/src/lib.rs` lines 118–127

### Read path (`nextChunk`):

```rust
Ok(chunk.map(|b| Uint8Array::new(b.to_vec())))
```

`b` is a `Bytes` (refcounted, zero-copy sliceable). `.to_vec()` allocates a
new `Vec<u8>` and copies every byte. Then `Uint8Array::new()` copies again into
a JS ArrayBuffer. For a 64 KB chunk: **128 KB of unnecessary copying**.

### Write path (`sendChunk`):

```rust
let bytes = Bytes::copy_from_slice(chunk.as_ref());
```

`chunk.as_ref()` gives a `&[u8]` view of the JS ArrayBuffer.
`Bytes::copy_from_slice` allocates and copies. For a 64 KB chunk: **64 KB
unnecessary copy**.

**Fix (read):** Use napi's `Buffer` which can take ownership of a `Vec`
without copying into JS:

```rust
Ok(chunk.map(|b| Buffer::from(b.to_vec())))
```

Or even better — avoid the `to_vec()` entirely by using `External<Bytes>` to
share the refcounted allocation. However, `Buffer::from(Vec)` is the
simplest improvement and eliminates one of the two copies.

**Fix (write):** Use `Bytes::from(chunk.to_vec())` — same copy count but
makes the ownership transfer explicit. True zero-copy from JS → Rust is
difficult with napi since JS may GC the ArrayBuffer. The current approach is
acceptable for the write path; the read path is the bigger win.

The old iroh reference had the same double-copy. This is also documented in
the napi-rs performance guide as a common pitfall.

---

## Finding 3: 4 KB output buffer forces double FFI calls — Deno

**Severity: P1 — doubles latency for chunk reads**

**File:** `packages/iroh-http-deno/guest-ts/adapter.ts` line 71

```ts
const INITIAL_BUF = 4096;
```

The `call<T>` dispatcher allocates a 4 KB output buffer. If the response
doesn't fit, the Rust side returns a negative value indicating the required
size, and the JS side retries with a larger buffer — **two FFI calls** instead
of one.

A single `nextChunk` response for a 64 KB chunk, base64-encoded, is ~87 KB
of JSON. This always overflows the 4 KB buffer, causing every chunk read
to require two FFI calls.

**Impact:** Every `nextChunk` takes 2× the FFI latency. For a 1 MB body at
64 KB chunks, that's 16 extra FFI calls.

**Fix:** Increase to 128 KB, which accommodates the common case (64 KB chunk
base64-encoded + JSON wrapper):

```ts
const INITIAL_BUF = 128 * 1024;
```

Also consider caching and reusing the output buffer between calls instead of
allocating a new `Uint8Array` each time:

```ts
let outBuf = new Uint8Array(128 * 1024);
// In call():
if (n < 0 && -n > outBuf.byteLength) {
  outBuf = new Uint8Array(-n);  // grow permanently, never shrink
}
```

---

## Finding 4: Deno could bypass base64 entirely via raw FFI buffers

**Severity: P2 — optimization opportunity**

**File:** `packages/iroh-http-deno/guest-ts/adapter.ts` lines 106–111

Unlike Tauri (which is constrained to JSON IPC), Deno FFI supports passing raw
`Uint8Array` buffers across the boundary. The current design wraps everything
in JSON including base64-encoded binary chunks — matching the Tauri pattern
even though Deno doesn't have that limitation.

**Opportunity:** Add dedicated FFI symbols for chunk transfer that pass raw
byte pointers:

```ts
// Rust side:
#[no_mangle]
pub unsafe extern "C" fn iroh_http_next_chunk(
    handle: u32,
    out_ptr: *mut u8,
    out_cap: usize,
) -> i32;  // returns bytes written, or -(required) if too small

// Deno side:
const chunk = new Uint8Array(65536);
const n = await lib.symbols.iroh_http_next_chunk(handle, chunk, chunk.byteLength);
```

This eliminates: JSON serialization, base64 encode/decode (33% overhead),
JSON parsing, and the output buffer resize dance. The chunk data goes directly
from the Rust mpsc channel into the JS ArrayBuffer.

**Trade-off:** More FFI symbols to maintain. The JSON dispatch pattern is
simpler. This optimization is worth it if streaming throughput matters for the
Deno target (e.g. file transfer, media).

---

## Finding 5: Per-call TextEncoder allocations — Deno

**Severity: P3 — minor waste**

**File:** `packages/iroh-http-deno/guest-ts/adapter.ts` line 77

```ts
const methodBuf = enc.encode(method);
```

Every bridge call re-encodes the method name string to UTF-8. For
`nextChunk` called thousands of times during a body stream, that's thousands
of identical 9-byte allocations.

**Fix:** Pre-encode method names at module initialization:

```ts
const METHOD_BUFS = Object.fromEntries(
  ["nextChunk", "sendChunk", "finishBody", "cancelRequest",
   "nextTrailer", "sendTrailers", "rawFetch", "rawConnect",
   "serveStart", "nextRequest", "respond", "allocBodyWriter",
   "createEndpoint", "closeEndpoint", "allocFetchToken",
   "cancelInFlight"].map(m => [m, enc.encode(m)])
) as Record<string, Uint8Array>;

// In call():
const methodBuf = METHOD_BUFS[method];
```

---

## Finding 6: Sequential read/write in `pipeToWriter` — Shared

**Severity: P2 — throughput opportunity**

**File:** `packages/iroh-http-shared/src/streams.ts` lines 42–52

```ts
while (true) {
  const { value, done } = await reader.read();  // wait for source
  if (done) break;
  await bridge.sendChunk(handle, value);          // wait for sink
}
```

Each iteration waits for the previous `sendChunk` to complete before reading
the next chunk from the source stream. The source and sink could overlap —
reading chunk N+1 while chunk N is being sent.

**Impact:** For each chunk, total time = `read_latency + send_latency`
instead of `max(read_latency, send_latency)`. With IPC round-trips of ~0.5ms
per call, a 1 MB body (16 chunks) loses ~8ms to unnecessary serialization.

**Fix:** One-chunk pipeline parallelism:

```ts
let pending: Promise<void> | null = null;
while (true) {
  const { value, done } = await reader.read();
  if (pending) await pending;
  if (done) break;
  pending = bridge.sendChunk(handle, value);
}
if (pending) await pending;
```

This overlaps each read with the previous send. More complex approaches
(N-deep pipeline) aren't worth the complexity.

---

## Finding 7: Tauri `createEndpoint` sends key as `number[]` — Tauri

**Severity: P3 — minor, one-time cost**

**File:** `packages/iroh-http-tauri/guest-js/index.ts` line 221

```ts
const keyBytes: number[] | null = options?.key
    ? Array.from(options.key instanceof Uint8Array ? options.key : ...)
    : null;
```

The 32-byte secret key is sent as a JSON array of 32 numbers rather than a
base64 string. This is only called once per `createNode()` so the performance
impact is negligible, but it's inconsistent with the base64 pattern used for
chunks.

Same pattern in the Deno adapter at `adapter.ts` line 243.

**Fix:** Use `encodeBase64(keyBytes)` and decode on the Rust side. Consistent
and slightly smaller JSON payload (44 chars vs ~130 chars for 32 numbers).

---

## Summary

| # | Finding | Platform | Severity | Est. effort |
|---|---------|----------|----------|-------------|
| 1 | O(n²) base64 encode | Tauri + Deno | **P0** | 5 min each |
| 2 | Double copy per chunk | Node | **P1** | 30 min |
| 3 | 4 KB output buffer → double FFI call | Deno | **P1** | 5 min |
| 4 | Raw byte FFI bypass | Deno | **P2** | 2 hours |
| 5 | Per-call method name encoding | Deno | **P3** | 5 min |
| 6 | Sequential pipe read/write | Shared | **P2** | 30 min |
| 7 | Key as number[] not base64 | Tauri + Deno | **P3** | 10 min |

**Recommended fix order:** 1 → 3 → 5 → 2 → 6 → 7 → 4

Findings 1–3 are quick fixes that address the worst regressions. Finding 4
is a larger structural change that would give Deno significantly better
throughput than both Tauri and the old reference — but it can wait until
after launch.
