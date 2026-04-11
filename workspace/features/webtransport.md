# WebTransport Compatibility + Datagrams

iroh-http's transport is QUIC-native, encrypted, and peer-authenticated.
`IrohSession` implements the
[WebTransport API](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport)
exactly, giving access to bidirectional streams, unidirectional streams, and
unreliable datagrams — all under a familiar interface.

## `IrohNode` vs `IrohSession`

`IrohNode` is a **multi-peer local endpoint** — one node talks to many peers
simultaneously. `WebTransport` in the browser is a **per-session** object,
scoped to one remote peer.

The mapping:

| Concept | iroh-http | Browser WebTransport |
|---|---|---|
| Local endpoint | `IrohNode` | Browser's QUIC stack |
| Session to one peer | `IrohSession` | `WebTransport` instance |
| HTTP request | `node.fetch(peer, url)` | `fetch(url)` over H3 |
| HTTP server | `node.serve(handler)` | Server-side WT handler |

## Opening a session

```ts
const session: IrohSession = await node.connect(peer);

// IrohSession satisfies the full WebTransport interface:
await session.ready;
session.datagrams.writable.getWriter().write(new Uint8Array([1, 2, 3]));

const stream = await session.createBidirectionalStream();
// stream.readable, stream.writable

for await (const stream of session.incomingBidirectionalStreams) {
  handleStream(stream);
}

session.close({ closeCode: 0, reason: 'done' });
await session.closed;
```

## `IrohSession` interface

`IrohSession` satisfies `WebTransport` exactly:

```ts
interface IrohSession extends WebTransport {
  // All standard WebTransport properties, plus:

  /** The peer's verified public key. Not on standard WebTransport. */
  readonly remoteId: PublicKey;
}
```

## `node.fetch` and `node.serve`

`node.fetch(peer, url, init)` is a convenience method that internally calls
`node.connect(peer)`, opens a bidirectional stream, sends an HTTP request over
it, and returns a standard `Response`. Sessions are managed transparently by
the connection pool — the caller sees no difference.

`node.serve` accepts incoming sessions and routes them to the HTTP handler.
The HTTP layer is built on top of raw sessions; raw sessions are also directly
accessible via `node.connect`.

---

## `WebTransport` property map

| Property | `IrohSession` implementation | Notes |
|---|---|---|
| `ready` | Resolves when QUIC handshake completes | Trivial |
| `closed` | `Promise<{closeCode, reason}>` | |
| `datagrams` | `IrohDatagramDuplexStream` (see below) | |
| `createBidirectionalStream()` | Opens a QUIC bidi stream | |
| `createUnidirectionalStream()` | Opens a QUIC send-only stream | |
| `incomingBidirectionalStreams` | `ReadableStream<WebTransportBidirectionalStream>` | |
| `incomingUnidirectionalStreams` | `ReadableStream<ReadableStream<Uint8Array>>` | |
| `close(info?)` | Graceful QUIC connection close | |

## Bidirectional streams

```ts
const stream = await session.createBidirectionalStream();

// Write to the remote:
const writer = stream.writable.getWriter();
await writer.write(new TextEncoder().encode('hello'));
await writer.close();

// Read from the remote:
for await (const chunk of stream.readable) {
  console.log(new TextDecoder().decode(chunk));
}
```

Both sides are fully spec-compliant `ReadableStream<Uint8Array>` /
`WritableStream<Uint8Array>`. They work with `pipeTo`, `pipeThrough`, `tee`,
`getReader`, and `getWriter` without restriction.

**Backpressure** is propagated end-to-end. Writing faster than the remote reads
causes the writer to yield — no unbounded buffering occurs.

**Incoming** bidi streams are consumed from `session.incomingBidirectionalStreams`:

```ts
for await (const stream of session.incomingBidirectionalStreams) {
  // Each stream is a WebTransportBidirectionalStream
  handleStream(stream.readable, stream.writable);
}
```

For HTTP-routed communication (request + response with a URL path), use
`node.fetch` / `node.serve` — they manage sessions and stream routing
transparently.

---

## Datagrams

```ts
const session = await node.connect(peer);

// Send
const writer = session.datagrams.writable.getWriter();
await writer.write(new Uint8Array([1, 2, 3]));

// Receive
for await (const packet of session.datagrams.readable) {
  handlePacket(packet);
}

// Max safe payload size (updated on path migration):
console.log(session.datagrams.maxDatagramSize); // e.g. 1200
```

`IrohDatagramDuplexStream` matches the spec's
[`WebTransportDatagramDuplexStream`](https://developer.mozilla.org/en-US/docs/Web/API/WebTransportDatagramDuplexStream):

```ts
interface IrohDatagramDuplexStream {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
  readonly maxDatagramSize: number | null;  // null = datagrams unavailable on this path
  incomingHighWaterMark: number;            // spec property
  outgoingHighWaterMark: number;            // spec property
}
```

Writing a datagram larger than `maxDatagramSize` throws `IrohSendDatagramError`
with code `"TOO_LARGE"`. `maxDatagramSize` is updated automatically when the
connection path migrates (relay ↔ direct changes MTU).

**Use cases for datagrams:**
- Game state / position updates
- Audio/video control signals and timestamps
- Sensor telemetry at high frequency
- Heartbeat / liveness pings

---

## What you do NOT get

- **Browser wire compatibility.** Browser `WebTransport` uses HTTP/3 with
  specific TLS certificate requirements. iroh-http uses Iroh's QUIC identity
  model (Ed25519 public keys). A browser cannot `new WebTransport("iroh://…")`
  to an iroh-http node.
- **`createUnidirectionalStream` for HTTP.** HTTP in iroh-http is always
  bidirectional. Unidirectional streams are a lower QUIC primitive exposed
  through `IrohSession` but not used by the HTTP layer.

---

## References

- [WebTransport API — MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport)
- [WebTransport spec — W3C](https://w3c.github.io/webtransport/)

→ [Patch 27](../patches/27_patch.md)
