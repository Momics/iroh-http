# iroh-http

Peer-to-peer HTTP — fetch and serve between devices using [Iroh](https://iroh.computer) QUIC transport. No servers, no DNS, no TLS certificates. Nodes are addressed by public key.

## How it works

```
  ┌──────────┐   QUIC (Iroh)   ┌──────────┐
  │  Node A  │◄────────────────►│  Node B  │
  └──────────┘                  └──────────┘
  fetch("/api")                 serve(handler)
```

Nodes find each other via Iroh's relay network or local mDNS discovery. Every connection is end-to-end authenticated using Ed25519 public keys.

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

### Python

```python
import iroh_http

node = iroh_http.create_node()
print("Node ID:", node.node_id())
```

## Features

- **Web-standard `fetch`/`serve` API** — drop-in for browser `fetch`
- **Bidirectional streaming** — full-duplex streams via `createBidirectionalStream`
- **Response trailers** — HTTP/1.1 chunked trailers for streaming metadata
- **AbortSignal** — cancel in-flight requests
- **mDNS discovery** — find peers on the local network automatically
- **Mobile lifecycle** — reconnect on app resume (Tauri)
- **Multi-platform** — Node.js, Deno, Tauri (desktop + mobile), Python

## Architecture

```
iroh-http-core (Rust)       — QUIC transport, HTTP framing
iroh-http-framing (Rust)    — no_std HTTP/1.1 parser
iroh-http-discovery (Rust)  — optional mDNS (feature = "mdns")
iroh-http-shared (TS)       — shared Bridge interface + error types
iroh-http-node (napi-rs)    — Node.js native addon
iroh-http-tauri (Tauri v2)  — Tauri plugin
iroh-http-deno (FFI)        — Deno native library
iroh-http-py (PyO3)         — Python bindings
```

## Development

```sh
# Build all Rust crates
cargo build --workspace

# Check (fast, no linking)
cargo check --workspace

# TypeScript (Node.js + Tauri)
npm install
npm run typecheck

# Tauri plugin (standalone workspace)
cd packages/iroh-http-tauri && cargo check
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
