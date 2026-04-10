# iroh-http — Code Review (Patch 00)

Line-by-line review of the critical packages and crates. Each finding is
tagged with a category and severity:

- **[guideline]** — violates `guidelines.md`
- **[error]** — error handling issue
- **[memory]** — memory / allocation efficiency
- **[stream]** — streaming or backpressure concern
- **[leak]** — resource leak
- **[correctness]** — logic bug or data-loss risk
- **[security]** — security concern
- **[naming]** — naming / API surface issue
- **[perf]** — performance

Severity: **critical**, **medium**, **low**.

---

## 1. `iroh-http-shared` — TypeScript bridge layer

### 1.1 `fetch.ts` — Error classes are not web-standard

**[guideline] [error] medium**

`makeFetch` throws errors with `{ name: "AbortError" }` via `Object.assign`:

```ts
throw Object.assign(new Error("The operation was aborted"), { name: "AbortError" });
```

This produces a plain `Error` with a patched `name` property. Web-standard
code checks `err instanceof DOMException` or relies on
`new DOMException("…", "AbortError")`. Many libraries (and the browser
`fetch` spec) throw a `DOMException`, not a patched `Error`.

`guidelines.md` §1 says: _"use `DOMException` names (`AbortError`,
`NetworkError`, `TypeError`) where applicable"_.

**Recommendation:** Use `new DOMException("The operation was aborted", "AbortError")`
wherever `DOMException` is available (Node 17+, Deno, Tauri webview).
For environments that lack it, a thin polyfill (`class AbortError extends
DOMException`) in `iroh-http-shared` keeps things web-compliant.

This pattern appears twice in `fetch.ts` — on the pre-check (`signal?.aborted`)
and on the abort listener.

### 1.2 `fetch.ts` — Abort listener is never cleaned up on success

**[leak] medium**

When `signal` is provided, an abort listener is registered. It is removed
from the abort _race_ promise in the `finally` block, **but the second
listener** wired after the response is received (the one that calls
`bridge.cancelRequest(bodyHandle)`) is never removed:

```ts
signal.addEventListener("abort", () => {
  bridge.cancelRequest(bodyHandle);
});
```

If the response body is fully consumed and the fetch completes normally, this
listener remains attached to the user's `AbortSignal`. If the signal is later
aborted (for a different purpose), `cancelRequest` is called on a handle that
was already cleaned up by the EOF path in `next_chunk` (Rust removes the slab
entry on EOF). The Rust side silently ignores this (no-op), but the dangling
listener is a minor memory leak and violates the principle of cleaning up
after yourself.

**Recommendation:** After the `ReadableStream` closes (EOF), remove the
abort listener. The cleanest way is to track a removal function and call it
in the `cancel()` callback of `makeReadable` and after the final `pull`
returns `null`.

### 1.3 `fetch.ts` — `res.trailers` is a getter that creates a new Promise on every access

**[correctness] medium**

```ts
Object.defineProperty(response, "trailers", {
  get: () =>
    bridge.nextTrailer(trailersHandle).then(…),
  configurable: true,
});
```

Every access to `response.trailers` calls `bridge.nextTrailer` again. But the
trailer-receiver slab entry is removed on the first call (the Rust
`next_trailer` removes the handle from the slab after `rx.await`). The second
access will get `"invalid trailer receiver handle"` error.

**Recommendation:** Cache the Promise on first access:

```ts
let cachedTrailers: Promise<Headers> | null = null;
Object.defineProperty(response, "trailers", {
  get: () => {
    if (!cachedTrailers) {
      cachedTrailers = bridge.nextTrailer(trailersHandle).then(…);
    }
    return cachedTrailers;
  },
});
```

### 1.4 `streams.ts` — `makeReadable` has no backpressure-aware highWaterMark

**[stream] low**

`makeReadable` creates a `ReadableStream` with default `highWaterMark`:

```ts
return new ReadableStream<Uint8Array>({
  async pull(controller) { … }
});
```

The default `highWaterMark` for a byte stream is 0 in the spec (a
`ReadableStream` with an underlying source is not a byte stream unless a
`type: "bytes"` is specified). Without `type: "bytes"`, the default HWM is 1
(one chunk queued), which is fine for many cases but means every `pull` blocks
the stream until the Rust side delivers. This is correct for backpressure.

