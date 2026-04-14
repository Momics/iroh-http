# TEST_PLAN.md — iroh-http Cross-Runtime Test Strategy

This document is the authoritative plan for building a robust, automated test
suite across all iroh-http platform adapters (Rust core, Node.js, Deno, Python,
Tauri). It explains what already exists, what is missing, and defines the exact
work required to reach a state where recurring issues cannot regress silently.

---

## 1. Current State

### What exists

| Layer | Tool | Coverage |
|---|---|---|
| Rust core (unit) | `cargo test --workspace` | Happy-path fetch/serve, trailers, streams, sign/verify, WebTransport |
| Cross-runtime compliance runner | `tests/http-compliance/runner.ts` | Shared TS runner; used by Node + Deno same-process tests |
| Cross-runtime compliance cases | `tests/http-compliance/cases.json` | 12 cases — all happy-path only |
| Cross-runtime orchestrator | `tests/http-compliance/run.sh` | node→deno and deno→node pairs only |
| Node in-process compliance | `packages/iroh-http-node/test/compliance.mjs` | Runs cases.json against Node adapter (same process) |
| Deno in-process compliance | `packages/iroh-http-deno/test/compliance.ts` | Runs cases.json against Deno adapter (same process) |
| Node E2E | `packages/iroh-http-node/test/e2e.mjs` | ~10 round-trip tests, in CI |
| Deno smoke | `packages/iroh-http-deno/test/smoke.test.ts` | Minimal smoke, in CI |
| Python unit | `packages/iroh-http-py/tests/` | Node-level tests; no compliance bridge |
| CI | `.github/workflows/ci.yml` | Rust + TS check, Node E2E, Deno smoke — **no cross-runtime pairs, no Python** |

### What is missing

1. **Python compliance scripts** — no `server.py` / `client.py` counterparts to the Node/Deno ones.
2. **Rust ground-truth server** — no standalone binary that implements compliance routes in native Rust so adapters can be tested against the canonical implementation.
3. **Cross-runtime pairs involving Python** — `run.sh` skips Python entirely.
4. **Tauri excluded from compliance matrix** — no server/client scripts for Tauri headless mode.
5. **cases.json covers only happy paths** — no error propagation, boundary values, invalid inputs, or regression cases from the 80+ closed issues.
6. **`run.sh` is not in CI** — the cross-runtime orchestrator never runs automatically.
7. **No issue→test policy** — closed issues leave no permanent test receipt.

---

## 2. Goal

Every issue resolved in this repository must leave a permanent automated test.
Every adapter must be tested not just in isolation but against every other
adapter over a real QUIC connection. A review agent finding a "new" issue that
was already closed should be impossible because CI would have caught a regression.

---

## 3. Phase 1 — Python Compliance Bridge

**Goal:** Python participates in the cross-runtime matrix.

### 3.1 `tests/http-compliance/server.py`

A standalone script that:
- Calls `asyncio.run(main())`
- Creates an iroh node via `await create_node()`
- Calls `node.serve(handler)` with the compliance echo handler
- Implements all compliance routes: `/status/:code`, `/echo`, `/echo-path`,
  `/echo-method`, `/echo-length`, `/header/:name`, `/set-header/:name/:val`, `/stream/:n`
- Prints `READY:{"nodeId":"...","addrs":["..."]}` to stdout (one line, no buffering —
  use `flush=True`)
- Blocks until SIGTERM / SIGINT

Handler routing mirrors `server.mjs` exactly. Use `urllib.parse.urlparse` for
path parsing.

### 3.2 `tests/http-compliance/client.py`

A standalone script that:
- Accepts `SERVER_JSON` as `sys.argv[1]` (same pattern as `client.mjs`)
- Parses `nodeId` and `addrs` from the JSON
- Loads `cases.json` from a path relative to its own `__file__`
- Creates an iroh node, calls `client.fetch(nodeId, url, direct_addrs=addrs)` for each case
- Asserts using the same logic as `client.mjs`: status, bodyExact, bodyNot,
  bodyNotEmpty, bodyLengthExact, headers
- Prints `pass <id>` or `FAIL <id>: <reason>` per case to stdout
- Exits non-zero if any case fails

### 3.3 `run.sh` additions

Add to the pair definitions section (after the existing `deno→node` pair):

