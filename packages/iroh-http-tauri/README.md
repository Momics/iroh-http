# @momics/iroh-http-tauri

[![npm](https://img.shields.io/npm/v/@momics/iroh-http-tauri)](https://www.npmjs.com/package/@momics/iroh-http-tauri)

> Pre-v1.0 — APIs may change between minor releases.

Tauri v2 plugin for [iroh-http](https://github.com/momics/iroh-http) — peer-to-peer HTTP over [Iroh](https://iroh.computer) QUIC transport.

## How is this different from regular HTTP?

iroh-http replaces DNS and TLS with public keys. Each node has a cryptographic identity — you `fetch()` and `serve()` using standard `Request`/`Response` objects, but connections go directly between devices over QUIC, with no server in between. You create a node because each one has its own identity and UDP socket. See the [root README](https://github.com/momics/iroh-http#how-is-this-different-from-regular-http) for a full comparison.

## Install

```sh
npm install @momics/iroh-http-tauri
```

Add the Rust plugin to your Tauri app's `Cargo.toml`:

```toml
[dependencies]
tauri-plugin-iroh-http = "0.2"
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

## Usage (guest JS)

```ts
import { createNode } from "@momics/iroh-http-tauri";

const node = await createNode({
  reconnect: { auto: true, maxRetries: 3 },
});

// serve() opens a public endpoint — any peer that knows your public key can connect.
// Always check Peer-Id to restrict access to known peers.
const ALLOWED_PEERS = new Set(["<remote-node-public-key>"]);
node.serve({}, (req) => {
  const peerId = req.headers.get("Peer-Id");
  if (!ALLOWED_PEERS.has(peerId)) return new Response("Forbidden", { status: 403 });
  return new Response("hello from Tauri!");
});
// Node ID is the peer address — share it out-of-band with the remote node
const remoteNodeId = "<paste the other node's publicKey.toString() here>";
const res = await node.fetch(remoteNodeId, "/hello");
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

## Permissions

Add to your app's `capabilities/default.json`:

```json
{
  "permissions": ["iroh-http:default"]
}
```

## Other runtimes

- **Node.js** → [`@momics/iroh-http-node`](https://www.npmjs.com/package/@momics/iroh-http-node) on npm
- **Deno** → [`@momics/iroh-http-deno`](https://jsr.io/@momics/iroh-http-deno) on JSR

## License

MIT OR Apache-2.0