However, `pipeToWriter` calls `reader.read()` in a tight loop and awaits
`bridge.sendChunk` for each chunk. For high-throughput workloads, there is no
batching. Consider adding `type: "bytes"` to the underlying source to enable
BYOB readers on the consumer side, or document that this is intentionally
pull-based at chunk granularity.

**Recommendation:** Set `type: "bytes"` + `autoAllocateChunkSize` to leverage
the BYOB path in Deno and Node. This is a performance optimisation, not a
correctness issue.

### 1.5 `streams.ts` — `cancel()` callback does not clean up the Rust reader

**[leak] medium**

```ts
cancel() {
  // Nothing to do — the Rust side will clean up when the writer drops.
},
```

If the consumer cancels a ReadableStream (e.g. `stream.cancel()` or the
internal `ReadableStream` pipe logic cancels), the Rust reader handle is
**never freed** from the slab. It sits there until the writer side drops
(which may be never if the pump task is waiting for the reader to drain).

The `Bridge` already has `cancelRequest(handle)`. The `cancel` callback
should call it to immediately free the slab entry and notify the pump task:

```ts
cancel() {
  bridge.cancelRequest(handle);
},
```

### 1.6 `serve.ts` — Body pipe errors are silently swallowed

**[error] medium**

```ts
doPipe().catch((err) =>
  console.error("[iroh-http] response body pipe error:", err)
);
```

If the handler's `Response.body` stream errors during piping, the error is
only logged. The Rust side sees the writer drop (channel EOF) and sends the
terminal chunk, so the remote peer receives a truncated body with a valid
chunked terminator. The remote has no way to detect corruption.

**Recommendation:** When a pipe error occurs, reset the QUIC stream (via a
new `bridge.resetStream(handle)` method or similar) so the remote receives a
transport-level error rather than a valid-looking truncated response.

### 1.7 `serve.ts` — Duplex mode calls `makeReadable(bridge, payload.reqBodyHandle)` twice

**[correctness] medium**

For duplex requests, a `ReadableStream` for the request body is created in
the main path:

```ts
const reqBody = hasBody ? makeReadable(bridge, payload.reqBodyHandle) : null;
```

Then inside the `duplexFn`:

```ts
readable: makeReadable(bridge, payload.reqBodyHandle),
```

Two `ReadableStream` objects both pull from the same Rust reader handle. This
is a race condition — depending on which stream's `pull` runs first, chunks
will be split between them unpredictably. For `POST` / `PUT` duplex requests
(where `hasBody` is true), the request body AND the duplex readable both
consume the same handle.

**Recommendation:** In duplex mode, do not create the initial `reqBody`
`ReadableStream`. The handler should use `req.duplex().readable` exclusively.
Guard with:
```ts
const reqBody = (hasBody && !payload.isDuplex)
  ? makeReadable(bridge, payload.reqBodyHandle)
  : null;
```

### 1.8 `bridge.ts` — Public API exposes `[string, string][]` headers

**[guideline] low**

`guidelines.md` §1 says: _"use the `Headers` class; not a `[string, string][]`
array at the API boundary"_. The internal `FfiRequest`, `FfiResponseHead`,
`RequestPayload` types use `[string, string][]`. This is acceptable for
internal types, but the `Bridge` interface itself uses `[string, string][]`
for `sendTrailers` and `nextTrailer`.

These are internal interfaces, so this is okay. But worth checking that none
of these tuple arrays leak into the public `IrohNode` / `serve` / `fetch`
signatures. Currently they don't — `makeServe` and `makeFetch` convert to
`Headers` at the boundary.

**Status:** Compliant. The internal types correctly stay internal.

---

## 2. `iroh-http-core` — Rust crate

### 2.1 `stream.rs` — `next_chunk` clones Arc on every call

**[perf] low**

```rust
let rx_arc = {
    let slab = reader_slab().lock().unwrap();
    slab.get(handle as usize)
        .ok_or_else(|| format!("invalid reader handle: {handle}"))?
        .rx
        .clone()
};
```

