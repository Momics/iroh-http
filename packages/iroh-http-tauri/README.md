# @momics/iroh-http-tauri

[![npm](https://img.shields.io/npm/v/@momics/iroh-http-tauri)](https://www.npmjs.com/package/@momics/iroh-http-tauri)

> Pre-v1.0. APIs may change between minor releases.

Tauri v2 plugin for [iroh-http](https://github.com/momics/iroh-http): peer-to-peer networking over [Iroh](https://iroh.computer) QUIC. Nodes are addressed by Ed25519 public key, with no DNS, no TLS certificates, and no intermediate servers.

## Install

```sh
npm install @momics/iroh-http-tauri
```

Add the Rust plugin to your Tauri app's `Cargo.toml`:

```toml
[dependencies]
tauri-plugin-iroh-http = "0.3"
```

Register in `src-tauri/src/lib.rs`:

```rust
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_iroh_http::init())
        .run(tauri::generate_context!())
        .unwrap();
}
```

## HTTP: serve and fetch

Send and receive HTTP requests over QUIC using the standard WHATWG `Request`/`Response` interface.

```ts
import { createNode } from "@momics/iroh-http-tauri";

const node = await createNode();
console.log("Node ID:", node.publicKey.toString()); // share out-of-band

const ALLOWED_PEERS = new Set(["<remote-node-public-key>"]);
node.serve({}, (req) => {
  const peerId = req.headers.get("Peer-Id");
  if (!ALLOWED_PEERS.has(peerId)) return new Response("Forbidden", { status: 403 });
  return new Response("Hello from Tauri!");
});

const res = await node.fetch("httpi://<remote-node-public-key>/");
console.log(await res.text());
```

## Raw QUIC sessions

Open a raw QUIC connection to any peer and exchange data over bidirectional streams, unidirectional streams, or datagrams. The API mirrors [WebTransport](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport).

```ts
// Connect to a peer:
const session = await node.dial("<peer-public-key>");
await session.ready;

const { readable, writable } = await session.createBidirectionalStream();
const writer = writable.getWriter();
await writer.write(new TextEncoder().encode("hello"));
await writer.close();

// Accept incoming sessions:
for await (const session of node.incoming()) {
  console.log("peer connected:", session.remoteId.toString());
  for await (const { readable, writable } of session.incomingBidirectionalStreams) {
    // handle stream
  }
}
```

Requires the `iroh-http:connect` permission (see [Permissions](#permissions) below).

## Cryptographic utilities

Every node has an Ed25519 keypair. Key generation, signing, and verification are also available as standalone functions.

```ts
import { generateSecretKey, secretKeySign, publicKeyVerify } from "@momics/iroh-http-tauri";

const sk = generateSecretKey();                              // 32-byte Uint8Array
const data = new TextEncoder().encode("hello");
const sig = await secretKeySign(sk, data);                   // 64-byte Uint8Array
const ok  = await publicKeyVerify(node.publicKey.bytes, data, sig); // boolean
```

Class API on a live node:

```ts
const sig = await node.secretKey.sign(data);
const ok  = await node.publicKey.verify(data, sig);
const saved = node.secretKey.toBytes(); // persist and restore identity
const restored = await createNode({ key: saved });
```

Requires the `iroh-http:crypto` permission.

## mDNS peer discovery

```ts
await node.advertise("my-app.iroh-http");

for await (const event of node.browse({ serviceName: "my-app.iroh-http" })) {
  if (event.type === "discovered") {
    const res = await node.fetch(`httpi://${event.nodeId}/api`);
  }
}
```

Requires the `iroh-http:mdns` permission.

## Security

Any peer that knows your node's public key can connect and send requests. Iroh QUIC authenticates peer *identity* cryptographically, but not *authorization*. Use `req.headers.get('Peer-Id')` in your HTTP handler, and `session.remoteId` for raw sessions, to implement allowlists:

```ts
node.serve({}, (req) => {
  const peerId = req.headers.get("Peer-Id");
  if (!ALLOWED_PEERS.has(peerId)) return new Response("Forbidden", { status: 403 });
  return new Response("ok");
});
```

## Permissions

Permissions are declared in your app's `capabilities/default.json`. They are split by capability so you only grant what your app actually uses.

| Permission | What it covers |
|---|---|
| `iroh-http:default` | `createNode()`, `close()`, node introspection (`publicKey`, `nodeAddr`, etc.) |
| `iroh-http:fetch` | `node.fetch()` and all internal body-streaming required for it |
| `iroh-http:serve` | `node.serve()` and all internal body-streaming required for it |
| `iroh-http:connect` | Raw QUIC sessions: bidirectional streams and datagrams |
| `iroh-http:mdns` | Local peer discovery via mDNS |
| `iroh-http:crypto` | Key generation, signing, and verification |

A typical app using fetch and serve:

```json
{
  "permissions": [
    "iroh-http:default",
    "iroh-http:fetch",
    "iroh-http:serve"
  ]
}
```

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
| Deno | `@momics/iroh-http-deno` | [jsr.io/@momics/iroh-http-deno](https://jsr.io/@momics/iroh-http-deno) |

## License

MIT OR Apache-2.0
