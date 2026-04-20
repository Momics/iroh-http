# @momics/iroh-http-node

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

const node = await createNode({ verifyNodeId: true });
console.log("Node ID:", node.publicKey.toString());

// Serve requests
node.serve({}, (req) => {
  const path = new URL(req.url).pathname;
  if (path === "/hello") return new Response("Hello, world!");
  return new Response("Not found", { status: 404 });
});

// Fetch from a remote peer
const res = await node.fetch(remotePeerId, "/hello");
console.log(await res.text());

await node.close();
```

## Options

```ts
const node = await createNode({
  key: savedKey,                              // SecretKey or Uint8Array — restore identity
  verifyNodeId: true,                         // trust all incoming peers (or pass a verifier fn)
  relayMode: "https://my-relay.example.com",  // custom relay URL (or "default", "staging", "disabled")
  advanced: { drainTimeout: 30_000 },         // ms to wait for slow body readers
});
// Use node.browse() / node.advertise() for mDNS peer discovery.
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
