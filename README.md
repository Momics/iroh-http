# iroh-http

Peer-to-peer HTTP — fetch and serve between devices using [Iroh](https://iroh.computer) QUIC transport. No servers, no DNS, no TLS certificates. Nodes are addressed by public key.

## How is this different from regular HTTP?

Regular HTTP needs infrastructure: a server with a public IP, DNS records, TLS certificates. A client connects to a server — never the other way around.

iroh-http replaces all of that with a **public key**. Every device gets a permanent cryptographic identity. Two devices that know each other's public key can connect directly — peer-to-peer, through NATs, without a server in between.

| | Regular HTTP | iroh-http |
|---|---|---|
| **Addressing** | Domain name → IP address (DNS) | Public key (Ed25519) |
| **Identity** | TLS certificate from a CA | Keypair you generate locally |
| **Connection** | Client → server only | Any node → any node |
| **NAT traversal** | Not possible | Built-in (Iroh relay + hole-punching) |
| **Discovery** | DNS | Relay, DNS, or local mDNS |
| **Encryption** | TLS (certificate-based) | QUIC (key-based, always on) |

### Why `createNode()`?

In regular HTTP, `fetch()` is a global — the browser or runtime manages the network socket for you. In iroh-http, each node has its own cryptographic identity and QUIC endpoint (like a personal mini-server), so you create one explicitly:

```ts
const node = await createNode();         // generates a new keypair
console.log(node.publicKey.toString());  // this is your "address"
```

The node can both **send and receive** — `fetch()` and `serve()` share the same identity and the same UDP socket. You can persist the keypair to keep the same address across restarts:

```ts
const node = await createNode({ key: savedKey }); // same public key every time
```

### Web-standard API

The `fetch()` and `serve()` APIs use standard `Request` and `Response` objects. If you know how to write a `fetch()` call or a request handler, you already know how to use iroh-http. Libraries that work with standard `Request`/`Response` (routing, middleware, body parsers) should work unchanged.

### What doesn't work

- **Browsers** — iroh-http requires raw UDP sockets, which browsers don't expose. A browser-compatible path via WebTransport is a future goal.
- **Existing HTTP servers/CDNs** — you can't `fetch("https://google.com")` through iroh-http. It's a separate network addressed by public key, not domain names.

## How it works

```
  ┌──────────┐   QUIC (Iroh)   ┌──────────┐
  │  Node A  │◄────────────────►│  Node B  │
  └──────────┘                  └──────────┘
  fetch("/api")                 serve(handler)
```

Nodes find each other via [Iroh's](https://iroh.computer) relay network or local mDNS discovery. Every connection is end-to-end authenticated using Ed25519 public keys.

> **Built on [Iroh](https://iroh.computer)** — a networking library for connecting devices directly. Iroh handles NAT traversal, relay fallback, and encrypted QUIC transport so iroh-http can focus on the HTTP layer. See the [Iroh documentation](https://iroh.computer/docs) to learn more.

## Quick start

### Node.js

```ts
import { createNode } from "@momics/iroh-http-node";

const node = await createNode();
console.log("My node ID:", node.publicKey.toString());

// Serve
node.serve({}, (req) => new Response("hello from iroh-http!"));

// Fetch from a remote peer
const res = await node.fetch(remotePeerId, "/hello");
console.log(await res.text());

await node.close();
```

### Deno

```ts
import { createNode } from "jsr:@momics/iroh-http-deno";

const node = await createNode();
node.serve({}, (req) => new Response("hello"));
const res = await node.fetch(remotePeerId, "/hello");
console.log(await res.text());
```

### Tauri

```ts
import { createNode } from "@momics/iroh-http-tauri";

const node = await createNode();
node.serve({}, (req) => new Response("hello"));
```

## Features

- **Web-standard `fetch`/`serve` API** — uses standard `Request`/`Response` objects; works with existing routing and middleware libraries
- **`httpi://` URL scheme** — clean, parseable URLs with the peer's public key as the host (see [Protocol docs](docs/protocol.md))
- **Bidirectional streaming** — full-duplex streams via `createBidirectionalStream`
- **Response trailers** — HTTP/1.1 chunked trailers for streaming metadata
- **AbortSignal** — cancel in-flight requests
- **mDNS discovery** — find peers on the local network automatically
- **Mobile lifecycle** — reconnect on app resume (Tauri)
- **Multi-platform** — Node.js, Deno, Tauri (desktop + mobile)

## Architecture

```
iroh-http-core (Rust)       — QUIC transport, HTTP framing (via hyper)
iroh-http-discovery (Rust)  — optional mDNS (feature = "mdns")
iroh-http-shared (TS)       — shared Bridge interface + error types
iroh-http-node (napi-rs)    — Node.js native addon
iroh-http-tauri (Tauri v2)  — Tauri plugin
iroh-http-deno (FFI)        — Deno native library
```

See the [docs/](docs/) folder for architecture details and the [examples/](examples/) folder for runnable demos.

## Development

All commands run from the repository root via npm scripts:

```sh
npm install                # install workspace dependencies (once)

npm run check              # fast typecheck: cargo check + tsc (no linking)
npm run lint               # cargo fmt --check + clippy
npm run build              # build everything: Rust, TypeScript, Node, Deno, Tauri
npm run test               # test everything: Rust, Node e2e, Deno, cross-runtime interop
```

### Build & test individual platforms

```sh
npm run build:core         # Rust workspace only (release)
npm run build:node         # Node.js native addon (current platform)
npm run build:deno         # Deno native library (current platform)
npm run build:tauri        # Tauri plugin TypeScript
npm run build:all          # all platforms (cross-compile, needs zigbuild)

npm run test:rust          # cargo test (unit + integration + property tests)
npm run test:node          # Node.js smoke + e2e + compliance
npm run test:deno          # Deno smoke + compliance
npm run test:interop       # cross-runtime compliance (node ↔ deno)
```

### Release

```sh
npm run release:dry -- 0.2.0   # dry run: build → test → version bump (no publish)
npm run release -- 0.2.0       # full release: build → test → bump → publish → tag → push
```

The release script runs build and test automatically before publishing. See
[scripts/release.sh](scripts/release.sh) for details and prerequisites.

For more details on testing infrastructure (property tests, fuzz targets,
CI configuration), see [docs/build-and-test.md](docs/build-and-test.md).

## Acknowledgements

iroh-http is built on top of [Iroh](https://iroh.computer) by [n0, inc.](https://n0.computer) Iroh provides the QUIC transport, NAT traversal, relay infrastructure, and peer identity that make serverless peer-to-peer HTTP possible.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
