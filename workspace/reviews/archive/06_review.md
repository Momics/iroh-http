---
status: resolved
source: deep-rust-audit (crates + rust adapters), compared against reviews/04 and 05
date: 2026-04-11
---

# Rust Core Deep Audit — Delta from Reviews 04 & 05

This review intentionally focuses on **new findings** not already covered in
`04_review.md` (guideline/API audit) and `05_review.md` (patch/features audit).

## Tracker (Resolved vs Unresolved)

Last checked: **2026-04-11**

| ID | Finding (short) | Priority | Status |
|---|---|---|---|
| R6-01 | Server request-body parser always assumes chunked framing | P0 | RESOLVED |
| R6-02 | `pending_responses` leak risk on timeout/cancel paths | P0 | RESOLVED |
| R6-03 | Session handles not isolated (shared underlying connection) | P1 | RESOLVED |
| R6-04 | Session slab leak after remote-side closure | P1 | RESOLVED |
| R6-05 | Timeout ms→secs truncation to zero + doc mismatch | P1 | RESOLVED |
| R6-06 | Invalid key material handling is silently permissive | P1 | RESOLVED |
| R6-07 | Stream backpressure config is global mutable state | P2 | RESOLVED |
| R6-08 | Slab sweep task can be spawned repeatedly | P2 | RESOLVED |
| R6-09 | Pool eviction policy is not LRU | P2 | RESOLVED |
| R6-10 | Error envelope implementation fragility | P2 | RESOLVED |
| R6-11 | Invalid direct addresses are silently dropped | P2 | RESOLVED |

Status conventions:
- `RESOLVED`: fixed and verified in code/tests.
- `PARTIAL`: some fixes landed, follow-up still required.
- `UNRESOLVED`: no fix merged yet.

## Overlap Check

The following items from 04/05 were re-validated and are **not duplicated** here:

- Compression implementation is bulk-buffered, not streaming.
- `min_body_bytes` is currently not enforced.
- Custom DNS resolver field exists but is not applied in endpoint bind logic.
- CI does not run the full Rust test suite.
- Python parity gaps (session lifecycle/discovery/export surface).

## Net-New Findings

### 1) P0 — Server request-body parser always assumes chunked framing

> ✅ **RESOLVED** — `dispatch_request` now inspects `Transfer-Encoding` and
> routes chunked requests through `pump_recv_to_body` (chunk-parsing) and
> raw/fixed-length requests through `pump_recv_raw_to_body_limited` (byte-limit
> enforced, no chunk parsing). Fixed in `refactor: Rust quality improvements
> batch` (ce24f81).

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

> ✅ **RESOLVED** — `PendingGuard` RAII drop guard added in `dispatch_request`.
> The guard removes the map entry on drop if the task is cancelled or errors
> before `respond()` is called. `.defuse()` is called after `rx.await` succeeds
> to prevent double-removal. Fixed in `refactor: Rust quality improvements
> batch` (ce24f81).

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

> ✅ **RESOLVED** — `session_connect` no longer uses the shared connection pool.
> Each call now dials a fresh dedicated QUIC connection so that `session_close`
> (which sends CONNECTION_CLOSE) can never affect sibling session handles.
> Fetch operations continue to use the pool. Fixed in
> `fix(session): R6-03 — each session_connect gets a dedicated QUIC connection`.

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

> ✅ **RESOLVED** — `session_closed()` now calls `try_remove` on the slab
> after `conn.closed().await` returns, freeing the handle regardless of whether
> explicit `session_close()` was also called. Fixed in `refactor: Rust quality
> improvements batch` (ce24f81).

**Files:**
- `crates/iroh-http-core/src/session.rs:188`
- `crates/iroh-http-core/src/session.rs:167`

`session_closed()` awaits close info but does not remove the session from slab.
Only explicit `session_close()` removes slab entries. Remote-initiated closure
therefore leaves stale handles unless caller also invokes explicit close.

### 5) P1 — Timeout unit conversion truncates sub-second values to zero

> ✅ **RESOLVED** — `request_timeout_ms` is now converted with
> `Duration::from_millis` everywhere (not integer division by 1000).
> Sub-millisecond values are not relevant since the field is in whole ms.
> Doc comment updated: `None` now correctly documents the 60 000 ms default
> and `Some(0)` as the explicit disable path. Fixed in `fix: R6-05 doc +
> R6-06 strict key validation`.

