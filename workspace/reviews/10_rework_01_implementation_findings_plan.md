# Rework 01 Implementation Findings Plan

Date: 2026-04-12  
Scope: post-implementation quality assessment against `workspace/rework_01`

## Findings (accepted)

### 1) [P0] Duplex upgrade path returns `101` but does not wire data channels
- File: `crates/iroh-http-core/src/server.rs:289-349`
- Impact: silent functional regression in duplex mode (handshake succeeds, stream semantics fail).

### 2) [P1] `max_concurrency` is enforced per connection, not per request
- File: `crates/iroh-http-core/src/server.rs:512-575`
- Impact: request-level fairness and DoS controls are weaker than configured semantics.

### 3) [P1] Small `max_header_size` can still break in `raw_connect` path
- File: `crates/iroh-http-core/src/client.rs:440-442`
- Impact: low header limits can destabilize duplex handshake path.

### 4) [P1] Compression feature build is broken in adapter crates
- File: `packages/iroh-http-node/src/lib.rs:168-174` (same pattern in deno/py adapters)
- Impact: advertised compression feature is not buildable end-to-end.

### 5) [P1] Tauri bridge was not migrated to `bigint` handles
- File: `packages/iroh-http-tauri/guest-js/index.ts:65-99`
- Impact: cross-platform bridge contract mismatch; workspace typecheck fails.

---

## Fix Plan (execution order)

## Phase A: unblock correctness and CI
1. Fix P0 duplex data path in `server.rs` so upgraded IO is fully wired to the existing exported handles.
2. Add duplex integration tests that verify:
   - bidirectional data flow after `101`
   - graceful close behavior
   - no handle orphaning
3. Clamp `raw_connect` header buffer settings (`max_buf_size >= 8192`) for parity with `fetch()`.

## Phase B: restore contract integrity across adapters
1. Repair compression feature wiring in node/deno/py adapters to match current core API.
2. Migrate Tauri guest bridge handle types from `number` to `bigint` across bridge + session + fetch/serve paths.
3. Re-run workspace TS typecheck and compression-enabled Rust build.

## Phase C: enforce requested server semantics
1. Move concurrency gating from connection-scope to request-scope (or clearly document and rename if intentionally connection-scope, not recommended).
2. Add a deterministic test that proves request-level limit behavior under many streams on a single QUIC connection.

---

## Acceptance gates

All of the following must pass:

```bash
cargo test -p iroh-http-core
cargo test -p iroh-http-core --features compression
cargo check --workspace --features compression
npm run typecheck --workspaces
```

Additional required regression tests:
- Duplex upgrade round-trip over `raw_connect` (read and write).
- Request-level concurrency limit under multi-stream single-connection load.
- Header-limit clamp parity test for `raw_connect`.

---

## Release decision

Do not release until Findings 1-5 are resolved and acceptance gates pass.
