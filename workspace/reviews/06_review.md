---
status: reported
source: deep-rust-audit (crates + rust adapters), compared against reviews/04 and 05
date: 2026-04-11
---

# Rust Core Deep Audit — Delta from Reviews 04 & 05

This review intentionally focuses on **new findings** not already covered in
`04_review.md` (guideline/API audit) and `05_review.md` (patch/features audit).

## Overlap Check

The following items from 04/05 were re-validated and are **not duplicated** here:

- Compression implementation is bulk-buffered, not streaming.
- `min_body_bytes` is currently not enforced.
- Custom DNS resolver field exists but is not applied in endpoint bind logic.
- CI does not run the full Rust test suite.
- Python parity gaps (session lifecycle/discovery/export surface).

## Net-New Findings

### 1) P0 — Server request-body parser always assumes chunked framing

**Files:**
- `crates/iroh-http-core/src/server.rs:433`
- `crates/iroh-http-core/src/server.rs:595`
- `crates/iroh-http-core/src/server.rs:608`
- `crates/iroh-http-core/src/server.rs:642`

`handle_stream` always routes non-duplex requests into `pump_recv_to_body`, and
that function always attempts `parse_chunk_header` framing. There is no branch
for raw/fixed-length request bodies.

Impacts:

- Raw bodies can be misparsed if payload bytes look like chunk headers.
- `max_request_body_bytes` checks are tied to parsed chunk sizes and can be
  bypassed on non-chunked fallback paths.
- Behavior diverges from response-side logic, which correctly distinguishes
  chunked vs non-chunked pumping.

### 2) P0 — `pending_responses` can leak on timeout/cancel paths

**Files:**
- `crates/iroh-http-core/src/server.rs:102`
- `crates/iroh-http-core/src/server.rs:111`
- `crates/iroh-http-core/src/server.rs:340`
- `crates/iroh-http-core/src/server.rs:540`

Request handles are inserted into a global map and removed only via
`respond(req_handle, ...)`. If a stream future times out or is dropped before
JS responds, map entries can remain indefinitely.

This is a latent memory/resource leak and can eventually cause req-handle churn
and lookup overhead.

### 3) P1 — Session handles are not isolated; one close can terminate sibling handles

**Files:**
- `crates/iroh-http-core/src/session.rs:82`
- `crates/iroh-http-core/src/session.rs:92`
- `crates/iroh-http-core/src/session.rs:166`

`session_connect` reuses pooled connections and inserts cloned connection
objects into new session handles. Multiple session handles can therefore point
to the same underlying QUIC connection. `session_close` closes that connection,
so closing one handle can close others unexpectedly.

This is surprising API behavior for a session abstraction.

### 4) P1 — Session slab entries can leak after remote-side closure

**Files:**
- `crates/iroh-http-core/src/session.rs:188`
- `crates/iroh-http-core/src/session.rs:167`

`session_closed()` awaits close info but does not remove the session from slab.
Only explicit `session_close()` removes slab entries. Remote-initiated closure
therefore leaves stale handles unless caller also invokes explicit close.

### 5) P1 — Timeout unit conversion truncates sub-second values to zero

**Files:**
- `crates/iroh-http-core/src/endpoint.rs:271`
- `crates/iroh-http-core/src/server.rs:142`

`request_timeout_ms` is converted to seconds using integer division in
`serve_options()`. Values `1..999ms` become `0s`, effectively disabling
timeouts for callers expecting small non-zero timeouts.

Also, `ServeOptions` docs claim `None` disables timeout, while runtime applies
a 60s default when not provided.

### 6) P1 — Invalid key material handling is silently permissive across adapters

**Files:**
- `packages/iroh-http-node/src/lib.rs:128`
- `packages/iroh-http-tauri/src/commands.rs:73`
- `packages/iroh-http-deno/src/dispatch.rs:169`
- `packages/iroh-http-py/src/lib.rs:614`

Adapters do not consistently reject malformed key lengths:

- Node pads/truncates to 32 bytes.
- Tauri/Deno/Python decode paths can fall back to `None` and generate a new key.

This can silently change node identity instead of failing fast.

### 7) P2 — Stream backpressure config is global mutable state across all endpoints

**Files:**
- `crates/iroh-http-core/src/stream.rs:31`
- `crates/iroh-http-core/src/stream.rs:40`
- `crates/iroh-http-core/src/endpoint.rs:223`

Backpressure controls are process-global atomics. Binding a new endpoint can
change channel behavior for already-running endpoints in the same process.

### 8) P2 — Slab sweep task can be spawned repeatedly (one per bind)

**Files:**
- `crates/iroh-http-core/src/stream.rs:322`
- `crates/iroh-http-core/src/endpoint.rs:230`

Each endpoint bind calls `start_slab_sweep`, which spawns an untracked infinite
task. Multi-endpoint processes can accumulate duplicate sweepers and duplicate
log emissions.

### 9) P2 — Connection pool eviction policy is not LRU

**Files:**
- `crates/iroh-http-core/src/pool.rs:188`
- `crates/iroh-http-core/src/pool.rs:207`

Pool docs/guidelines describe LRU-style behavior, but implementation evicts
arbitrary ready entries from `HashMap` iteration order once over limit.

This makes cache behavior non-deterministic and can evict hot connections.

### 10) P2 — Error envelope implementation has fragility points

**Files:**
- `crates/iroh-http-core/src/lib.rs:67`
- `crates/iroh-http-core/src/lib.rs:70`
- `crates/iroh-http-core/src/lib.rs:110`

`classify_error_json` manually escapes only a subset of JSON-unsafe chars.
This is brittle versus serializer-based encoding. Catch-all is currently
`UNKNOWN`, while guideline docs describe a network-oriented catch-all pattern,
which can cause classification drift at platform boundaries.

### 11) P2 — Address parsing silently drops invalid direct addresses

**Files:**
- `packages/iroh-http-node/src/lib.rs:35`
- `packages/iroh-http-tauri/src/commands.rs:17`
- `packages/iroh-http-deno/src/dispatch.rs:30`

Adapters filter invalid address strings out with `filter_map(parse().ok())`
instead of returning a user-visible error. This hides misconfiguration.

## Empirical Validation (this audit run)

### Commands

1. `cargo check --workspace`  
2. `cargo test -p iroh-http-core --tests`  
3. `cargo test --workspace --all-targets --no-fail-fast`  
4. `cargo clippy --workspace --all-targets --all-features -- -D warnings`

### Results

- `cargo check --workspace`: pass (warning in Deno dispatch: unused compression
  fields when feature is disabled).
- `cargo test -p iroh-http-core --tests`: failure in
  `force_close_aborts_immediately` (`integration.rs:1127`), observed duration
  just over 1s.
- `cargo test --workspace --all-targets --no-fail-fast`: failures in
  `session_multiple_bidi_streams` timeout (`bidi_stream.rs:118`) and the same
  `force_close_aborts_immediately` check.
- `clippy -D warnings`: fails in `iroh-http-framing` (`type_complexity`) and
  `iroh-http-discovery` (`never_loop`), so workspace is not clippy-clean under
  strict settings.

## Recommended Fix Order

1. Fix server request-body framing mode split + limit enforcement (P0).
2. Add guaranteed cleanup for `pending_responses` on all terminal paths (P0).
3. Clarify/enforce session ownership semantics and slab lifecycle cleanup (P1).
4. Make key parsing strict and fail-fast across all adapters (P1).
5. Remove global mutable cross-endpoint knobs in stream subsystem (P2).
6. Align pool behavior with documented LRU expectation (P2).
