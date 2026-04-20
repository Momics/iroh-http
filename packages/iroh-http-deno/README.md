# @momics/iroh-http-deno

[![JSR](https://jsr.io/badges/@momics/iroh-http-deno)](https://jsr.io/@momics/iroh-http-deno)

> **Experimental** — This package is in an early, unstable state. APIs may change or break without notice between any releases. Do not depend on it for production use.

Deno native library for [iroh-http](https://github.com/momics/iroh-http) — peer-to-peer HTTP over [Iroh](https://iroh.computer) QUIC transport.

## Install

```sh
deno add jsr:@momics/iroh-http-deno
```

Or import directly:

```ts
import { createNode } from "jsr:@momics/iroh-http-deno";
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

- **Node.js** → [`@momics/iroh-http-node`](https://www.npmjs.com/package/@momics/iroh-http-node) on npm
- **Tauri** → [`@momics/iroh-http-tauri`](https://www.npmjs.com/package/@momics/iroh-http-tauri) on npm

## How is this different from regular HTTP?

iroh-http replaces DNS and TLS with public keys. Each node has a cryptographic identity — you `fetch()` and `serve()` using standard `Request`/`Response` objects, but connections go directly between devices over QUIC, with no server in between. You create a node because each one has its own identity and UDP socket. See the [root README](https://github.com/momics/iroh-http#how-is-this-different-from-regular-http) for a full comparison.

## Usage

```ts
import { createNode } from "jsr:@momics/iroh-http-deno";

const node = await createNode();
console.log("Node ID:", node.publicKey.toString());

node.serve({}, (req) => new Response("Hello from Deno iroh-http!"));

// Node ID is the peer address — share it out-of-band with the remote node
const remoteNodeId = "<paste the other node's publicKey.toString() here>";
const res = await node.fetch(remoteNodeId, "/hello");
console.log(await res.text());
await node.close();
```

## Build from source

```sh
cd packages/iroh-http-deno
deno task build
```

The native library is placed in `lib/`.

## Options

```ts
const node = await createNode({
  key: savedKey,
  discovery: { mdns: true, serviceName: "my-app.iroh-http" },
  advanced: { drainTimeout: 30_000 },
});
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

## License

MIT OR Apache-2.0
