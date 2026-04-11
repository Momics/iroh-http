---
status: done
refs: features/server-limits.md
---

# Patch 28 — Expose Server Limits in TypeScript `ServeOptions`

Wire the five `ServeOptions` fields already implemented in
`crates/iroh-http-core/src/server.rs` through to the TypeScript `serve()`
call, as described in [server-limits.md](../features/server-limits.md).

## Problem

`ServeOptions` in Rust has `max_concurrency`, `max_connections_per_peer`,
`request_timeout_secs`, `max_request_body_bytes`, and `max_header_size` — all
implemented and defaulted. But every platform adapter passes
`Default::default()` for these fields (or only `max_consecutive_errors`).
Developers cannot configure them without forking the library.

## Changes

### 1. TypeScript — `packages/iroh-http-shared/src/bridge.ts`

Add to the existing `NodeOptions` interface (serve options live here since
`serve()` takes `NodeOptions`-derived options):

```ts
interface NodeOptions {
  // ... existing fields ...

  // ── Server limits ──────────────────────────────────────────────────────
  /** Maximum simultaneous in-flight requests, all peers combined. Default: 64. */
  maxConcurrency?: number;
  /** Maximum simultaneous connections from a single peer. Default: 8. */
  maxConnectionsPerPeer?: number;
  /** Per-request timeout in milliseconds. Default: 60 000. Set to 0 to disable. */
  requestTimeout?: number;
  /** Reject request bodies larger than this many bytes with 413. Default: no limit. */
  maxRequestBodyBytes?: number;
  /** Reject request header blocks larger than this many bytes with 431. Default: 65536. */
  maxHeaderBytes?: number;
}
```

### 2. Node.js adapter — `packages/iroh-http-node/src/lib.rs`

In `raw_serve`, read the limit fields from the endpoint options and populate
`ServeOptions`:

```rust
pub fn raw_serve(endpoint_handle: u32, ...) {
    let ep = get_endpoint(endpoint_handle);
    let opts = ep.options();

    let serve_opts = ServeOptions {
        max_concurrency:          opts.max_concurrency.map(|n| n as usize),
        max_connections_per_peer: opts.max_connections_per_peer.map(|n| n as usize),
        request_timeout_secs:     opts.request_timeout_ms.map(|ms| ms / 1000),
        max_request_body_bytes:   opts.max_request_body_bytes.map(|n| n as usize),
        max_consecutive_errors:   Some(ep.max_consecutive_errors()),
        ..Default::default()
    };
    // max_header_size lives on the endpoint, already used in client.rs
    // ↑ already wired; no change needed there

    iroh_http_core::serve(ep.inner(), serve_opts, callback);
}
```

### 3. Store the limit fields on the endpoint

The `IrohEndpointOptions` struct passed from JS to Rust at node creation needs
the five new fields:

```rust
// crates/iroh-http-core/src/endpoint.rs (or bridge equivalent)
pub struct IrohEndpointOptions {
    // ... existing ...
    pub max_concurrency: Option<u32>,
    pub max_connections_per_peer: Option<u32>,
    pub request_timeout_ms: Option<u64>,
    pub max_request_body_bytes: Option<u64>,
    // max_header_size already exists
}
```

### 4. Repeat for Deno, Tauri, Python adapters

Each adapter's `raw_serve` / equivalent entry point reads the same fields from
the options struct passed at node creation and forwards them to `ServeOptions`.

### 5. Tests

Add to `crates/iroh-http-core/tests/integration.rs`:

```rust
#[tokio::test]
async fn server_rejects_oversized_body() {
    // serve with maxRequestBodyBytes: 100
    // send 1000 byte body → expect 413
}

#[tokio::test]
async fn server_rejects_when_concurrency_exceeded() {
    // serve with maxConcurrency: 1
    // open 2 simultaneous requests → second gets 503
}

#[tokio::test]
async fn server_times_out_slow_request() {
    // serve with requestTimeout: 100ms
    // handler stalls → expect 408
}
```

## Files

- `packages/iroh-http-shared/src/bridge.ts` — five new `NodeOptions` fields
- `packages/iroh-http-node/src/lib.rs` — populate `ServeOptions` in `raw_serve`
- `packages/iroh-http-deno/src/` — same
- `packages/iroh-http-tauri/src/` — same
- `packages/iroh-http-py/src/` — same
- `crates/iroh-http-core/src/endpoint.rs` (or bridge) — store new fields on options struct
- `crates/iroh-http-core/tests/integration.rs` — three new tests

## Notes

- `requestTimeout` is in milliseconds in TypeScript (consistent with all JS
  timeout APIs) and converted to seconds in Rust.
- `max_header_size` is already stored on the endpoint and used in `client.rs`.
  This patch reuses the same field for the server path — no new storage needed.
- `drain_timeout_secs` (graceful shutdown wait) is already correctly wired via
  `drainTimeout` in the existing options. No change needed.
