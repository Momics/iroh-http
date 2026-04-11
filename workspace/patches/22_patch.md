---
status: pending
refs: features/webtransport.md
---

# Patch 22 — Bidirectional Streams on `IrohSession`

Verify and complete `IrohSession.createBidirectionalStream()` across all four
platform adapters as part of the WebTransport alignment in
[webtransport.md](../features/webtransport.md).

## Problem

`IrohSession.createBidirectionalStream()` is the correct home for raw bidi
streams. `node.createBidirectionalStream(peer, path)` created a second, weaker
mental model. Raw bidi streams are accessed via `node.connect(peer)` →
`IrohSession` instead. HTTP-routed communication (with URL paths) continues to
use `node.fetch` / `node.serve`.

## Changes

### 1. Remove `createBidirectionalStream` from `IrohNode`

```ts
// Remove from IrohNode interface:
createBidirectionalStream(peer: string | NodeAddr, path: string, init?: ...): Promise<BidirectionalStream>
// ↑ deleted
```

### 2. `IrohSession.createBidirectionalStream()` — verify and complete

For each of the four adapters, verify:

- [ ] `session.createBidirectionalStream()` returns a
  `WebTransportBidirectionalStream` with working `.readable` and `.writable`.
- [ ] `session.incomingBidirectionalStreams` is a
  `ReadableStream<WebTransportBidirectionalStream>` that yields streams as the
  remote opens them.
- [ ] Backpressure: writing faster than the reader reads causes the writer to
  yield, not buffer unboundedly.
- [ ] The stream closes cleanly when both sides finish.
- [ ] Both sides are aborted correctly when the session closes.

### 3. Type alignment

```ts
// WebTransportBidirectionalStream (spec type):
interface WebTransportBidirectionalStream {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
}
```

No custom stream wrapper types. Both sides must pass through `pipeTo`,
`pipeThrough`, `tee`, `getReader()`, and `getWriter()` without modification.

### 4. Integration tests — `iroh-http-core/tests/`

Add `bidi_stream.rs`:

```rust
#[tokio::test]
async fn session_bidi_stream_round_trip() {
    let (a, b) = two_test_nodes().await;
    let session_a = bridge::connect(a.handle, &b.public_key()).await.unwrap();
    let stream = bridge::session_create_bidi_stream(session_a).await.unwrap();
    // Write N chunks from A, read on B via incomingBidirectionalStreams
    // Verify data, order, and clean close
}

#[tokio::test]
async fn session_bidi_stream_backpressure() {
    // Write many chunks without reading; assert no unbounded memory growth
}
```

### 5. Python adapter

Python exposes the session bidi stream as an async generator / async context
manager rather than a web stream:

```python
session = await node.connect(peer)
stream = await session.create_bidirectional_stream()
await stream.write(b"hello")
async for chunk in stream:
    process(chunk)
await stream.close()
```

## Files

- `packages/iroh-http-shared/src/index.ts` — remove `createBidirectionalStream` from `IrohNode`
- `packages/iroh-http-shared/src/session.ts` — verify `IrohSession.createBidirectionalStream`
- `crates/iroh-http-core/src/bridge.rs` — verify `session_create_bidi_stream` + `session_next_bidi_stream`
- `crates/iroh-http-core/tests/bidi_stream.rs` — new integration tests
- All four adapter packages — wire `session_create_bidi_stream`, `session_next_bidi_stream`

- `packages/iroh-http-deno/src/` — FFI bidi stream implementation
- `packages/iroh-http-tauri/src/` — invoke bridge bidi stream implementation
- `packages/iroh-http-py/src/` — Python bidi stream implementation
