# @momics/iroh-http-node

[![npm](https://img.shields.io/npm/v/@momics/iroh-http-node)](https://www.npmjs.com/package/@momics/iroh-http-node)

> **Experimental.** This package is in an early, unstable state. APIs may change or break without notice between any releases. Do not depend on it for production use.

Node.js native addon for [iroh-http](https://github.com/momics/iroh-http): peer-to-peer networking over [Iroh](https://iroh.computer) QUIC. Nodes are addressed by Ed25519 public key, with no DNS, no TLS certificates, and no intermediate servers.

## Install

```sh
npm install @momics/iroh-http-node
```

## HTTP: serve and fetch

Send and receive HTTP requests over QUIC using the standard WHATWG `Request`/`Response` interface.

```ts
import { createNode } from "@momics/iroh-http-node";

const node = await createNode();
console.log("Node ID:", node.publicKey.toString()); // share out-of-band

const ALLOWED_PEERS = new Set(["<remote-node-public-key>"]);
node.serve({}, (req) => {
  const peerId = req.headers.get("Peer-Id");
  if (!ALLOWED_PEERS.has(peerId)) return new Response("Forbidden", { status: 403 });
  return new Response("Hello, world!");
});

const res = await node.fetch("httpi://<remote-node-public-key>/");
console.log(await res.text());
await node.close();
```

`serve()` accepts an optional `AbortSignal` for graceful shutdown:

```ts
const ac = new AbortController();
node.serve({ signal: ac.signal }, handler);
// ...
ac.abort(); // stop accepting new connections
```

## Raw QUIC sessions

Open a raw QUIC connection to any peer and exchange data over bidirectional streams, unidirectional streams, or datagrams. The API mirrors [WebTransport](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport).

**Connect to a peer:**

```ts
const session = await node.dial("<peer-public-key>");
await session.ready;

// Bidirectional: both sides can read and write
const { readable, writable } = await session.createBidirectionalStream();
const writer = writable.getWriter();
await writer.write(new TextEncoder().encode("hello"));
await writer.close();

const reader = readable.getReader();
const { value } = await reader.read();
console.log(new TextDecoder().decode(value));

session.close();
```

**Accept incoming sessions:**

```ts
const ac = new AbortController();
for await (const session of node.incoming({ signal: ac.signal })) {
  console.log("peer connected:", session.remoteId.toString());
  // Accept bidi streams opened by the remote peer:
  for await (const { readable, writable } of session.incomingBidirectionalStreams) {
    // handle stream
  }
}
```

**Datagrams** (unreliable, low-latency):

```ts
const session = await node.dial("<peer-public-key>");
await session.datagrams.writable.getWriter()
  .write(new TextEncoder().encode("ping"));

const { value } = await session.datagrams.readable.getReader().read();
console.log(new TextDecoder().decode(value)); // "pong"
```

**Unidirectional streams:**

```ts
// Send-only stream:
const writable = await session.createUnidirectionalStream();
await writable.getWriter().write(data);

// Receive incoming send-only streams from the remote peer:
const reader = session.incomingUnidirectionalStreams.getReader();
const { value: inStream } = await reader.read();
```

## Cryptographic utilities

Every node has an Ed25519 keypair. Key generation, signing, and verification are also available as standalone functions without needing a live node.

**Standalone functions:**

```ts
import { generateSecretKey, secretKeySign, publicKeyVerify } from "@momics/iroh-http-node";

const sk = generateSecretKey();                      // 32-byte Buffer (Ed25519 seed)
const pk = node.publicKey.bytes;                     // 32-byte Uint8Array

const data = new TextEncoder().encode("hello");
const sig = secretKeySign(sk, data);                 // 64-byte Buffer
const ok  = publicKeyVerify(pk, data, sig);          // boolean
```

**Class API on a live node** (runs through Rust, async):

```ts
const sig = await node.secretKey.sign(data);         // Uint8Array (64 bytes)
const ok  = await node.publicKey.verify(data, sig);  // boolean
```

**Key serialization:**

```ts
const saved = node.secretKey.toBytes();              // Uint8Array (32 bytes)
const restored = await createNode({ key: saved });   // same identity
```

**PublicKey helpers:**

```ts
const pk = PublicKey.fromString(nodeIdString);       // from base32 node ID
console.log(pk.toString());                          // base32 string
console.log(pk.bytes);                               // Uint8Array(32)
pk.equals(otherKey);                                 // identity comparison
```

## mDNS peer discovery

Discover and advertise peers on the local network without out-of-band coordination.

```ts
// Advertise this node to local peers:
await node.advertise("my-app.iroh-http");

// Browse for local peers:
const ac = new AbortController();
for await (const event of node.browse({ serviceName: "my-app.iroh-http", signal: ac.signal })) {
  if (event.type === "discovered") {
    console.log("found peer:", event.nodeId, event.addrs);
    const res = await node.fetch(`httpi://${event.nodeId}/api`);
  } else {
    console.log("peer expired:", event.nodeId);
  }
}
```

## Options

```ts
const node = await createNode({
  key: savedKey,                              // SecretKey or Uint8Array (restores stable identity)
  relayMode: "https://my-relay.example.com",  // custom relay URL (or "default", "staging", "disabled")
  advanced: { drainTimeout: 30_000 },         // ms to wait for slow body readers
});
```

## Security

Any peer that knows your node's public key can connect and send requests. Iroh QUIC authenticates peer *identity* cryptographically, but not *authorization*. Use `req.headers.get('Peer-Id')` in your HTTP handler, and `session.remoteId` for raw sessions, to implement allowlists:

```ts
node.serve({}, (req) => {
  const peerId = req.headers.get("Peer-Id");
  if (!ALLOWED_PEERS.has(peerId)) return new Response("Forbidden", { status: 403 });
  return new Response("ok");
});

for await (const session of node.incoming()) {
  if (!ALLOWED_PEERS.has(session.remoteId.toString())) {
    session.close(403, "Forbidden");
    continue;
  }
  // ...handle session
}
```

## Supported Platforms

Pre-built native binaries are published for:

| Platform | Architecture | Status |
|----------|:----------:|:------:|
| macOS | x86_64 | ✅ |
| macOS | aarch64 (Apple Silicon) | ✅ |
| Linux (glibc) | x86_64 | ✅ |
| Linux (glibc) | aarch64 | ✅ |
| Windows | x86_64 | ✅ |

Other platforms (Linux musl, FreeBSD, Android) are **not** currently supported. To build from source for an unlisted platform:

```sh
cd packages/iroh-http-node
npx napi build --platform --release
```

## Other runtimes

| Runtime | Package | Docs |
|---------|---------|------|
| Deno | `@momics/iroh-http-deno` | [jsr.io/@momics/iroh-http-deno](https://jsr.io/@momics/iroh-http-deno) |
| Tauri v2 | `@momics/iroh-http-tauri` | [npmjs.com/package/@momics/iroh-http-tauri](https://www.npmjs.com/package/@momics/iroh-http-tauri) |

## License

MIT OR Apache-2.0
