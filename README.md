# iroh-http

[![CI](https://github.com/Momics/iroh-http/actions/workflows/ci.yml/badge.svg)](https://github.com/Momics/iroh-http/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/@momics/iroh-http-node)](https://www.npmjs.com/package/@momics/iroh-http-node)
[![JSR](https://jsr.io/badges/@momics/iroh-http-deno)](https://jsr.io/@momics/iroh-http-deno)
[![crates.io](https://img.shields.io/crates/v/iroh-http-core)](https://crates.io/crates/iroh-http-core)

> Pre-v1.0 — **DO NOT rely on this package in critical/production usecases!** Still early WIP. APIs may change between minor releases.

iroh-http lets you dial peers by Ed25519 public key and speak HTTP to them. The transport is [Iroh](https://iroh.computer) QUIC — connections are authenticated by keypair, hole-punching and relay are handled for you, and there are no intermediate servers, no DNS records to maintain, and no IP addresses to track.

The API is standard WHATWG `Request`/`Response`. Handlers, routers, and middleware written for Deno, Cloudflare Workers, Hono, or anything `fetch`-shaped work without modification — you're just changing what's underneath.

```sh
npm install @momics/iroh-http-node
```

**Serve:**

```ts
import { createNode } from "@momics/iroh-http-node";

const node = await createNode();
console.log(node.publicKey.toString()); // give this to peers

node.serve({}, (req) => new Response("hello"));
```

**Fetch:**

```ts
import { createNode } from "@momics/iroh-http-node";

const node = await createNode();
const res = await node.fetch("<peer-public-key>", "/");
console.log(await res.text()); // "hello"
await node.close();
```

Each node owns a QUIC endpoint and a keypair. `createNode()` is explicit because there is no ambient socket — a node can both send and receive:

```ts
const node = await createNode();        // ephemeral keypair
const node = await createNode({ key }); // stable identity across restarts
```

Iroh authenticates *who* is connecting, not *whether they should*. If you need access control, gate on the injected `Peer-Id` header:

```ts
node.serve({}, (req) => {
  if (req.headers.get("Peer-Id") !== TRUSTED_KEY)
    return new Response("Forbidden", { status: 403 });
  return new Response("ok");
});
```

Browsers are not supported (raw UDP required). This is not a proxy for public HTTP — peers are addressed by key, not by hostname.

## Deno / Tauri

```sh
deno add jsr:@momics/iroh-http-deno
npm install @momics/iroh-http-tauri   # Tauri v2 plugin
```

The API is identical across runtimes.

## Architecture

```
iroh-http-core (Rust)       — QUIC transport, HTTP framing (hyper)
iroh-http-discovery (Rust)  — optional mDNS (feature = "mdns")
iroh-http-adapter (Rust)    — shared FFI adapter layer
iroh-http-shared (TS)       — Bridge interface + error types
iroh-http-node (napi-rs)    — Node.js native addon
iroh-http-tauri (Tauri v2)  — Tauri plugin
iroh-http-deno (FFI)        — Deno native library
```

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

Apache-2.0 or MIT — see [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).