**Files:**
- `crates/iroh-http-core/src/endpoint.rs:271`
- `crates/iroh-http-core/src/server.rs:142`

`request_timeout_ms` is converted to seconds using integer division in
`serve_options()`. Values `1..999ms` become `0s`, effectively disabling
timeouts for callers expecting small non-zero timeouts.

Also, `ServeOptions` docs claim `None` disables timeout, while runtime applies
a 60s default when not provided.

### 6) P1 — Invalid key material handling is silently permissive across adapters

> ✅ **RESOLVED** — Tauri and Deno now fail-fast on invalid key material.
> Tauri: closure returns `Result<NodeOptions, String>` propagated with
> `.transpose()?`. Deno: early-return match blocks. Node and Python were
> already correct (`try_into().map_err(...)?`). Fixed in `fix: R6-05 doc +
> R6-06 strict key validation`.

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

> ✅ **RESOLVED** — `configure_backpressure()` is now idempotent via a
> `BACKPRESSURE_CONFIGURED` `AtomicBool`. Only the first call (first endpoint
> bind) takes effect; subsequent calls are no-ops, preventing a second endpoint
> from clobbering channel settings for an already-running endpoint. Fixed in
> `fix: R6-07, R6-08, R6-10, R6-11`.

**Files:**
- `crates/iroh-http-core/src/stream.rs:31`
- `crates/iroh-http-core/src/stream.rs:40`
- `crates/iroh-http-core/src/endpoint.rs:223`

Backpressure controls are process-global atomics. Binding a new endpoint can
change channel behavior for already-running endpoints in the same process.

### 8) P2 — Slab sweep task can be spawned repeatedly (one per bind)

> ✅ **RESOLVED** — `start_slab_sweep()` is now guarded by a `SWEEP_STARTED`
> `AtomicBool`. The first call spawns the sweep task; all subsequent calls
> return immediately. Fixed in `fix: R6-07, R6-08, R6-10, R6-11`.

**Files:**
- `crates/iroh-http-core/src/stream.rs:322`
- `crates/iroh-http-core/src/endpoint.rs:230`

Each endpoint bind calls `start_slab_sweep`, which spawns an untracked infinite
task. Multi-endpoint processes can accumulate duplicate sweepers and duplicate
log emissions.

### 9) P2 — Connection pool eviction policy is not LRU

> ✅ **RESOLVED** — `Slot::Ready` now carries a `std::time::Instant` timestamp
> updated on every cache hit. `evict_if_needed` uses `min_by_key` on the
> timestamp to evict the oldest idle connection. Fixed in
> `fix(pool): LRU eviction` (f75be1d).

**Files:**
- `crates/iroh-http-core/src/pool.rs:188`
- `crates/iroh-http-core/src/pool.rs:207`

Pool docs/guidelines describe LRU-style behavior, but implementation evicts
arbitrary ready entries from `HashMap` iteration order once over limit.

This makes cache behavior non-deterministic and can evict hot connections.

### 10) P2 — Error envelope implementation has fragility points

> ✅ **RESOLVED** — `classify_error_json` now uses `serde_json::Value::String`
> to serialise the message, which handles all control characters, null bytes,
> and Unicode escapes correctly. Fixed in `fix: R6-07, R6-08, R6-10, R6-11`.

**Files:**
- `crates/iroh-http-core/src/lib.rs:67`
- `crates/iroh-http-core/src/lib.rs:70`
- `crates/iroh-http-core/src/lib.rs:110`

`classify_error_json` manually escapes only a subset of JSON-unsafe chars.
This is brittle versus serializer-based encoding. Catch-all is currently
`UNKNOWN`, while guideline docs describe a network-oriented catch-all pattern,
which can cause classification drift at platform boundaries.

### 11) P2 — Address parsing silently drops invalid direct addresses

> ✅ **RESOLVED** — `parse_direct_addrs` now returns
> `Result<Option<Vec<SocketAddr>>, String>`. Invalid address strings return
> an `Err` instead of being silently dropped. All three binding crates
> propagate the error to callers. Fixed in `fix: R6-07, R6-08, R6-10, R6-11`.

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