Every `next_chunk` call acquires the slab mutex and clones the `Arc`. This is
a pointer-sized atomic increment, so it's cheap. But for streaming hot paths
(100k+ chunks per second), this means the slab mutex is contended on every
chunk. Consider caching the `Arc` on the JS side (passing it back in), but
this would break the handle abstraction. Acceptable for now.

**Status:** Acceptable. The `std::sync::Mutex` + `Arc::clone` pattern is the
standard approach for slab-based handle designs.

### 2.2 `stream.rs` — `cancel_reader` drops while another task holds the Arc

**[correctness] low**

`cancel_reader` removes the entry from the slab:

```rust
pub fn cancel_reader(handle: u32) {
    let mut slab = reader_slab().lock().unwrap();
    if slab.contains(handle as usize) {
        slab.remove(handle as usize);
    }
}
```

If a `next_chunk` call is currently awaiting on the `tokio::sync::Mutex`
(which it holds via a cloned `Arc`), the slab removal doesn't actually drop
the `mpsc::Receiver` — the `Arc` keeps it alive. The in-flight `recv().await`
will eventually complete normally. This means cancellation is **lazy**, not
immediate. The docstring says _"causing any pending `nextChunk` to return an
error"_ — this is misleading. It returns the next queued chunk (if any), not
an error.

True immediate cancellation would require closing the `mpsc::Receiver`
directly (e.g. via a `tokio::sync::Notify` or `CancellationToken`).

**Recommendation:** Either fix the doc comment to say cancellation is lazy, or
implement proper cancellation by wrapping the receiver in a
`tokio::select!` with a `CancellationToken`.

### 2.3 `stream.rs` — `Bytes::copy_from_slice` allocates on every chunk

**[memory] medium**

In `pump_recv_to_body`, `pump_recv_raw_to_body`, and `pump_stream_to_body`:

```rust
let data = Bytes::copy_from_slice(&buf[header_len..data_end]);
```

This copies the data out of the buffer into a new `Bytes` allocation. Quinn's
`read_chunk` returns owned `Bytes` (zero-copy from the QUIC stack), but the
chunked-encoding loop copies sub-slices after parsing. For the non-chunked
duplex path, this could be avoided by passing Quinn's `Bytes` directly.

For the chunked path: consider using `Bytes::from(buf.split_to(n))` from a
`BytesMut` accumulator instead of `Vec<u8>`, which would avoid the copy.

**Recommendation:** Use `BytesMut` as the accumulation buffer and `freeze()`
/ `split_to()` to create zero-copy `Bytes` slices. This is meaningful for
large body transfers.

### 2.4 `client.rs` — `pump_body_to_stream` is called inline (not spawned)

**[stream] medium**

In `do_request`:

```rust
if let Some(reader) = req_body_reader {
    pump_body_to_stream(reader, &mut send, true, None).await?;
}
send.finish().map_err(|e| format!("finish send: {e}"))?;
```

The request body is pumped to completion **before** reading the response head.
This means for bodies larger than the QUIC send buffer, the client blocks
waiting for the server to ACK body chunks before reading any response. If the
server wants to start responding before the request body is fully received
(e.g., early 400/413 error), there's a deadlock: the server sends a response
but the client isn't reading it; the client sends body but the server isn't
reading it (because it already sent a response).

This is how HTTP/1.1-over-TCP works (half-duplex), but QUIC streams are
full-duplex. The pump should be spawned as a task (or driven via `tokio::select!`
alongside response-head reading) as it is done on the server side.

**Recommendation:** Spawn the request body pump as a background task and await
the response head concurrently. This matches what the server side already does.

### 2.5 `client.rs` — `looks_like_chunk_header` is a heuristic, not authoritative

**[correctness] low**

```rust
fn looks_like_chunk_header(buf: &[u8]) -> bool {
    for &b in buf.iter().take(10) {
        if b == b'\r' { return true; }
        if !(b.is_ascii_hexdigit()) { return false; }
    }
    false
}
```

