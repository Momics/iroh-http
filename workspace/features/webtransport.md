---
status: not-implemented
scope: core — IrohNode + new IrohSession type
---

# Feature: WebTransport Compatibility + Datagrams

## The case for full WebTransport alignment

iroh-http's transport is QUIC-native, encrypted, peer-authenticated, and
supports bidirectional streams and unreliable datagrams. The
[WebTransport API](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport)
was designed to expose exactly these capabilities to the web platform over
HTTP/3. The conceptual fit is near-perfect.

The naming alignment was intentional from the start: `createBidirectionalStream`,
`BidirectionalStream` with `.readable` / `.writable`, `closed` promise. The
gap to full compatibility is small and should be closed.

---

## Why `IrohNode` does not implement `WebTransport` directly

`WebTransport` in the browser is a **per-session** object — one instance per
remote peer:

```ts
const wt = new WebTransport("https://example.com");
await wt.ready;
```

`IrohNode` is a **multi-peer local endpoint** — analogous to the browser's QUIC
engine itself, not to a single session. A single node talks to many peers
simultaneously; making it implement `WebTransport` would collapse the
many-to-one distinction that makes P2P different from client/server.

The right mapping:

| Concept | iroh-http | Browser WebTransport |
|---|---|---|
| Local endpoint | `IrohNode` | Browser's QUIC stack |
| Session to one peer | `IrohSession` (new) | `WebTransport` instance |
| HTTP request | `node.fetch(peer, url)` | `fetch(url)` over H3 |
| HTTP server | `node.serve(handler)` | Server-side WT handler |

---

## Proposed design: `IrohSession implements WebTransport`

```ts
// Open a WebTransport-compatible session to a specific peer.
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

`IrohSession` satisfies `WebTransport` exactly:

```ts
interface IrohSession extends WebTransport {
  // All standard WebTransport properties, plus:

  /** The peer's verified public key. Not on standard WebTransport. */
  readonly remoteId: PublicKey;
}
```

### `node.fetch` and `node.serve` stay unchanged

`node.fetch(peer, url, init)` is a convenience method that internally calls
`node.connect(peer)`, opens a bidirectional stream, sends an HTTP request over
it, and returns a standard `Response`. From the caller's perspective nothing
changes — sessions are managed transparently by the connection pool.

`node.serve` similarly accepts incoming sessions and routes them to the
HTTP handler. The HTTP layer is built on top of raw sessions; raw sessions are
also directly accessible.

---

## `WebTransport` interface — full property map

| Property | `IrohSession` implementation | Notes |
|---|---|---|
| `ready` | Resolves when QUIC handshake completes | Trivial |
| `closed` | `Promise<{closeCode, reason}>` | Shape change from current `Promise<void>` |
| `datagrams` | `IrohDatagramDuplexStream` (see below) | New |
| `createBidirectionalStream()` | Opens a QUIC bidi stream | Already implemented; signature change (no `peer`) |
| `createUnidirectionalStream()` | Opens a QUIC send-only stream | New |
| `incomingBidirectionalStreams` | `ReadableStream<WebTransportBidirectionalStream>` | Currently internal to serve |
| `incomingUnidirectionalStreams` | `ReadableStream<ReadableStream<Uint8Array>>` | New |
| `close(info?)` | Graceful QUIC connection close | Currently async; spec says sync initiation |

---

## Datagrams

Datagrams are the most compelling feature of this alignment — unreliable,
unordered, low-latency messages with zero HTTP framing overhead.

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
- Game state / position updates — stale data is worthless; latency matters more than reliability
- Audio/video control signals and timestamps
- Sensor telemetry at high frequency
- Heartbeat / liveness pings

---

## Rust side

`iroh::endpoint::Connection` already supports datagrams:
- `connection.send_datagram(bytes)` — unreliable send
- `connection.read_datagram()` — async receive, one datagram at a time

Two new FFI functions needed:
1. `sendDatagram(sessionHandle, data: Uint8Array): Promise<void>`
2. `readDatagram(sessionHandle): Promise<Uint8Array>` — long-poll

The session handle replaces the current endpoint handle + node ID pair;
sessions are first-class objects in the new model.

The old `iroh-rs` adapter implemented both; the pattern is in:
`workspace/old_references/iroh/src/streams/IrohDatagramDuplexStream.mts`

---

## What you do NOT get

- **Browser wire compatibility.** Browser `WebTransport` uses HTTP/3 with
  specific TLS certificate requirements (`serverCertificateHashes` or a CA
  cert). iroh-http uses Iroh's QUIC identity model (Ed25519 public keys).
  These are not wire-compatible; a browser cannot `new WebTransport("iroh://…")`
  to an iroh-http node.
- **`createUnidirectionalStream` for HTTP.** HTTP in iroh-http is always
  bidirectional (request + response on the same stream). Unidirectional streams
  are a lower QUIC primitive exposed through `IrohSession` but not used by the
  HTTP layer.

---

## Migration path

iroh-http is pre-production, so breaking changes are acceptable.

1. Add `node.connect(peer): Promise<IrohSession>` — new entry point.
2. `node.createBidirectionalStream(peer, path)` becomes a shorthand for
   `(await node.connect(peer)).createBidirectionalStream()` with path routing.
3. `node.fetch(peer, url)` stays unchanged at the call site — sessions are
   managed transparently by the pool.
4. `node.closed` shape changes from `Promise<void>` to
   `Promise<{closeCode: number, reason: string}>`.

---

## References

- [WebTransport API — MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport)
- [WebTransport spec — W3C](https://w3c.github.io/webtransport/)
- Old datagram stream: `workspace/old_references/iroh/src/streams/IrohDatagramDuplexStream.mts`
- Old connection with datagrams: `workspace/old_references/iroh/src/IrohConnection.mts`
