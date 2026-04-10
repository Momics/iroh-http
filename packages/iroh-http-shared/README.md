# @momics/iroh-http-shared

Shared TypeScript layer for [iroh-http](https://github.com/momics/iroh-http) — pure TypeScript, no native dependencies.

This package contains the platform-agnostic logic that reconstructs web-standard `Request`/`Response` objects from raw FFI data. It is a transitive dependency of the platform adapters (Node.js, Tauri, Deno) and is not intended to be imported directly.

## What's inside

- **`Bridge` interface** — the three methods (`nextChunk`, `sendChunk`, `finishBody`) that each platform adapter implements
- **`makeReadable()`** — wraps a Rust body handle in a `ReadableStream`
- **`pipeToWriter()`** — drains a `ReadableStream` into a Rust body handle
- **`makeFetch()`** — wraps raw FFI fetch in web-standard `fetch()` signature
- **`makeServe()`** — wraps raw FFI serve in Deno-style `serve()` signature
- **`PublicKey` / `SecretKey`** — key classes with base32 encoding
- **`IrohError`** — structured error hierarchy with error codes

## License

MIT OR Apache-2.0