```
# Python server ↔ Node client
python → node

# Node server ↔ Python client
node → python

# Python server ↔ Deno client
python → deno

# Deno server ↔ Python client
deno → python
```

Each pair guarded by `command -v python3 && command -v node/deno` availability
check, consistent with the existing pattern. Use `python3` as the interpreter
name throughout.

### Verification

- `bash tests/http-compliance/run.sh` with Python available passes all 4 new pairs
- `bash tests/http-compliance/run.sh --pairs python-node` runs a single pair

---

## 4. Phase 2 — Rust Ground-Truth Compliance Server

**Goal:** Adapters can be tested against the canonical Rust implementation,
isolating bugs to specific adapter boundaries with certainty.

### 4.1 New binary target

Add `[[bin]]` to `crates/iroh-http-core/Cargo.toml`:

```toml
[[bin]]
name = "compliance-server"
path = "src/bin/compliance_server.rs"
```

### 4.2 `crates/iroh-http-core/src/bin/compliance_server.rs`

A `#[tokio::main]` binary that:
- Binds an `IrohEndpoint` with `bind_addrs: vec!["0.0.0.0:0".into()]`
- Calls `serve()` with a handler implementing all compliance routes in native
  Rust mirroring the handler logic in `server.mjs`
- Prints `READY:{"nodeId":"...","addrs":["..."]}` to stdout, then flushes stdout
  (`use std::io::Write; std::io::stdout().flush().unwrap();`)
- Blocks on `tokio::signal::ctrl_c().await`

All compliance routes use `iroh_http_core` primitives: `respond()`,
`stream::send_chunk()`, `stream::finish_body()`.

### 4.3 `run.sh` additions

After Phase 1 pairs, add:

```
# Rust server ↔ Node client
rust → node

# Rust server ↔ Deno client
rust → deno

# Rust server ↔ Python client
rust → python
```

The Rust server binary is built by the CI `cargo build` step already; `run.sh`
references `target/release/compliance-server`.

### 4.4 Optional: Rust compliance client binary

A second binary, `compliance-client`, that:
- Accepts `SERVER_JSON` as first CLI argument
- Accepts `CASES_PATH` as second CLI argument
- Runs all cases using `fetch()` from `iroh_http_core`
- Exits non-zero on any failure

This enables `node→rust`, `deno→rust`, `python→rust` pairs, completing the
full N×N matrix.

### Verification

- `cargo build -p iroh-http-core` produces `compliance-server` binary
- `bash tests/http-compliance/run.sh --pairs rust-node` passes all cases
- A deliberate bug in the Node adapter causes `rust→node` to fail

---

## 5. Phase 3 — Expand cases.json

**Goal:** `cases.json` is a living regression database, not just a hello-world
suite.

### 5.1 New case categories

All categories below map directly to known issue clusters from the archive.

**Error propagation** (maps to DENO-006, NODE-series)
- `handler-throws-returns-500` — handler throws synchronously; client receives 500
- `handler-async-rejects-returns-500` — handler async rejects; client receives 500

**Boundary values**
- `empty-request-body` — POST 0-byte body; `/echo-length` returns `"0"`
- `single-byte-request-body` — POST 1-byte body; `/echo-length` returns `"1"`
- `status-code-100` — GET `/status/100`; client sees 100
- `status-code-599` — GET `/status/599`; client sees 599

**Numeric / type coercion at FFI boundary** (maps to NODE-007, PY-series)
- `body-length-zero-content-length` — response with `content-length: 0`; client reads empty body cleanly
- `large-header-value` — request header value of 4096 bytes; echoed back exactly

**Identity and security** (maps to A-ISS-series)
- `iroh-node-id-is-valid-base32` — `iroh-node-id` value passes base32 validation and is ≥ 52 chars
- `iroh-node-id-consistent` — two consecutive requests to same server return same `iroh-node-id`

**Compression** (maps to TAURI-015, compression feature)
- `zstd-response-requested` — GET with `accept-encoding: zstd`; server sends zstd-compressed
  response; client decompresses correctly
- `gzip-request-accepted` — POST with `content-encoding: gzip` compressed body; server
  decompresses, `/echo-length` returns uncompressed byte count

