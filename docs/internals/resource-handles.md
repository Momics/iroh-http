# Resource Handles

All runtime resources in iroh-http-core — body streams, trailer channels, fetch cancellation tokens, sessions, and pending request heads — are referenced by opaque `u64` handles at the FFI boundary. This document explains the handle system.

---

## Why u64 handles

Platform FFI boundaries (napi-rs, Deno, PyO3, Tauri) cannot safely hold Rust references or `Arc` values. Instead, the platform side holds an integer key and calls back into Rust with that key. Rust looks up the corresponding resource and performs the operation.

The handle is a `u64` because that is the natural FFI type for a 64-bit integer on all supported platforms, and because the underlying slotmap key is a `u64` (`KeyData::as_ffi()`).

> In TypeScript/JavaScript, these are `bigint` values (not `number`) since `number` has only 53 bits of integer precision.

---

## Generational slotmap keys

Handles are produced by `slotmap`. Each slotmap key encodes:

- **32-bit index** — slot position in the backing array
- **32-bit generation** — incremented each time a slot is reused

When a resource is freed (e.g. `finish_body(handle)` drops the `BodyReader`), the slot's generation is incremented. Any subsequent call with the old handle finds a mismatched generation and returns `Err("invalid handle: …")` — no panic, no use-after-free, no silent wrong-resource access.

This makes handle invalidation automatic and cheap. There is no additional bookkeeping needed.

---

## Handle types

`stream.rs` defines seven distinct slotmap key types and one registry per type:

| Key type | Resource | Produced by | Consumed/freed by |
|----------|----------|-------------|-------------------|
| `ReaderKey` | `BodyReader` (mpsc receiver) | `insert_reader` | `next_chunk` (EOF auto-removes), `cancel_reader` |
| `WriterKey` | `BodyWriter` (mpsc sender) | `insert_writer` | `finish_body` (drops writer) |
| `TrailerSenderKey` | `oneshot::Sender<Vec<(String,String)>>` | `insert_trailer_sender` | `send_trailers` (fires and removes) |
| `TrailerReceiverKey` | `oneshot::Receiver<Vec<(String,String)>>` | `insert_trailer_receiver` | `next_trailer` (awaits and removes) |
| `FetchCancelKey` | `CancellationToken` | `alloc_fetch_token` | `cancel_in_flight`, `remove_fetch_token` |
| `SessionKey` | `SessionEntry` (QUIC session state) | `insert_session` | `remove_session` |
| `RequestHeadKey` | `oneshot::Sender<ResponseHeadEntry>` | `allocate_req_handle` | `take_req_sender` (called by `respond()`) |

Each registry is a `OnceLock<Mutex<SlotMap<K, Entry<T>>>>` — a process-global, lazily initialised, mutex-protected slotmap.

---

## Endpoint association

Every entry stores the `ep_idx` of the endpoint that created it:

```rust
struct Entry<T> {
    ep_idx: u32,
    value: T,
}
```

When an endpoint is closed (`unregister_endpoint(ep_idx)`), all seven registries sweep their entries via `.retain(|_, e| e.ep_idx != ep_idx)`. This prevents orphaned handles from accumulating when an endpoint is destroyed.

---

## Handle lifecycle — typical request

```
serve loop                       JS handler
   │                                │
   │  insert_reader → req_body_handle ──────────────────► nextChunk(req_body_handle)
   │  insert_writer → res_body_handle ──────────────────► sendChunk(res_body_handle, …)
   │  insert_trailer_receiver → req_trailers_handle ────► nextTrailer(req_trailers_handle)
   │  insert_trailer_sender   → res_trailers_handle ────► sendTrailers(res_trailers_handle, …)
   │  allocate_req_handle → req_handle ─────────────────► respond(req_handle, 200, […])
   │                                │
   │  head_rx.await ◄─────── take_req_sender fires ◄────── respond() called
   │  (serves response)
   │
   │  body_from_reader(res_body_reader) ◄── JS called sendChunk / finish_body
```

All five handles are created before the JS callback fires, so JS always receives a fully valid set of handles.

---

## Fetch cancellation tokens

`alloc_fetch_token(ep_idx)` allocates a `FetchCancelKey` backed by a `CancellationToken`. The token is passed to `fetch()` as an optional argument. Calling `cancel_in_flight(token)` fires the token, which races against the in-progress HTTP exchange and returns `Err("aborted")` if it wins.

Tokens are automatically swept when `unregister_endpoint` runs, preventing leaks if a token is allocated but never explicitly freed.

---

## Thread safety

All slotmap operations lock the per-registry `Mutex`. This is a short critical section (insert/remove/lookup), so contention is negligible under normal workloads. The `OnceLock` wrapper ensures each registry is initialised exactly once at first use.

---

## Debugging stale handles

If a function returns `Err("invalid handle: N")` it means either:

1. The handle was already freed (generation mismatch) — likely a double-free or use-after-free bug in the adapter
2. The handle was never valid (wrong type passed) — likely a misrouted call
3. The endpoint was closed and all its handles were swept — legitimate if the node was closed while a request was in flight

The generation in the handle key makes (1) reliable: the old handle can never silently alias a new resource.