A raw (non-chunked) body that happens to start with hex digits followed by
`\r` (e.g. `"CAFE\r..."`) will be misclassified as chunked. Since iroh-http
always sends `Transfer-Encoding: chunked`, the response head should be the
source of truth.

In `pump_stream_to_body`, `chunked_mode` should be derived from the response
headers (the `Transfer-Encoding` header), not from a heuristic on the first
bytes.

**Recommendation:** Pass a `chunked: bool` flag from `do_request` (where the
response headers are available) into `pump_stream_to_body`.

### 2.6 `client.rs` — Duplicated `read_trailers_from_buf` functions

**[correctness] low**

Both `client.rs` and `server.rs` define their own `read_trailers_from_buf`
with identical logic. This should be extracted to a shared function (e.g. in
`stream.rs` or a private `io.rs` module).

### 2.7 `server.rs` — `NEXT_REQ_HANDLE` is an `AtomicU32` that can wrap around

**[correctness] low**

```rust
static NEXT_REQ_HANDLE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(1);
```

After 4 billion requests, this wraps to 0, and handle collisions start. In
practice this will never happen in a single process lifetime. But using a slab
(like the body/writer handles do) would make it consistent and avoid the
theoretical issue.

### 2.8 `server.rs` — `ServeOptions` is ignored by the serve function

**[correctness] low**

`raw_serve` in the napi layer always passes `ServeOptions::default()`:

```rust
iroh_http_core::serve(ep, ServeOptions::default(), …)
```

The Tauri side does the same. The `options` parameter from JS is never
forwarded. The `max_concurrency` option exists in Rust but is inaccessible
from JS.

### 2.9 `endpoint.rs` — `address_lookup` calls are not conditional

**[correctness] low**

```rust
let mut builder = Endpoint::empty_builder(relay_mode)
    .address_lookup(PkarrPublisher::n0_dns())
    .address_lookup(DnsAddressLookup::n0_dns())
    .alpns(alpns);
```

`PkarrPublisher` and `DnsAddressLookup` are always added (using n0 DNS). Even
when the user provides a custom `dns_discovery` string, these defaults are
still registered. The `dns_discovery` option appears to be unused.

### 2.10 `server.rs` — `serve` returns a `JoinHandle` that nobody awaits or stores

**[leak] low**

The `serve` function returns a `JoinHandle<()>` but both the napi and Tauri
callers ignore it:

```rust
// napi
iroh_http_core::serve(ep, ServeOptions::default(), …);  // JoinHandle dropped
```

Dropping a `JoinHandle` detaches the task (it runs forever). This is probably
intentional (serve runs until the endpoint closes), but it means there is no
way to gracefully shut down the serve loop or detect when it exits.

---

## 3. `iroh-http-node` — napi-rs bindings

### 3.1 `lib.rs` — `js_finish_body` and `js_cancel_request` are sync, not async

**[correctness] low**

In `index.ts`:

```ts
finishBody: (handle: number) => {
    jsFinishBody(handle);
    return Promise.resolve();
},
cancelRequest: (handle: number) => {
    jsCancelRequest(handle);
    return Promise.resolve();
},
```

