# TEST_PLAN.md — iroh-http Test Strategy

Test where the bugs are. This plan is derived from analysing 106 closed issues
and their root causes based on the actual bug history of this codebase.

---

## 1. Bug Profile

| Root cause | Count | Best defence |
|---|---|---|
| docs-drift | 25 | Not a testing problem — specification.md now exists |
| ffi-boundary | 19 | **Per-adapter integration tests** |
| api-surface | 15 | Per-adapter integration tests + type checking |
| missing-feature | 14 | Not a testing problem — feature work |
| config-default | 13 | Rust core unit tests + per-adapter tests |
| type-safety | 9 | Static type checking (tsc, clippy) |
| code-duplication | 6 | Not a testing problem — refactoring |
| architecture | 5 | Rust core integration tests |

**Key insight:** Zero closed issues were caused by wire protocol incompatibility
between runtimes. All adapters share the same Rust core for framing,
serialisation, and QUIC transport. If adapter A serialises correctly and adapter
B serialises correctly, A→B works. The real pain point is the FFI boundary in
each individual adapter.

---

## 2. Current State

### What exists and works

| Layer | Files | Tests | In CI |
|---|---|---|---|
| Rust core integration | `integration.rs` (2039 lines) | ~30 scenarios | ✅ |
| Rust WebTransport | `bidi_stream.rs`, `session_webtransport.rs` | ~10 scenarios | ✅ |
| Rust unit | `sign_verify.rs`, `ticket.rs` | 6 tests | ✅ |
| Node integration | `e2e.mjs` | 7 tests | ✅ |
| Node smoke | `smoke.mjs` | 3 tests | ❌ |
| Deno integration + smoke | `smoke.test.ts` | 17 tests | ✅ |
| Cross-runtime compliance | `cases.json` (12 cases), `run.sh` | node↔deno | ❌ |
| TypeScript type check | `npm run typecheck` | — | ✅ |

### What is missing

1. **Node adapter has thin integration coverage** — 7 tests vs Deno's 17.
   Missing: crypto, cancellation, error classification, concurrent streams.
2. **Deno adapter has no dedicated integration tests** — `smoke.test.ts` mixes
   smoke checks and integration tests. No error path or limit tests.
3. **Rust core edge cases** — missing: mid-stream cancellation, pool exhaustion
   under contention, malformed input rejection, zero-config boundary values.
4. **Cross-runtime compliance not in CI** — `run.sh` exists but never runs
   automatically.
5. **No regression test policy** — closed issues leave no permanent test receipt.

---

## 3. Strategy

### Test at the right layer

```
┌────────────────────────────────────────────────────────┐
│  Static analysis (tsc, clippy)                        │ ← Cheapest. Catches
│  Catches: type mismatches, missing exports, API drift  │   type-safety + api-surface
├────────────────────────────────────────────────────────┤
│  Rust core tests (cargo test)                          │ ← Fast. Catches
│  Catches: config defaults, edge cases, concurrency     │   config-default + architecture
├────────────────────────────────────────────────────────┤
│  Per-adapter integration tests                         │ ← The workhorse. Catches
│  (same-process, two nodes, serve+fetch)                │   ffi-boundary + api-surface
│  Catches: type coercion, handle leaks, error mapping   │
├────────────────────────────────────────────────────────┤
│  Cross-runtime compliance (run.sh)                     │ ← Smoke check only.
│  Catches: wire protocol bugs (never seen one yet)      │   One pair is sufficient.
└────────────────────────────────────────────────────────┘
```

### Principles

1. **Test where the bugs are.** FFI boundaries produce the most bugs → invest
   most in per-adapter integration tests.
2. **Static analysis is free.** tsc and clippy already gate.
3. **Cross-runtime is a smoke check, not a matrix.** One pair (node↔deno) in CI
   validates wire compatibility. Add pairs only when a wire bug motivates it.
4. **Regression tests go in the right layer.** A Rust
   config edge case → `cargo test`. A protocol behavior → `cases.json`. Never
   force a regression into the wrong layer.