**Streaming**
- `chunked-response-1mb` — GET `/stream/1048576`; client receives exactly 1 MiB
- `chunked-request-1mb` — POST 1 MiB body; `/echo-length` returns `"1048576"`

**Regression cases from closed issues** (one entry per resolved issue)
Name convention: `reg-<issue-id>-<slug>`. Examples:
- `reg-node-007-nan-body-size` — NaN passed as body size option; server rejects, no panic
- `reg-deno-006-serve-error-propagates` — handler throws; outer promise rejects cleanly
- `reg-py-014-path-changes-stubs` — path_changes type is accessible; no AttributeError

### 5.2 Case schema extensions

The current schema supports: `status`, `bodyExact`, `bodyNot`, `bodyNotEmpty`,
`bodyLengthExact`, `headers`. Add these backwards-compatible optional fields:

| Field | Type | Meaning |
|---|---|---|
| `expectThrow` | `boolean` | Fetch is expected to throw/reject |
| `requestCompression` | `"gzip" \| "zstd"` | Runner compresses request body before sending |
| `responseCompression` | `"gzip" \| "zstd"` | Runner verifies response is decompressed |
| `minBodyLength` | `number` | Response body must be at least N bytes |
| `concurrent` | `number` | Run N times concurrently; all must pass |

The runner (`runner.ts`, `client.mjs`, `client.deno.ts`, `client.py`) must be
updated to handle the new fields.

### 5.3 Policy: every closed issue gets a case

When an issue is closed as `fixed`, the issue file must reference the new case
ID. Add to `issues/_template.md`:

```markdown
## Regression test

- `cases.json` case ID: `reg-<issue-id>-<slug>`
- Verified failing before fix: yes / N/A
```

If no runtime test is applicable (docs, build config, CI), write `N/A — not a
runtime behavior fix`.

### Verification

- `node test/compliance.mjs` passes all new cases for Node
- `deno run --allow-read --allow-ffi test/compliance.ts` passes all new cases for Deno
- Python compliance client passes all new cases against Python server
- Deliberately revert a closed-issue fix; confirm the `reg-*` case catches it

---

## 6. Phase 4 — CI Integration

**Goal:** The full cross-runtime matrix runs on every push and pull request.

### 6.1 New CI job: `cross-runtime-compliance`

Add to `.github/workflows/ci.yml` after the existing `e2e` job:

```yaml
cross-runtime-compliance:
  name: Cross-runtime compliance (Node × Deno × Python × Rust)
  runs-on: ubuntu-latest
  needs: [e2e]
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
    - uses: actions/setup-node@v4
      with:
        node-version: 20
    - uses: denoland/setup-deno@v2
      with:
        deno-version: v2.x
    - uses: actions/setup-python@v5
      with:
        python-version: "3.11"
    - name: Install Python deps
      run: |
        pip install maturin
        cd packages/iroh-http-py && maturin develop
    - name: Build native libs
      run: |
        cargo build --release -p iroh-http-node -p iroh-http-deno
        cargo build --release -p iroh-http-core
    - name: Build Node adapter
      run: |
        cd packages/iroh-http-node
        npx napi build --platform --release
        npx tsc
    - name: Copy Deno native lib
      run: |
        mkdir -p packages/iroh-http-deno/lib
        cp target/release/libiroh_http_deno.so \
           packages/iroh-http-deno/lib/libiroh_http_deno.linux-x86_64.so
    - name: Run cross-runtime compliance
      run: bash tests/http-compliance/run.sh
```

### 6.2 Job dependency

`needs: [e2e]` ensures the new job only runs if individual adapter tests already
pass, preserving fast feedback for simple failures without blocking on the longer
cross-runtime suite.

### 6.3 Reporting

`run.sh` already prints a summary with pass/fail counts. If any pair fails,
`run.sh` exits non-zero and CI marks the job failed. The failing pair name is
visible in the CI log output.

### 6.4 `docs/build-and-test.md` update

Add a section documenting the cross-runtime compliance command:

```sh
bash tests/http-compliance/run.sh              # all pairs
bash tests/http-compliance/run.sh --pairs rust-node  # single pair
```

### Verification

- Open a PR that breaks Python↔Node interop; CI fails on `cross-runtime-compliance`
- Open a PR that only breaks Rust compilation; CI fails on `rust-check` before
  reaching the compliance job