These call synchronous napi functions and wrap the result in
`Promise.resolve()`. The Rust functions (`finish_body`, `cancel_reader`) are
also synchronous (they only acquire a `std::sync::Mutex` briefly). This is
fine, but the `Bridge` interface declares them as `Promise<void>`. The
mismatch between sync execution and async interface is intentional (for Tauri
where they're async invoke calls) — acceptable.

### 3.2 `lib.rs` — `raw_serve` drops the `JoinHandle` and never passes options

**[leak] low**

Same as §2.8 / §2.10.

### 3.3 `lib.rs` — `create_endpoint` unwinds on `.unwrap()` in slab access

**[error] low**

All slab accesses use `.lock().unwrap()`. If any panic occurs while a slab
mutex is held, the mutex is poisoned and all subsequent calls will panic
(bringing down the Node.js process). Using `.lock().expect("...")` with a
meaningful message would aid debugging, but the real fix is that panics in
napi code become fatal process errors anyway.

**Status:** Acceptable for napi context; unrecoverable panics are the norm.

### 3.4 `lib.rs` — `raw_serve` callback shape mismatch

**[correctness] medium**

The napi `raw_serve` callback constructs a JS object but does not include the
new fields from patch 01:

```rust
obj.set("reqTrailersHandle", …)?  // missing
obj.set("resTrailersHandle", …)?  // missing
obj.set("isDuplex", …)?           // missing
```

Wait — let me re-check. Reading more carefully, I see the napi code creates
an object with these fields:

```rust
obj.set("reqHandle", …)?;
obj.set("reqBodyHandle", …)?;
obj.set("resBodyHandle", …)?;
obj.set("method", …)?;
obj.set("url", …)?;
obj.set("remoteNodeId", …)?;
obj.set("headers", …)?;
```

The `reqTrailersHandle`, `resTrailersHandle`, and `isDuplex` fields from
`RequestPayload` are **not forwarded to JS**. The TypeScript side
(`RequestPayload` in `bridge.ts`) expects them. This means:

- `payload.reqTrailersHandle` → `undefined` in JS → trailer reads silently fail
- `payload.resTrailersHandle` → `undefined` → trailer sends silently fail
- `payload.isDuplex` → `undefined` → duplex detection is broken

**This is a critical bug.** The serve callback is missing three fields.

**Recommendation:** Add the missing fields to the napi callback serialisation.

---

## 4. `iroh-http-tauri` — Tauri plugin

### 4.1 `commands.rs` — `send_chunk` copies the full body chunk via `Vec<u8>`

**[memory] medium**

```rust
pub async fn send_chunk(handle: u32, chunk: Vec<u8>) -> Result<(), String> {
    iroh_http_core::stream::send_chunk(handle, Bytes::from(chunk)).await
}
```

The Tauri invoke deserialiser produces a `Vec<u8>` from the JSON array of
numbers. On the JS side:

```ts
chunk: Array.from(chunk),
```

Every chunk is serialised as a JSON array of numbers (`[72, 101, 108, …]`),
transmitted via IPC, deserialised to `Vec<u8>`, then converted to `Bytes`.
For large payloads this is extremely expensive — each byte takes 1–3 JSON
characters (plus commas), so a 64KB chunk becomes ~250KB of JSON.

This is an inherent limitation of the Tauri `invoke()` IPC (no binary
support without raw IPC plugins), so there is no easy fix within the current
architecture. But it should be documented as a known performance bottleneck
for Tauri targets.

**Recommendation:** Consider Tauri's raw request/response IPC or a custom
binary protocol for chunk transfer. Alternatively, base64-encode chunks
(33% overhead vs 300%+ JSON overhead).

### 4.2 `commands.rs` — `next_chunk` returns `Option<Vec<u8>>`

**[memory] low**

Same issue as §4.1 in the other direction. Every chunk from Rust to JS goes
through JSON serialisation as a number array. Base64 would be significantly
more efficient.

### 4.3 `guest-js/index.ts` — `rawServe` error handler ignores second `invoke` failure

**[error] low**

```ts
await invoke(`${PLUGIN}|respond_to_request`, {
  args: { reqHandle: raw.reqHandle, status: 500, headers: [] },
}).catch(() => {/* ignore */});
```

If the fallback 500 response also fails, the error is silently eaten. The
QUIC stream will eventually time out, but there's no logging. Adding a
`console.warn` would help debugging.

---

## 5. `iroh-http-framing` — no_std crate

### 5.1 `lib.rs` — Fixed header buffer of 64

**[correctness] low**

```rust
let mut headers_buf = [httparse::EMPTY_HEADER; 64];
```

Both `parse_request_head` and `parse_response_head` allow a maximum of 64
headers. If a request/response has more than 64 headers, `httparse` returns
`TooManyHeaders`. This is probably fine for real-world usage, but it's good to
document the limit. It could also be a minor DoS vector — a malicious peer
sending 65+ headers causes a parse error.

### 5.2 `lib.rs` — ALPN constants are duplicated between core and framing

**[correctness] low**

`iroh-http-framing` defines `ALPN_BASE`, `ALPN_DUPLEX`, `ALPN_TRAILERS`,
`ALPN_FULL`. `iroh-http-core` defines `ALPN`, `ALPN_DUPLEX`, `ALPN_TRAILERS`,
`ALPN_FULL`. These are the same values with slightly different names. A
disagreement between the two would cause ALPN negotiation failures.

**Recommendation:** Core should re-export from framing, or framing should not
define them (since framing is no_std and doesn't do ALPN negotiation).

---

## 6. `iroh-http-deno` — Deno FFI crate (in progress)

### 6.1 `lib.rs` — `iroh_http_call` uses `block_on` inside a `nonblocking` FFI call

**[perf] medium**

```rust
let response = runtime().block_on(dispatch::dispatch(method, payload));
```

Deno's `nonblocking: true` runs the FFI call on a thread pool. `block_on`
then blocks that thread while the Tokio runtime executes the future. This
means each concurrent FFI call pins a thread pool thread. Under high
concurrency (many simultaneous `nextChunk` calls from parallel streams), the
Deno thread pool can be exhausted. The old reference implementation had the
same pattern, so this is a known design trade-off.

**Recommendation:** Document the concurrency limitation. If it becomes a
problem, the entry point could spawn a Tokio task and use a different
mechanism (e.g., a completion callback) to deliver the result.

### 6.2 `lib.rs` — Missing `dispatch.rs` module

**[correctness] critical**

`lib.rs` declares `mod dispatch;` but `dispatch.rs` does not exist yet. The
crate won't compile. This is expected since the package is in progress, noted
here for completeness.

### 6.3 `serve_registry.rs` — `ServeQueue` holds both tx and rx, never accessed concurrently

**[correctness] low**

The `ServeQueue` struct holds both `tx` and `rx`. The `tx` is used by the
Rust serve loop; the `rx` is used by the TypeScript polling via
`nextRequest`. Both are behind an `Arc`, which is correct. But it might be
cleaner to store them separately — the serve loop only needs `tx`, the polling
path only needs `rx`. Currently no issue, but the combined struct makes
ownership semantics less clear.

---

## 7. Guidelines compliance summary

| Guideline | Status | Notes |
|---|---|---|
| §1 Fetch/Request/Response as-is | **compliant** | serve builds standard `Request`, fetch returns standard `Response` |
| §1 ReadableStream/WritableStream | **compliant** | all streaming uses web stream types |
| §1 AbortSignal | **mostly compliant** | works, but error class is wrong (§1.1), listener leak (§1.2) |
| §1 Headers class at boundary | **compliant** | internal types use tuples, public API uses Headers |
| §1 DOMException error names | **non-compliant** | patched `Error` objects instead of `DOMException` (§1.1) |
| §1 WebTransport naming | **non-compliant** | still uses `DuplexStream` / `connect()` / `req.duplex()` (patch 04 pending) |
| §3 Minimal surface | **compliant** | small API surface |
| §4 Security — node-id injection | **compliant** | injected by Rust, stripped from incoming headers |
| §5 Naming | **compliant** | JS camelCase, types PascalCase |

---

## 8. Priority ranking

| # | Finding | Severity | Section |
|---|---|---|---|
| 1 | napi `raw_serve` missing 3 fields (trailers + duplex) | critical | §3.4 |
| 2 | `fetch.ts` duplicate ReadableStream on duplex reqBody | medium | §1.7 |
| 3 | `fetch.ts` trailers getter creates new Promise each access | medium | §1.3 |
| 4 | `client.rs` request body pump blocks response reading | medium | §2.4 |
| 5 | `client.rs` chunked detection via heuristic | medium | §2.5 |
| 6 | `stream.rs` / pump functions copy via `Bytes::copy_from_slice` | medium | §2.3 |
| 7 | AbortError uses patched Error, not DOMException | medium | §1.1 |
| 8 | Abort listener leak after body fully consumed | medium | §1.2 |
| 9 | `makeReadable` cancel callback is a no-op | medium | §1.5 |
| 10 | `serve.ts` pipe errors silently swallowed | medium | §1.6 |
| 11 | Tauri chunk serialisation via JSON number arrays | medium | §4.1 |
| 12 | Deno `dispatch.rs` missing (expected, in progress) | critical | §6.2 |
