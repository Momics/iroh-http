---
status: not-implemented
scope: core — adapter audit
priority: high
---

# Feature: Bidirectional Stream Exposure (Audit + Completion)

## What

`createBidirectionalStream` is defined on the `IrohNode` interface in
`iroh-http-shared` but has not been verified to be fully implemented and
correctly exposed across all four platform adapters: Node.js (napi), Deno
(FFI), Tauri (invoke bridge), and Python.

## Why

Bidirectional streams are the natural primitive for persistent, stateful
connections — chat, collaborative editing, real-time telemetry, RPC. Without
a working bidi stream, consumers must simulate duplex communication with pairs
of `fetch` / `serve` calls, which is awkward, adds latency, and breaks the
mental model.

The old `iroh` package had `IrohBidirectionalStream` with `readable` and
`writable` sides as web-standard streams, full stats, and reset support.
iroh-http defines the API but the implementation completeness across adapters
is unverified.

## Required Work

### 1. Audit each adapter

For each of the four adapters, verify:

- [ ] `createBidirectionalStream(nodeId, path, init?)` is implemented and
  returns a `BidirectionalStream` with working `readable` (ReadableStream) and
  `writable` (WritableStream) sides.
- [ ] The serve handler receives incoming bidi streams correctly (the serve
  path must handle `*-duplex` ALPNs).
- [ ] Stream backpressure works: writing faster than the reader drains causes
  the writer to yield rather than buffer unboundedly.
- [ ] `AbortSignal` cancels both sides of the stream correctly.
- [ ] The stream is closed gracefully when both sides finish.

### 2. Align with web standard types

```ts
interface BidirectionalStream {
  /** Bytes from the remote, as a web-standard ReadableStream<Uint8Array>. */
  readable: ReadableStream<Uint8Array>;
  /** Bytes to the remote, as a web-standard WritableStream<Uint8Array>. */
  writable: WritableStream<Uint8Array>;
}
```

No custom stream types. Both sides must work with `pipeTo`, `pipeThrough`,
`getReader()`, `getWriter()`, and all standard stream combinators.

### 3. Add integration tests

Add a test in `iroh-http-core/tests/` that:

1. Opens a bidi stream from node A to node B.
2. Writes `n` chunks from A; reads them on B.
3. Writes `m` chunks from B; reads them on A.
4. Verifies both sides receive correct data, in order, with no corruption.

### 4. Expose on the `IrohNode` interface clearly

Patch 17 noted that `createBidirectionalStream` exists but may not be
prominently documented. It should appear as a first-class method alongside
`fetch` and `serve` in the README and JSDoc examples.

## Notes

- The ALPN negotiation for duplex streams (`iroh-http/1-duplex`,
  `iroh-http/1-full`) is already planned. This audit ensures the JS surface
  is correct once the ALPN work lands.
- Python bidi streams are a separate sub-task; the async iterator pattern
  differs from the web stream API.
