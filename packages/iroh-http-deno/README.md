# @momics/iroh-http-deno

[![JSR](https://jsr.io/badges/@momics/iroh-http-deno)](https://jsr.io/@momics/iroh-http-deno)

> **Experimental.** This package is in an early, unstable state. APIs may change or break without notice between any releases. Do not depend on it for production use.

Deno native library for [iroh-http](https://github.com/momics/iroh-http): peer-to-peer networking over [Iroh](https://iroh.computer) QUIC. Nodes are addressed by Ed25519 public key, with no DNS, no TLS certificates, and no intermediate servers.

## Install

```sh
deno add jsr:@momics/iroh-http-deno
```

Or import directly:

```ts
import { createNode } from "jsr:@momics/iroh-http-deno";
```

## HTTP: serve and fetch

Send and receive HTTP requests over QUIC using the standard WHATWG `Request`/`Response` interface.

```ts
import { createNode } from "jsr:@momics/iroh-http-deno";

const node = await createNode();
console.log("Node ID:", node.publicKey.toString()); // share out-of-band

const ALLOWED_PEERS = new Set(["<remote-node-public-key>"]);
node.serve({}, (req) => {
  const peerId = req.headers.get("Peer-Id");
  if (!ALLOWED_PEERS.has(peerId)) return new Response("Forbidden", { status: 403 });
  return new Response("Hello from Deno iroh-http!");
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
ac.abort();
```

## Raw QUIC sessions

Open a raw QUIC connection to any peer and exchange data over bidirectional streams, unidirectional streams, or datagrams. The API mirrors [WebTransport](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport).

**Connect to a peer:**

```ts
const session = await node.connect("<peer-public-key>");
await session.ready;

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
for await (const session of node.sessions({ signal: ac.signal })) {
  console.log("peer connected:", session.remoteId.toString());
  for await (const { readable, writable } of session.incomingBidirectionalStreams) {
    // handle stream
  }
}
```

**Datagrams** (unreliable, low-latency):

```ts
const session = await node.connect("<peer-public-key>");
await session.datagrams.writable.getWriter()
  .write(new TextEncoder().encode("ping"));

const { value } = await session.datagrams.readable.getReader().read();
console.log(new TextDecoder().decode(value));
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
import { generateSecretKey, secretKeySign, publicKeyVerify } from "jsr:@momics/iroh-http-deno";

const sk = generateSecretKey();                      // 32-byte Uint8Array (Ed25519 seed)
const pk = node.publicKey.bytes;                     // 32-byte Uint8Array

const data = new TextEncoder().encode("hello");
const sig = await secretKeySign(sk, data);           // Uint8Array (64 bytes)
const ok  = await publicKeyVerify(pk, data, sig);    // boolean
```

**Class API on a live node** (round-trips through Rust, async):

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
await node.advertise("my-app.iroh-http");

const ac = new AbortController();
for await (const event of node.browse({ serviceName: "my-app.iroh-http", signal: ac.signal })) {
  if (event.type === "discovered") {
    const res = await node.fetch(event.nodeId, "/api");
  }
}
```

## Options

```ts
const node = await createNode({
  key: savedKey,
  discovery: { mdns: true, serviceName: "my-app.iroh-http" },
  advanced: { drainTimeout: 30_000 },
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

for await (const session of node.sessions()) {
  if (!ALLOWED_PEERS.has(session.remoteId.toString())) {
    session.close(403, "Forbidden");
    continue;
  }
  // ...handle session
}
```

## Build from source

```sh
cd packages/iroh-http-deno
deno task build
```

The native library is placed in `lib/`.

## Supported Platforms

| Platform | Architecture | Status |
|----------|:----------:|:------:|
| macOS | x86_64 | ✅ |
| macOS | aarch64 (Apple Silicon) | ✅ |
| Linux | x86_64 | ✅ |
| Linux | aarch64 | ✅ |
| Windows | x86_64 | ✅ |

## Other runtimes

| Runtime | Package | Docs |
|---------|---------|------|
| Node.js | `@momics/iroh-http-node` | [npmjs.com/package/@momics/iroh-http-node](https://www.npmjs.com/package/@momics/iroh-http-node) |
| Tauri v2 | `@momics/iroh-http-tauri` | [npmjs.com/package/@momics/iroh-http-tauri](https://www.npmjs.com/package/@momics/iroh-http-tauri) |

## License

MIT OR Apache-2.0