5. **Every fixed issue leaves a test.** But in the test suite that matches its
   root cause, not always in `cases.json`.

---

## 4. Phase 1 — Per-Adapter Integration Depth

**Goal:** Each adapter has integration tests covering error paths, limits,
cancellation, and crypto — not just happy-path fetch/serve.

This is the highest-leverage investment. The 19 FFI-boundary bugs would have
been caught by same-process adapter tests.

### 4.1 Node integration tests (`packages/iroh-http-node/test/e2e.mjs`)

Add tests for:
- **Error classification:** handler throws → client gets 500;
  handler rejects → client gets 500; verify error is `IrohError` subclass
- **Crypto round-trip:** `SecretKey.generate()`, `sign()`, `verify()` via
  the re-exported classes (validates A-ISS-050 fix)
- **Cancellation:** fetch with `AbortSignal.timeout(1)` against a slow handler
  → throws `AbortError`
- **Server limits:** `maxRequestBodyBytes` exceeded → 413 response
- **Concurrent streams:** 10 concurrent fetches, all return correct bodies
  (already exists — verify it covers the buffer race from DENO-001)
- **Node ID header:** `iroh-node-id` header is present, valid base32, and
  consistent across requests
- **Handle lifecycle:** double-close does not throw; close during active
  serve completes gracefully

Target: ≥ 15 tests (currently 7).

### 4.2 Deno integration tests (`packages/iroh-http-deno/test/smoke.test.ts`)

Add tests for:
- **Error classification:** same as Node — handler throws → 500
- **Cancellation:** `AbortSignal.timeout()` against slow handler
- **Server limits:** body too large → 413
- **Serve lifecycle:** `serveHandle.close()` during active serving
- **PublicKey/SecretKey imports:** verify re-exports work
  (`import { PublicKey } from "@momics/iroh-http-deno"`)

Target: ≥ 22 tests (currently 17).

### Verification

Each adapter's test suite passes locally. No cross-runtime infrastructure
needed.

---

## 5. Phase 2 — Rust Core Edge Cases

**Goal:** The Rust core rejects bad inputs and handles concurrency edge cases
that config-default and architecture bugs emerge from.

### 5.1 New test cases in `crates/iroh-http-core/tests/integration.rs`

- **Zero-value configs:** `max_chunk_size = 0`, `channel_capacity = 0` →
  either rejected at construction or handled gracefully (A-ISS-034, A-ISS-035)
- **Cancellation mid-stream:** client cancels fetch while body is streaming →
  server observes broken pipe, no panic
- **Pool exhaustion:** `max_pooled_connections = 1`, rapidly open 10 connections
  → oldest evicted cleanly, no deadlock
- **Timeout during body transfer:** `request_timeout = 100ms`, handler sleeps
  during body write → 408 response
- **Concurrent serve handlers:** 20 concurrent requests to same endpoint →
  all complete (stress test for `max_concurrency`)
- **Invalid handle after close:** use endpoint handle after `close()` →
  returns error, no panic

### Verification

`cargo test --workspace` passes. No new dependencies.

---

## 6. Phase 3 — Static Analysis in CI

**Goal:** Type mismatches and missing exports are caught before any test runs.

### 6.1 Add Node compliance to CI

The existing `compliance.mjs` (same-process, 12 cases) should run in CI alongside `e2e.mjs`:

```yaml
- name: Node compliance
  run: node packages/iroh-http-node/test/compliance.mjs
```

### 6.2 Gate cross-runtime (node↔deno only)

Add `run.sh` to CI with the existing two pairs only.

```yaml
- name: Cross-runtime compliance (node↔deno)
  run: bash tests/http-compliance/run.sh --pairs node-deno,deno-node
```

### Verification

CI pipeline: `rust-check` → `typescript-check` → `e2e` →
`cross-runtime` (node↔deno only).

---

## 7. Phase 4 — Regression Test Policy

**Goal:** Every fixed issue leaves a test in the right layer.

