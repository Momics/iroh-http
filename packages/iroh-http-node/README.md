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

const node = await createNode();
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
  key: savedKey,                // SecretKey or Uint8Array — restore identity
  idleTimeout: 30_000,          // ms before idle connection cleanup
  relays: ["https://my-relay"], // custom relay URLs
  discovery: { mdns: true, serviceName: "my-app.iroh-http" }, // local discovery
  drainTimeout: 30_000,         // ms to wait for slow body readers
});
```

## License

MIT OR Apache-2.0
