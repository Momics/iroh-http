# Tauri Guidelines

Applies to: `iroh-http-tauri` (Rust plugin + guest JS).

For engineering values and invariants, see [principles.md](../principles.md).
For JS/TS conventions on the guest side, see [javascript.md](javascript.md).

---

## Architecture

```
Guest JS (webview)
  └── @momics/iroh-http-shared  (buildNode, makeFetch, makeServe)
        └── Tauri invoke / Channel
              └── iroh-http-tauri plugin (Rust)
                    └── iroh-http-core
```

The guest JS implements the `Bridge` interface using Tauri invoke calls. The result is the same `IrohNode` interface Node and Deno users get — no Tauri-specific surface in the user-facing API.

---

## Rust Plugin Conventions

**Command naming:** `snake_case` in Rust, registered via `tauri::generate_handler![]`, invoked as `plugin:iroh-http|command_name` on the JS side.

**Serde:** command args use `#[derive(Deserialize)]` with `#[serde(rename_all = "camelCase")]`. Return types use `#[derive(Serialize)]` with the same rename. The Rust struct uses `endpoint_handle`; JS sends/receives `endpointHandle`.

**Errors:** all commands return `Result<T, String>`. Errors are stringified before crossing the invoke boundary. The guest JS wraps them in the shared error classification system (`classifyError`).

**Binary data:** body chunks cross the invoke boundary as base64-encoded strings. This is a Tauri limitation — invoke does not support raw binary payloads efficiently. Encode on send, decode on receive. Code above the bridge never sees base64.

---

## Guest JS Conventions

The guest JS implements `Bridge` by calling `invoke`. All base64 conversion happens at the bridge boundary — nothing above sees it.

**Serve callback:** uses a `Channel<RequestPayload>` rather than `ThreadsafeFunction` (Node) or JSON dispatch (Deno). The Rust `serve` command accepts the channel; each incoming request is pushed through it; the guest receives it, runs the handler, and calls back with the response.

**Naming:** follows the [JavaScript guidelines](javascript.md) — `camelCase` functions, `PascalCase` types, WHATWG standard types for `Request`/`Response`.

---

## Permissions

Follow Tauri's permission naming: `allow-<command-name>` in kebab-case. The default permission set grants all plugin commands. Do not leave commands unprotected by relying on ambient app permissions.

---

## What Not To Do

- Don't add Tauri-specific types to the user-facing `IrohNode` API. The `buildNode` factory produces an identical interface across all platforms.
- Don't expose invoke command names or Tauri internals in error messages shown to users.
- Don't duplicate logic that belongs in `@momics/iroh-http-shared`. If it works for all JS platforms, it goes in shared.

---

## Testing

- **Rust commands:** test through `iroh-http-core` integration tests. Tauri commands are thin wrappers.
- **Guest JS:** test through the `IrohNode` surface. The `Bridge` is substitutable — mock tests can replace invoke calls.
- **End-to-end:** `tauri-driver` or manual webview testing. Reserve for release validation.
