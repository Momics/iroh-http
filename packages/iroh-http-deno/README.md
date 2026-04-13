# @momics/iroh-http-deno

> **Experimental** — This package is in an early, unstable state. APIs may change or break without notice between any releases. Do not depend on it for production use.

Deno native library for [iroh-http](https://github.com/momics/iroh-http) — peer-to-peer HTTP over [Iroh](https://iroh.computer) QUIC transport.

## How is this different from regular HTTP?

iroh-http replaces DNS and TLS with public keys. Each node has a cryptographic identity — you `fetch()` and `serve()` using standard `Request`/`Response` objects, but connections go directly between devices over QUIC, with no server in between. You create a node because each one has its own identity and UDP socket. See the [root README](https://github.com/momics/iroh-http#how-is-this-different-from-regular-http) for a full comparison.

## Usage

```ts
import { createNode } from "jsr:@momics/iroh-http-deno";

const node = await createNode();
console.log("Node ID:", node.publicKey.toString());

node.serve({}, (req) => new Response("Hello from Deno iroh-http!"));

const res = await node.fetch(remotePeerId, "/hello");
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

## License

MIT OR Apache-2.0
