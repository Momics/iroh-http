# @momics/iroh-http-node

[![npm](https://img.shields.io/npm/v/@momics/iroh-http-node)](https://www.npmjs.com/package/@momics/iroh-http-node)

> **Experimental** — This package is in an early, unstable state. APIs may change or break without notice between any releases. Do not depend on it for production use.

Node.js native addon for [iroh-http](https://github.com/momics/iroh-http) — peer-to-peer HTTP over [Iroh](https://iroh.computer) QUIC transport.

## How is this different from regular HTTP?

iroh-http replaces DNS and TLS with public keys. Each node has a cryptographic identity — you `fetch()` and `serve()` using standard `Request`/`Response` objects, but connections go directly between devices over QUIC, with no server in between. You create a node because each one has its own identity and UDP socket. See the [root README](https://github.com/momics/iroh-http#how-is-this-different-from-regular-http) for a full comparison.

## Install

```sh
npm install @momics/iroh-http-node
```

## Usage

```ts
import { createNode } from "@momics/iroh-http-node";

const node = await createNode();
console.log("Node ID:", node.publicKey.toString());

// Serve requests
node.serve({}, (req) => {
  const path = new URL(req.url).pathname;
  if (path === "/hello") return new Response("Hello, world!");
  return new Response("Not found", { status: 404 });
});

// Node ID is the peer address — share it out-of-band with the remote node
const remoteNodeId = "<paste the other node's publicKey.toString() here>";
const res = await node.fetch(remoteNodeId, "/hello");
console.log(await res.text());

await node.close();
```

## Options

```ts
const node = await createNode({
  key: savedKey,                              // SecretKey or Uint8Array — restore identity
  relayMode: "https://my-relay.example.com",  // custom relay URL (or "default", "staging", "disabled")
  advanced: { drainTimeout: 30_000 },         // ms to wait for slow body readers
});
// Use node.browse() / node.advertise() for mDNS peer discovery.
```

## Security

Any peer that knows your node's public key can connect and send requests. Iroh QUIC authenticates peer *identity* cryptographically, but not *authorization*. Use `req.headers.get('Peer-Id')` in your handler to implement allowlists or other access control:

```ts
node.serve({}, (req) => {
  const peerId = req.headers.get('Peer-Id');
  if (!ALLOWED_PEERS.has(peerId)) return new Response('Forbidden', { status: 403 });
  return new Response('ok');
});
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

Other platforms (Linux musl, FreeBSD, Android) are **not** currently
supported. To build from source for an unlisted platform:

```sh
cd packages/iroh-http-node
npx napi build --platform --release
```

## Other runtimes

- **Deno** → [`@momics/iroh-http-deno`](https://jsr.io/@momics/iroh-http-deno) on JSR
- **Tauri** → [`@momics/iroh-http-tauri`](https://www.npmjs.com/package/@momics/iroh-http-tauri) on npm

## License

MIT OR Apache-2.0
