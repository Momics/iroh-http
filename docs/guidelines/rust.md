# Rust Guidelines

Applies to: `iroh-http-core`, `iroh-http-discovery`.

For engineering values and invariants, see [principles.md](../principles.md).
For internal implementation details, see [internals/](../internals/).

---

## Naming

| Scope | Convention | Example |
|-------|------------|---------|
| Functions | `snake_case` | `next_chunk`, `alloc_body_writer` |
| Types / Structs | `PascalCase` | `IrohEndpoint`, `ServeHandle` |
| Constants | `UPPER_SNAKE` | `ALPN`, `READ_BUF` |
| Modules | `snake_case` | `client`, `server`, `stream` |

---

## Visibility

- **`pub`** — part of the crate's API, consumed by platform adapters. Every `pub` item requires a doc comment.
- **`pub(crate)`** — shared across modules but invisible to adapters. Use freely for internal machinery.
- **Private** — default. Only promote when genuinely needed.

Platform adapters depend only on `pub` items. If an adapter needs something `pub(crate)`, promote it deliberately and document why.

---

## Error Handling

Use `anyhow::Result` internally. Use `anyhow::Context` to attach context when propagating errors — never strip the source.

At the FFI boundary, convert errors to the structured JSON envelope via `classify_error_json`. This produces `{"code":"TIMEOUT","message":"..."}` — a stable, machine-readable format platform layers can dispatch on without string matching.

When a new failure mode is added, add a new error code. Never rely on the catch-all `NETWORK` code for something that has a distinct cause.

---

## Async

- All I/O is `async`. The crate does not own its runtime — it runs on whatever runtime the platform adapter starts (always Tokio multi-thread).
- Background tasks use `tokio::spawn`. Track the `JoinHandle` for tasks that must be cancelled on shutdown.
- Shutdown signaling uses `tokio::sync::Notify`. Do not use `watch` channels for shutdown — dropping a `watch::Sender` causes spurious wakeups.
- Wrap every async I/O operation in a timeout. An await without a bound is a latent hang.

---

## Doc Comments

Every `pub` item gets a `///` doc comment answering:

1. What does this do?
2. What does it expect (inputs, preconditions)?
3. What does it return / what can go wrong?

Use `//!` module-level docs at the top of each file to explain the module's role and design intent.

---

## Testing

- Unit tests: `#[cfg(test)] mod tests` inside each module.
- Integration tests: `tests/` directory, exercising two real Iroh nodes over real QUIC connections.
- Use `#[tokio::test]` for async tests.
- Tests must be deterministic. No `sleep`-based timing — use `Notify` / `oneshot` for synchronization.
- Test failure paths and hostile inputs, not just happy paths. Every limit has a test that exceeds it.

---

## FFI payload field naming

Applies to every `pub struct` in `crates/iroh-http-core/src/ffi/types.rs` and any future FFI-boundary struct.

**Rule: the struct name carries the domain; fields do not repeat it.**

`RequestPayload` fields describe the incoming request plus the FFI handles the adapter needs to respond:

| Field suffix | Meaning | Example |
|---|---|---|
| `_handle` | Opaque `u64` slotmap key for a *read-only* body source | `req_body_handle` |
| `_body_handle` | Same — body-specific alias for clarity when both a request and a response handle are present in the same struct | `req_body_handle`, `res_body_handle` |
| no suffix | Plain data value (string, bool, vec) | `method`, `url`, `headers` |

`FfiResponse` fields describe the resolved response:

| Field | Type | Notes |
|---|---|---|
| `status` | `u16` | HTTP status code |
| `headers` | `Vec<(String, String)>` | Name-value pairs |
| `body_handle` | `u64` | Handle to a `BodyReader`; `0` = no body (204/205/304) |
| `url` | `String` | Full `httpi://` URL of the responding peer |

**Disambiguation rule:** when a single struct carries handles for *both* sides (e.g. `RequestPayload` carries both a request body *source* and a response body *sink*), prefix with `req_` / `res_` respectively. Within `FfiResponse` (response-only), no prefix is needed.

**Reader vs. writer:** a handle that adapters read *from* ends in `_body_handle` (source). A handle that adapters write *to* is named `res_body_handle` in `RequestPayload` (sink). New handle fields must follow this pattern and note in their doc comment whether they are sources or sinks.

**No new handle types without a doc comment.** Every `u64` handle field must explain what slotmap type it keys into and what "0" means.
