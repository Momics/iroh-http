# iroh-http

[![CI](https://github.com/Momics/iroh-http/actions/workflows/ci.yml/badge.svg)](https://github.com/Momics/iroh-http/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/@momics/iroh-http-node)](https://www.npmjs.com/package/@momics/iroh-http-node)
[![JSR](https://jsr.io/badges/@momics/iroh-http-deno)](https://jsr.io/@momics/iroh-http-deno)
[![crates.io](https://img.shields.io/crates/v/iroh-http-core)](https://crates.io/crates/iroh-http-core)

> Pre-v1.0: **do not rely on this in critical or production use.** Still early WIP. APIs may change between minor releases.

Peer-to-peer networking over [Iroh](https://iroh.computer) QUIC. Nodes are addressed by Ed25519 public key: no DNS, no TLS certificates, no intermediate servers.

The transport uses [Iroh](https://iroh.computer) for QUIC connectivity, including NAT traversal, hole-punching, and relay fallback. HTTP/1.1 framing runs inside QUIC bidirectional streams via [hyper](https://hyper.rs). Each node's identity is an Ed25519 keypair, so the public key is the network address. Changing your IP or network does not change your address.

#### Supported Runtimes
| Runtime  | Install                               | Docs                                                                                               |
| -------- | ------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Node.js  | `npm install @momics/iroh-http-node`  | [npmjs.com/package/@momics/iroh-http-node](https://www.npmjs.com/package/@momics/iroh-http-node)   |
| Deno     | `deno add jsr:@momics/iroh-http-deno` | [jsr.io/@momics/iroh-http-deno](https://jsr.io/@momics/iroh-http-deno)                             |
| Tauri v2 | `npm install @momics/iroh-http-tauri` | [npmjs.com/package/@momics/iroh-http-tauri](https://www.npmjs.com/package/@momics/iroh-http-tauri) |

The API is identical across all runtimes. See the package READMEs for platform support matrices, install details, and runtime-specific options.

## Features

### HTTP over QUIC

Standard WHATWG `fetch`/`serve`, but over QUIC to a peer identified by public key. Handlers, routers, and middleware written for Deno, Cloudflare Workers, Hono, or anything `fetch`-shaped work without modification.

```ts
import { createNode } from "@momics/iroh-http-node";

const node = await createNode();
// Share node.publicKey.toString() with peers out-of-band.
// For a full address ticket (key + relay + direct IPs): node.addr()

const ALLOWED_PEERS = new Set(["<remote-node-public-key>"]);
node.serve({}, (req) => {
  // Verify the connecting peer before processing anything.
  if (!ALLOWED_PEERS.has(req.headers.get("Peer-Id")))
    return new Response("Forbidden", { status: 403 });
  return new Response("hello");
});

// On another machine:
const res = await node.fetch("httpi://<peer-public-key>/");
console.log(await res.text()); // "hello"
await node.close();
```

Browsers are not supported (raw UDP required). This is not a proxy for public HTTP: peers are addressed by key, not by hostname.

### Raw QUIC sessions

Open a direct QUIC connection to any peer and exchange data over bidirectional streams, unidirectional streams, or datagrams. The API mirrors [WebTransport](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport).

```ts
// Initiating side:
const session = await node.dial("<peer-public-key>");
const { readable, writable } = await session.createBidirectionalStream();

// Receiving side:
for await (const session of node.incoming()) {
  const stream = await session.incomingBidirectionalStreams.getReader().read();
  // ...handle stream
}
```

### Cryptographic utilities

Every node has an Ed25519 keypair. Key generation, signing, and verification are available as standalone functions without needing a live node.

```ts
import { generateSecretKey, secretKeySign, publicKeyVerify } from "@momics/iroh-http-node";

const sk  = generateSecretKey();                      // 32-byte key
const sig = secretKeySign(sk, data);                  // 64-byte signature
const ok  = publicKeyVerify(publicKey, data, sig);    // boolean
```

The class API on a live node runs through Rust and is async:

```ts
const sig = await node.secretKey.sign(data);
const ok  = await node.publicKey.verify(data, sig);
```

### mDNS peer discovery

Advertise and discover peers on the local network without out-of-band coordination.

```ts
await node.advertise("my-app.iroh-http");

for await (const event of node.browse("my-app.iroh-http")) {
  if (event.type === "discovered") {
    const res = await node.fetch(`httpi://${event.nodeId}/api`);
  }
}
```


## Architecture

| Package | Role |
|---------|------|
| `iroh-http-core` (Rust) | QUIC transport, HTTP/1.1 framing via [hyper](https://hyper.rs) |
| `iroh-http-discovery` (Rust) | mDNS peer discovery (`feature = "mdns"`) |
| `iroh-http-adapter` (Rust) | Shared FFI adapter layer |
| `iroh-http-shared` (TS) | Node class, key types, session types, error hierarchy |
| `iroh-http-node` | Node.js native addon (napi-rs) |
| `iroh-http-deno` | Deno native library (FFI) |
| `iroh-http-tauri` | Tauri v2 plugin |

See [docs/](docs/) and [examples/](examples/).

## Development

```sh
npm install

npm run check    # cargo check + tsc
npm run lint     # cargo fmt --check + clippy
npm run build    # build everything
npm run test     # test everything
```

```sh
npm run build:core    npm run build:node    npm run build:deno    npm run build:tauri
npm run test:rust     npm run test:node     npm run test:deno     npm run test:interop
```

## Acknowledgements

Built on [Iroh](https://iroh.computer) by [n0](https://n0.computer).

## License

Apache-2.0 or MIT. See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).
