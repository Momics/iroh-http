---
status: skip
---

# iroh-http — Patch 16: Integration Test Suite

## Problem

The project has 16 patches, 3 platform adapters, a core Rust crate, and a
framing crate — but no integration tests. Patch 11 (open-source readiness)
mentions CI but doesn't specify what the tests actually are.

Before open-sourcing, the minimum bar is: can two nodes talk to each other?
Currently, the only way to verify this is to manually run the examples.

---

## Design

### Tier 1: Rust integration tests (`iroh-http-core`)

These run in `cargo test` with no JS, no native addons, no platform
dependencies. They exercise the Rust core directly — the same code path that
all platform adapters sit on top of.

**File:** `crates/iroh-http-core/tests/integration.rs`

#### Test cases

| Test | Description |
|---|---|
| `test_fetch_simple` | Node A fetches `/hello` from Node B. Assert status 200 and body `"hello"`. |
| `test_fetch_json` | POST JSON body, receive JSON response. Assert headers and parsed body. |
| `test_fetch_streaming_body` | Node A sends a multi-chunk request body. Node B echoes it back. Assert all chunks round-trip correctly. |
| `test_fetch_large_body` | Send 1 MB body in 64 KB chunks. Assert full body received without corruption. |
| `test_concurrent_fetches` | 10 parallel fetches from A to B. All should succeed. |
| `test_bidirectional_stream` | Open a duplex stream. Write from both sides. Assert both sides receive data. |
| `test_trailers` | Send request with trailers. Server reads them. Server sends response with trailers. Client reads them. |
| `test_fetch_cancel` | Start a slow fetch, cancel via token. Assert the fetch returns an abort error within 1 second. |
| `test_serve_concurrency_limit` | Set max_concurrency=2. Open 3 concurrent requests. The 3rd should succeed after one of the first two completes. |
| `test_mutual_fetch` | A serves and fetches from B. B serves and fetches from A. Both directions work simultaneously. |
| `test_unknown_peer` | Fetch to a non-existent NodeId. Should return a connection error, not hang. |
| `test_node_close` | Close node A while B is connected. B should observe a connection drop. |

#### Test harness

A small helper that spins up two `IrohEndpoint`s in the same process:

```rust
async fn test_pair() -> (IrohEndpoint, IrohEndpoint) {
    let a = IrohEndpoint::bind(NodeOptions::default()).await.unwrap();
    let b = IrohEndpoint::bind(NodeOptions::default()).await.unwrap();
    (a, b)
}
```

Since both nodes run on localhost with the default relay, they'll discover
each other immediately. No network setup needed.

For serve tests, a helper that registers a handler and returns the serve
join handle:

```rust
async fn serve_echo(endpoint: &IrohEndpoint) -> tokio::task::JoinHandle<()> {
    let ep = endpoint.clone();
    serve(ep, ServeOptions::default(), |payload| {
        // Respond with echoed body
        respond(payload.req_handle, 200, vec![]);
    })
}
```

### Tier 2: Framing unit tests (`iroh-http-framing`)

The framing crate already has doc tests. Add property-based round-trip tests:

**File:** `crates/iroh-http-framing/tests/roundtrip.rs`

| Test | Description |
|---|---|
| `test_request_roundtrip` | Serialize then parse. Assert method, path, headers match. |
| `test_response_roundtrip` | Serialize then parse. Assert status, reason, headers match. |
| `test_chunked_roundtrip` | Encode chunks, parse them back. Assert data matches. |
| `test_trailer_roundtrip` | Serialize trailers, parse them back. Assert match. |
| `test_max_headers` | 64 headers (the parser buffer limit). Should succeed. |
| `test_oversized_headers` | 65+ headers. Should fail gracefully, not panic. |
| `test_malformed_input` | Various garbage bytes. Should return `FramingError::Parse`, not panic. |

### Tier 3: Platform smoke tests (CI only)

These verify the Node/Tauri/Deno adapters compile and can do a basic
round-trip. They run in CI, not in `cargo test`.

| Platform | Test |
|----------|------|
| Node | `node -e "const {createNode} = require('..'); ..."` — create node, fetch self, assert response |
| Deno | `deno run --allow-ffi test.ts` — same |
| Tauri | Build the plugin, run a Tauri integration test (or defer to manual) |

Platform tests are less critical — if the Rust core works, the adapters are
thin wiring. But a basic smoke test catches build regressions.

---

## CI configuration

**File:** `.github/workflows/ci.yml`

```yaml
name: CI

on: [push, pull_request]

jobs:
  rust-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings
      - run: cargo fmt --check

  node-smoke:
    runs-on: ubuntu-latest
    needs: rust-tests
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - run: cd packages/iroh-http-node && npm install && npm run build
      - run: cd packages/iroh-http-node && node --test test/smoke.mjs
```

---

## Scope of changes

| Layer | Change |
|---|---|
| `crates/iroh-http-core/tests/integration.rs` (new) | 12 integration tests with test harness. |
| `crates/iroh-http-framing/tests/roundtrip.rs` (new) | 7 round-trip and edge case tests. |
| `packages/iroh-http-node/test/smoke.mjs` (new) | Node.js smoke test. |
| `.github/workflows/ci.yml` (new) | CI workflow: cargo test, clippy, fmt, node smoke. |

---

## Verification

The tests themselves are the verification. Success criteria:

1. `cargo test --workspace` passes with 0 failures.
2. CI workflow runs green on a fresh PR.
3. A deliberate regression (e.g. break the header parser) causes a test
   failure with a clear error message.
