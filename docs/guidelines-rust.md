# Rust Core Guidelines

Applies to: `iroh-http-core`, `iroh-http-framing`, `iroh-http-discovery`.

---

## Architecture layers

```
iroh-http-framing   (portable wire layer; target: no_std + alloc) — wire format only
iroh-http-core      (std + tokio + iroh)                           — endpoint, client, server, streams
iroh-http-discovery (std + iroh, DNS/mDNS)                         — peer discovery
```

Each crate has a single responsibility. `framing` parses and serializes the
wire format without any I/O. `core` owns the `IrohEndpoint`, connection
pool, fetch/serve logic, and body/trailer slab. `discovery` configures DNS
and mDNS resolution on top of Iroh's discovery traits.

---

## Naming

| Scope            | Convention       | Example                               |
| ---------------- | ---------------- | ------------------------------------- |
| Functions        | `snake_case`     | `next_chunk`, `alloc_body_writer`     |
| Types / Structs  | `PascalCase`     | `IrohEndpoint`, `ServeHandle`         |
| Constants        | `UPPER_SNAKE`    | `ALPN`, `READ_BUF`                    |
| Modules          | `snake_case`     | `client`, `server`, `stream`          |
| Crate-internal   | `pub(crate)`     | `pool`, `qpack_bridge`                |

---

## Visibility boundaries

- **`pub`** — part of the crate's API, consumed by platform adapters
  (napi-rs, PyO3, Tauri commands, Deno FFI). Every `pub` item needs a doc
  comment.
- **`pub(crate)`** — shared across modules within the crate but invisible to
  adapters. Use freely for internal machinery (connection pool, QPACK
  bridge).
- **Private** — everything else. Default to private; promote only when
  needed.

Platform adapters must only depend on `pub` items. If an adapter needs
something currently `pub(crate)`, promote it deliberately and document why.

---

## Error handling

The crates use `anyhow::Result` internally and `String`-based errors at the
FFI boundary.

**Internal code:** use `anyhow::Context` to attach context to errors.

**FFI boundary:** convert errors to strings, then classify via
`classify_error_json`:

```rust
pub fn classify_error_json(e: impl std::fmt::Display) -> String
```

This produces `{"code":"TIMEOUT","message":"..."}` — a stable,
machine-readable envelope that platform layers can dispatch on without
regex-matching error messages.

**Error codes** (in `classify_error_code`):

| Code                | Trigger pattern                                |
| ------------------- | ---------------------------------------------- |
| `TIMEOUT`           | "timed out", "timeout", "deadline"             |
| `DNS_FAILURE`       | "dns", "resolv"                                |
| `ALPN_MISMATCH`     | "alpn"                                         |
| `UPGRADE_REJECTED`  | "upgrade" + "reject", "non-101"                |
| `PARSE_FAILURE`     | "parse" + "response head" / "request head"     |
| `TOO_MANY_HEADERS`  | "too many headers"                             |
| `INVALID_HANDLE`    | "invalid"/"unknown" + "handle"                 |
| `WRITER_DROPPED`    | "writer dropped"                               |
| `READER_DROPPED`    | "reader dropped"                               |
| `STREAM_RESET`      | "reset", "closed", "finish"                    |
| `NETWORK`           | catch-all                                       |

When adding new failure modes, add a new code rather than relying on
catch-all.

---

## Async conventions

- Everything I/O-bound is `async`.
- Runtime: Tokio (multi-threaded). The crate does not manage its own
  runtime — it runs on whatever runtime the platform adapter starts.
- Background tasks use `tokio::spawn`. Per-request tasks are spawned inside
  the serve loop and governed by a semaphore for concurrency control.
- Shutdown signaling uses `tokio::sync::Notify` (not `watch` channels —
  dropping a `watch::Sender` triggers spurious wakeups).

---

## Body and trailer slab

Body handles are `u32` indices into global slab maps (`OnceLock<Mutex<HashMap>>`).

**Rules:**
- `insert_*` allocates a slot and returns the handle.
- `remove_*` extracts and drops the slot. Each handle is removed exactly
  once.
- Handles are an internal transport detail. They appear in FFI signatures
  but must never be exposed in user-facing APIs on any platform.
- `cancel_reader` and `cancel_in_flight` are the cleanup paths for
  cancellation.

---

## Connection pool

`ConnectionPool` (`pub(crate)`) provides connection reuse and storm
prevention:

- Keyed by `NodeId`.
- Idle connections are reused if the QUIC connection is still alive.
- A `DashMap<NodeId, Notify>` coalesces concurrent connection attempts to
  the same peer (storm prevention).
- Pool capacity is bounded; over-limit entries are evicted LRU.

---

## Security defaults

All resource limits are enforced in the serve loop and are conservative by
default:

| Limit                              | Default | Config field                     |
| ---------------------------------- | ------- | -------------------------------- |
| Max concurrent requests            | 64      | `ServeOptions::max_concurrency`  |
| Per-request timeout                | 60 s    | `ServeOptions::request_timeout_secs` |
| Per-peer connection limit          | 8       | `ServeOptions::max_connections_per_peer` |
| Max request head size              | 64 KB   | hardcoded in framing             |
| Max request body size              | none    | `ServeOptions::max_request_body_bytes` |
| Drain timeout (graceful shutdown)  | 30 s    | `ServeOptions::drain_timeout_secs` |

Defaults are safe against hostile peers without opt-in. Increasing limits is
always opt-in.

---

## Embedded portability strategy (`iroh-http-framing`)

`iroh-http-framing` is the wire-level contract layer for future embedded
targets. The target state is `no_std + alloc` so embedded/WASM implementations
can reuse it directly.

Rules:

- Keep the crate free of runtime and transport concerns (`tokio`, sockets,
  endpoint state machines).
- Prefer dependencies that are `no_std`-compatible.
- If a `std`-only dependency is adopted temporarily for robustness, document
  the reason and migration plan in `docs/embedded-roadmap.md`.
- Never couple framing to `iroh` internals; framing must remain pure
  parse/serialize logic.

Examples:

- Allowed: parser/codec crates, byte/container utilities.
- Forbidden in framing: `tokio`, `std::net`, `std::io`, discovery, transport.

---

## Doc comments

Every `pub` item gets a `///` doc comment that answers:

1. What does this do?
2. What does it expect (inputs, preconditions)?
3. What does it return / what can go wrong?

Use `//!` module-level docs at the top of each file to explain the module's
role and design intent.

Internal (`pub(crate)` / private) items need enough context for a
contributor to understand the design intent, but don't need the same level
of polish.

---

## Testing

- Unit tests live in `#[cfg(test)] mod tests` inside each module.
- Integration tests live in `tests/` and exercise two real Iroh nodes
  over real QUIC connections.
- Use `#[tokio::test]` for async tests.
- Every security limit (patch 14) has a test that exceeds the limit and
  verifies the response.
- Graceful shutdown tests verify drain behaviour and in-flight request
  completion.
- Tests must be deterministic — no `sleep`-based timing, use `Notify` /
  `oneshot` for synchronization.
