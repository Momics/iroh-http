---
status: integrated
---

# iroh-http — Patch 04: WebTransport-Style API Alignment

This document proposes changes to align the iroh-http public surface with the
**WebTransport** API (MDN / WHATWG Living Standard). The goal is not full
compliance with the WebTransport spec — the transport layer is QUIC over Iroh,
not HTTPS — but the JS API should feel native and unsurprising to anyone
familiar with WebTransport.

---

## Background

The current `DuplexStream` interface already matches the shape of
`WebTransportBidirectionalStream`:

```ts
// WebTransport spec — WebTransportBidirectionalStream
{ readable: ReadableStream<Uint8Array>; writable: WritableStream<Uint8Array> }

// iroh-http — DuplexStream (current)
{ readable: ReadableStream<Uint8Array>; writable: WritableStream<Uint8Array> }
```

The shape is identical. What does not align yet:

1. **Method naming.** `node.connect()` is informal. The WebTransport equivalent
   is `session.createBidirectionalStream()`.

2. **No per-peer session object.** WebTransport represents a connection to one
   server as a `WebTransportSession`. iroh-http has no equivalent — there is
   only the node-global `connect()` / `serve()`. A session object would let
   callers manage per-peer state in a familiar way.

3. **Server-push streams.** WebTransport exposes
   `session.incomingBidirectionalStreams` — a
   `ReadableStream<WebTransportBidirectionalStream>` that the server can push
   new streams into at any time. The current `serve()` model only supports
   request/response or handler-initiated duplex; it has no async-iterable
   equivalent for server-pushed streams.

4. **Session lifecycle.** WebTransport has `session.ready` (Promise), and
   `session.closed` (Promise with close info). iroh-http nodes close via
   `node.close()` but there is no `closed` promise that external code can
   await.

5. **Type naming.** `DuplexStream` is a local custom name. Aligning with the
   spec name `BidirectionalStream` (or exporting an alias) makes the API
   immediately legible to WebTransport users.

---

## Proposed Changes

### 1. Rename `DuplexStream` → `BidirectionalStream`

The interface shape stays exactly the same. Only the exported type name changes.
All existing internal usage of `req.duplex()` and `node.connect()` is updated
to use the new name.

The old name is kept as a deprecated re-export for one cycle:
```ts
/** @deprecated Use `BidirectionalStream` instead. */
export type DuplexStream = BidirectionalStream;
```

### 2. Rename `node.connect()` → `node.createBidirectionalStream()`

```ts
// Before
connect(nodeId: string, path: string, init?: RequestInit): Promise<DuplexStream>

// After
createBidirectionalStream(
  nodeId: string,
  path: string,
  init?: RequestInit
): Promise<BidirectionalStream>
```

This mirrors WebTransport exactly (the `nodeId` parameter is unavoidable since
iroh-http addresses peers by key, not by URL hostname).

### 3. Rename `req.duplex()` → `req.acceptWebTransport()`

On the server side, the method that promotes an HTTP request into a duplex stream
currently is `req.duplex()`. Aligning with the intent of an "upgrade" makes this
clearer:

```ts
// Before
const { readable, writable } = req.duplex();

// After
const { readable, writable } = req.acceptWebTransport();
```

The WebTransport spec calls this pattern "session establishment". The method
name reflects that the server is accepting an upgrade to the bidirectional
streaming protocol.

### 4. Add `IrohNode.closed` promise

```ts
interface IrohNode {
  // ... existing members ...

  /**
   * Resolves when the node has been closed (either via `close()` or due to
   * a fatal error).  Mirrors `WebTransportSession.closed`.
   */
  readonly closed: Promise<void>;
}
```

The Rust side sends on a `oneshot` when the endpoint shuts down; the bridge
wraps it in a JS `Promise` and attaches it to the node object when `buildNode`
is called.

### 5. (Future / non-breaking) `PeerSession` object

This is called out as a future direction rather than an immediate requirement.

A `PeerSession` object could be introduced that mirrors `WebTransportSession`
for a specific remote peer:

```ts
interface PeerSession {
  readonly peerId: string;
  readonly ready: Promise<void>;
  readonly closed: Promise<void>;
  close(): void;
  createBidirectionalStream(path: string, init?: RequestInit): Promise<BidirectionalStream>;
  /** Streams pushed by the peer (server-initiated). */
  readonly incomingBidirectionalStreams: ReadableStream<BidirectionalStream>;
}
```

`node.connect(peerId)` (without a `path`) would then return a `PeerSession`
rather than a single stream, and `createBidirectionalStream` on the session
would open individual streams within that logical connection. This is a bigger
API surface change and is deferred.

---

## Changes Required

| Layer | Change |
|---|---|
| `iroh-http-shared` / `bridge.ts` | Rename `DuplexStream` → `BidirectionalStream`; add deprecated alias |
| `iroh-http-shared` / `bridge.ts` | Rename `connect` → `createBidirectionalStream` on `IrohNode`; add `closed` |
| `iroh-http-shared` / `serve.ts` | Rename `req.duplex()` → `req.acceptWebTransport()` |
| `iroh-http-shared` / `index.ts` | Export `BidirectionalStream`; re-export `DuplexStream` as deprecated |
| `iroh-http-shared` / `fetch.ts` | Rename `makeConnect` internals to match |
| `iroh-http-node` / `index.ts` | Update call sites |
| `iroh-http-tauri` / `guest-js/index.ts` | Update call sites |
| `iroh-http-deno` / `guest-ts/adapter.ts` | Update call sites |
| `iroh-http-core` / `lib.rs` | No functional change; `is_duplex` field rename to `is_bidi` for consistency |
| Docs | Update `00_brief.md` layer diagram method names |
