# @momics/iroh-http-tauri

[![npm](https://img.shields.io/npm/v/@momics/iroh-http-tauri)](https://www.npmjs.com/package/@momics/iroh-http-tauri)

> Pre-v1.0. APIs may change between minor releases.

Tauri v2 plugin for [iroh-http](https://github.com/momics/iroh-http). Runs as a Rust plugin with capability-based permissions. Your frontend JS only gets the network access you grant.

## Install

**Frontend:**

```sh
npm install @momics/iroh-http-tauri
```

**Rust plugin** in `src-tauri/Cargo.toml`:

```toml
[dependencies]
tauri-plugin-iroh-http = "0.3"
```

**Register** in `src-tauri/src/lib.rs`:

```rust
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_iroh_http::init())
        .run(tauri::generate_context!())
        .unwrap();
}
```

## Quick start

```ts
import { createNode } from "@momics/iroh-http-tauri";

const node = await createNode();
console.log("Node ID:", node.publicKey.toString());

node.serve({}, (req) => {
  if (req.headers.get("Peer-Id") !== ALLOWED_PEER)
    return new Response("Forbidden", { status: 403 });
  return new Response("hello");
});

const res = await node.fetch("httpi://<peer-public-key>/");
console.log(await res.text());
```

## Full API

The API is identical across Node.js, Deno, and Tauri: HTTP fetch/serve, QUIC sessions, mDNS discovery, and Ed25519 crypto. See the [API overview](../../docs/api-overview.md) for the complete reference.

## Permissions

Tauri's capability system controls what the frontend can access. Declare permissions in `capabilities/default.json`:

| Permission | Covers |
|---|---|
| `iroh-http:default` | `createNode()`, `close()`, node introspection |
| `iroh-http:fetch` | `node.fetch()` + body streaming |
| `iroh-http:serve` | `node.serve()` + body streaming |
| `iroh-http:connect` | Raw QUIC sessions (bidi streams, datagrams) |
| `iroh-http:mdns` | mDNS peer discovery |
| `iroh-http:crypto` | Key generation, signing, verification |

A typical app using fetch and serve:

```json
{
  "permissions": [
    "iroh-http:default",
    "iroh-http:fetch",
    "iroh-http:serve"
  ]
}
```

## Tauri specifics

- Serve callbacks are delivered to the frontend via Tauri `Channel` events (push model).
- All crypto functions are async (round-trip through the Rust plugin via Tauri invoke).
- QUIC sessions require the `iroh-http:connect` permission.
- mDNS requires the `iroh-http:mdns` permission.

## Supported platforms

| Platform | Architecture | Status |
|----------|:----------:|:------:|
| macOS | x86_64 | ✅ |
| macOS | aarch64 (Apple Silicon) | ✅ |
| Linux | x86_64 | ✅ |
| Linux | aarch64 | ✅ |
| Windows | x86_64 | ✅ |

## Other runtimes

| Runtime | Package |
|---------|---------|
| Node.js | [`@momics/iroh-http-node`](https://www.npmjs.com/package/@momics/iroh-http-node) |
| Deno | [`@momics/iroh-http-deno`](https://jsr.io/@momics/iroh-http-deno) |

## License

MIT OR Apache-2.0