- Green CI on main with all pairs passing

---

## 7. Phase 5 — Issue Resolution Policy

**Goal:** Future issues cannot be silently re-introduced.

### 7.1 `issues/_template.md` update

Add mandatory `## Regression test` section (see §5.3).

### 7.2 `.github/copilot-instructions.md` update

Add a short section:

```markdown
## Issue Resolution Policy

Before closing any issue as `fixed`:
1. Write a new entry in `tests/http-compliance/cases.json` that reproduces the
   bug (name it `reg-<issue-id>-<slug>`).
2. Verify the case fails against the unfixed code (or document why not possible
   for this issue type).
3. Apply the fix.
4. Verify the case passes.
5. Record the case ID in the issue file under `## Regression test`.

No issue may be marked `fixed` without completing step 1 unless it cannot
produce a runtime test (e.g., CI config, docs, build scripts).
```

### Verification

- Pick a recently closed issue (e.g., DENO-006); add `reg-deno-006-*` to
  `cases.json`; run compliance against a pre-fix snapshot and confirm it fails.

---

## 8. Execution Order & Dependencies

```
Phase 1 — Python compliance bridge
  │  Independent. Start immediately.
  │
Phase 2 — Rust ground-truth server
  │  Independent of Phase 1. Can run in parallel.
  │
Phase 3 — Expand cases.json (base cases)
  │  Independent of Phase 1+2. Can run in parallel.
  │  Compression cases require Phase 3 runner schema extension before
  │  Phase 1 Python client can assert them.
  │
Phase 4 — CI integration
  │  Requires Phase 1 + 2 complete (all server/client scripts must exist).
  │  Phase 3 can continue after Phase 4 ships; new cases auto-run in CI.
  │
Phase 5 — Policy
     Requires Phase 3 partially done (to establish the reg-* naming pattern).
     Apply incrementally as future issues are resolved.
```

Phases 1, 2, and 3 are all independently executable and can be worked in
parallel by separate agents.

---

## 9. File Inventory

### Files to create

| File | Phase | Description |
|---|---|---|
| `tests/http-compliance/server.py` | 1 | Python compliance server |
| `tests/http-compliance/client.py` | 1 | Python compliance client |
| `crates/iroh-http-core/src/bin/compliance_server.rs` | 2 | Rust ground-truth server |
| `crates/iroh-http-core/src/bin/compliance_client.rs` | 2 | Rust compliance client (optional) |

### Files to modify

| File | Phase | Change |
|---|---|---|
| `tests/http-compliance/run.sh` | 1, 2 | Add Python and Rust pairs |
| `tests/http-compliance/cases.json` | 3 | Add ~20 new cases across all categories |
| `tests/http-compliance/runner.ts` | 3 | Handle new schema fields |
| `tests/http-compliance/client.mjs` | 3 | Handle new schema fields |
| `tests/http-compliance/client.deno.ts` | 3 | Handle new schema fields |
| `.github/workflows/ci.yml` | 4 | Add `cross-runtime-compliance` job |
| `crates/iroh-http-core/Cargo.toml` | 2 | Add `[[bin]]` for compliance-server |
| `issues/_template.md` | 5 | Add `## Regression test` section |
| `.github/copilot-instructions.md` | 5 | Add issue resolution policy |
| `docs/build-and-test.md` | 4 | Document new compliance run command |

---

## 10. Success Criteria

The plan is complete when all of the following are true:

1. `bash tests/http-compliance/run.sh` passes all pairs:
   - `node→deno`, `deno→node` (already working)
   - `python→node`, `node→python`, `python→deno`, `deno→python`
   - `rust→node`, `rust→deno`, `rust→python`
   - (optional) `node→rust`, `deno→rust`, `python→rust`

2. `cases.json` contains at least 35 cases, including at least one `reg-*`
   regression case for each of the 10 most recent closed issues.

3. CI has a `cross-runtime-compliance` job that passes on the `main` branch.

4. `issues/_template.md` includes the `## Regression test` field.

5. `.github/copilot-instructions.md` includes the issue resolution policy.

6. A new agent review cannot find an issue that already has a `reg-*` case in
   `cases.json` — because the compliance suite would catch any regression before
   the review runs.
