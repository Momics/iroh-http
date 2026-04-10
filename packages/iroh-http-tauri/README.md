# @momics/iroh-http-tauri

Tauri v2 plugin for [iroh-http](https://github.com/momics/iroh-http) — peer-to-peer HTTP over QUIC.

## Install

```sh
npm install @momics/iroh-http-tauri
```

Add the Rust plugin to your Tauri app's `Cargo.toml`:

```toml
[dependencies]
tauri-plugin-iroh-http = { path = "path/to/packages/iroh-http-tauri" }
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
  lifecycle: { autoReconnect: true, maxRetries: 3 },
});

node.serve({}, (req) => new Response("hello from Tauri!"));
const res = await node.fetch(remotePeerId, "/hello");
```

## Permissions

Add to your app's `capabilities/default.json`:

```json
{
  "permissions": ["iroh-http:default"]
}
```

## License

MIT OR Apache-2.0