### 7.1 Update `issues/_template.md`

Add a `## Regression test` section:

```markdown
## Regression test

- Layer: rust-core | node | deno | cross-runtime | type-check | N/A
- Test: `test name or file path`
- Verified failing before fix: yes | N/A
```

### 7.2 Update `.github/copilot-instructions.md`

Add:

```markdown
## Issue Resolution Policy

Every fixed issue must leave a regression test in the appropriate layer:
- FFI boundary bugs → per-adapter integration test (e2e.mjs, smoke.test.ts)
- Rust core bugs → cargo test (integration.rs or new test file)
- Type/export bugs → verified by tsc (no new test needed if CI gates it)
- Protocol behavior → cases.json entry
- Docs/build/config → N/A (document in issue)
```

### 7.3 Backfill high-value regressions

Not every closed issue needs a retroactive test. Prioritise the 19
ffi-boundary issues — scan for any whose fix is not covered by existing tests.
Write regression tests only for those.

### Verification

Template is updated. Future issues follow the policy. 5–10 high-value
regression tests are backfilled.

---

## 8. What We Explicitly Do Not Do

1. **Full N×N cross-runtime matrix.** 4 runtimes × 4 = 12 pairs. Each pair
   spawns two processes, does QUIC handshake, runs cases. Three minutes per
   pair in CI. The added bug-finding value over node↔deno is near zero because
   all adapters share the same Rust wire layer.

2. **Rust ground-truth server binary.** With 2,039 lines of Rust integration
   tests already covering the core, a separate compliance binary is redundant.
   If adapter A is broken, same-process adapter tests catch it faster than
   `rust→A`.

3. **Compression and streaming in `cases.json`.** These require schema
   extensions, 4× client updates, and server handler changes. The Rust core
   already tests compression and streaming directly. Per-adapter tests can
   cover adapter-specific compression wiring without cross-runtime overhead.

4. **Tauri in the compliance matrix.** Tauri requires a webview runtime and
   cannot run as a headless CLI process. Test Tauri through its Rust plugin
   tests and manual QA.

---

## 9. Execution Order

```
Phase 1 — Per-adapter integration depth    ← Start here. Highest leverage.
  │
Phase 2 — Rust core edge cases             ← Can run in parallel with Phase 1.
  │
Phase 3 — Static analysis in CI            ← Requires Phase 1 tests to exist.
  │
Phase 4 — Regression test policy           ← Apply incrementally from day one.
```

Phases 1 and 2 are independent. Phase 3 depends on Phase 1 (tests must exist
to gate on). Phase 4 is a process change applied continuously.

---

## 10. Files to Modify

| File | Phase | Change |
|---|---|---|
| `packages/iroh-http-node/test/e2e.mjs` | 1 | Add ~8 integration tests |
| `packages/iroh-http-deno/test/smoke.test.ts` | 1 | Add ~5 integration tests |
| `crates/iroh-http-core/tests/integration.rs` | 2 | Add ~6 edge-case tests |
| `.github/workflows/ci.yml` | 3 | Add compliance, cross-runtime jobs |
| `docs/build-and-test.md` | 3 | Document new CI jobs |
| `issues/_template.md` | 4 | Add `## Regression test` section |
| `.github/copilot-instructions.md` | 4 | Add issue resolution policy |

No new files needed.

---

## 11. Success Criteria

1. Each adapter has ≥ 15 same-process integration tests covering error paths,
   limits, cancellation, and crypto.
2. Rust core has edge-case tests for zero-value configs, mid-stream
   cancellation, pool exhaustion, and concurrent limits.
3. CI gates on: `cargo test`, `cargo clippy`, `tsc`, Node e2e +
   compliance, Deno smoke, cross-runtime node↔deno.
4. `issues/_template.md` has a `## Regression test` section.
5. A new FFI boundary bug is caught by per-adapter tests before it reaches
   cross-runtime — because the bug is in one adapter's type conversion, not
   in the wire protocol.
