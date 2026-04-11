# Tauri Platform Guidelines

Applies to: `iroh-http-tauri` (Rust plugin + guest JS).

The Tauri adapter is a bridge between two worlds: a Rust plugin that
registers Tauri commands, and guest-side JavaScript that wires those
commands into the shared `@momics/iroh-http-shared` layer. These guidelines
cover conventions specific to the Tauri boundary.

---

## Architecture

```
Guest JS (webview)
  └─ @momics/iroh-http-shared  (buildNode, makeFetch, makeServe)
       └─ Tauri invoke / Channel
            └─ iroh-http-tauri plugin (Rust)
                 └─ iroh-http-core
```

The guest JS imports `buildNode` from shared and plugs in Tauri invoke
calls as the `Bridge` implementation. The result is the same `IrohNode`
interface that Node and Deno users get.

---

## Rust plugin conventions

### Command naming

Commands use `snake_case` in Rust and are invoked as
`plugin:iroh-http|command_name` on the JS side.

All commands are registered in `lib.rs` via `tauri::generate_handler![]`.

### Serde serialization

- Tauri command args use `#[derive(Deserialize)]` with
  `#[serde(rename_all = "camelCase")]` to match JS-side naming.
- Return types use `#[derive(Serialize)]` with the same `camelCase` rename.
- This means the Rust struct says `endpoint_handle` but the JS caller
  sends/receives `endpointHandle`.

### Error handling

All commands return `Result<T, String>`. Errors are stringified before
crossing the invoke boundary. The guest JS side wraps these in the shared
error classification system (`classifyError`).

### Binary data

Body chunks cross the Tauri invoke boundary as **base64-encoded strings**,
not raw bytes. The Rust side encodes with `base64::STANDARD` on send and
decodes on receive.

```rust
// Sending a chunk to JS:
B64.encode(&bytes)

// Receiving a chunk from JS:
B64.decode(chunk_b64)?
```

This is a Tauri limitation — `invoke` doesn't support raw binary payloads
efficiently. The performance cost is acceptable for typical HTTP body sizes.

---

## Guest JS conventions

### Bridge implementation

The guest JS implements the `Bridge` interface from `@momics/iroh-http-shared`
by calling `invoke`:

```ts
const bridge: Bridge = {
  async nextChunk(handle) {
    const b64 = await invoke("plugin:iroh-http|next_chunk", { handle });
    return b64 ? base64ToUint8Array(b64) : null;
  },
  // ...
};
```

All base64 encoding/decoding happens at the bridge boundary. Code above
the bridge never sees base64.

### Serve callback — Channel

Unlike Node (which uses `ThreadsafeFunction`) or Deno (which uses JSON
dispatch), Tauri uses a `Channel` to push incoming requests from Rust to
the guest JS.

The Rust `serve` command accepts a `Channel<RequestPayload>` argument.
Each incoming request is sent over the channel. The guest JS receives it,
invokes the user's handler, and calls `respond_to_request` with the
response head.

### Naming in guest JS

Same as the [JavaScript guidelines](guidelines-javascript.md): `camelCase`
functions, `PascalCase` types, WHATWG standard types for Request/Response.

---

## Permissions

Tauri plugin permissions are defined in `permissions/`. The default
permission set grants all commands:

```toml
# permissions/default.toml
[default]
description = "Default permissions for iroh-http plugin"
permissions = ["allow-create-endpoint", "allow-close-endpoint", ...]
```

Follow Tauri's permission naming: `allow-<command-name>` with kebab-case.

---

## Testing

- **Rust commands:** test through `iroh-http-core` integration tests (the
  Tauri commands are thin wrappers).
- **Guest JS:** test through the shared `IrohNode` surface. The bridge
  implementation is substitutable, so mock tests can replace `invoke` calls.
- **End-to-end:** requires a Tauri test harness (`tauri-driver` or manual
  webview test). These are expensive and reserved for release validation.

---

## What not to do

- Don't add Tauri-specific types to the user-facing `IrohNode` API. The
  `buildNode` factory produces an identical interface across all platforms.
- Don't expose `invoke` call names or Tauri internals in error messages
  shown to users.
- Don't bypass the shared layer — if logic works for all JS platforms, it
  belongs in `@momics/iroh-http-shared`, not duplicated in the Tauri guest.
