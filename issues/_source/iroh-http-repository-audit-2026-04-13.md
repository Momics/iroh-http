---
date: 2026-04-13
status: open
---

# iroh-http Repository Audit (Code-First)

Date: 2026-04-13
Reviewer scope: repository code reviewed against `docs/` claims (features, principles, architecture, protocol, roadmap).  
Primary focus: code behavior and contract correctness (not test flakiness triage).

## How to use this document

- Treat each `ISS-*` as an executable work item.
- Work in severity order: `P0` first, then `P1`, then `P2/P3`.
- Each issue includes: evidence, impact, concrete remediation, and acceptance criteria.

## Severity legend

- `P0` Critical correctness/safety issue with high production risk.
- `P1` High-impact behavior/contract mismatch.
- `P2` Medium-impact API/docs drift or implementation gap.
- `P3` Low-impact cleanup, consistency, or process gap.

---

## Executive summary

- Total issues: 26
- `P0`: 2
- `P1`: 8
- `P2`: 10
- `P3`: 6

---

## Detailed issues

### ISS-001 (`P0`) Server can panic/abort for small `maxHeaderBytes`

**Evidence**
- Server uses un-clamped `max_buf_size(max_header_size)`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/server.rs:573`
- Internal docs explicitly require clamp to `max(8192)` and warn panic otherwise:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/internals/http-engine.md:164`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/internals/design-decisions.md:69`
- Release profile aborts on panic:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/Cargo.toml:35`

**Impact**
- Runtime crash in production on misconfigured/small header limits.

**Remediation**
1. Change server builder to `.max_buf_size(max_header_size.max(8192))`.
2. Add post-parse header byte measurement and explicit reject path (see ISS-003).

**Acceptance criteria**
1. `maxHeaderBytes = 1` no longer crashes.
2. Oversize headers return deterministic error/response (no panic).

---

### ISS-002 (`P0`) Duplex path ignores handler response status and always returns 101

**Evidence**
- Server awaits JS response head, then always returns `101 Switching Protocols` in upgrade path:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/server.rs:292`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/server.rs:355`
- Shared handler returns actual `res.status` for bidi:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/serve.ts:227`

**Impact**
- Handler cannot reject/authorize duplex requests reliably.
- Potential auth bypass in apps expecting non-101 rejection.

**Remediation**
1. In server upgrade path, honor handler status/headers.
2. Only perform upgrade pump when status is 101 + valid `Upgrade` semantics.
3. Return non-101 responses as normal HTTP responses.

**Acceptance criteria**
1. Duplex handler returning `403` sends `403` and does not upgrade.
2. Existing valid duplex path still upgrades on 101.

---

### ISS-003 (`P1`) Server-side `maxHeaderBytes` contract is incomplete (no post-parse enforcement / no 431)

**Evidence**
- Server only sets hyper parser limits:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/server.rs:573`
- No server-side header byte counting/reject code found.
- Internal docs say enforcement must be: parser clamp + post-parse byte count:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/internals/http-engine.md:158`
- Feature docs promise 431 behavior:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/server-limits.md:52`

**Impact**
- Behavioral mismatch; inconsistent header-limit semantics.

**Remediation**
1. Compute measured request header bytes server-side after parse.
2. Return `431 Request Header Fields Too Large` when exceeded.
3. Surface a structured `HEADER_TOO_LARGE` path.

**Acceptance criteria**
1. Request headers above configured limit receive 431.
2. `ErrorCode::HeaderTooLarge` becomes reachable in code paths.

---

### ISS-004 (`P1`) `maxRequestBodyBytes` does not explicitly reject with 413

**Evidence**
- Body pump breaks loop on overflow without explicit 413 response:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/client.rs:336`
- Docs promise 413 before body read completion:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/server-limits.md:20`

**Impact**
- Callers do not get documented overflow status semantics.

**Remediation**
1. Convert body-limit overflow into explicit error path.
2. Map path to 413 (or update docs to match actual chosen behavior).

**Acceptance criteria**
1. Oversize body test receives deterministic 413 response.

---

### ISS-005 (`P1`) `maxConnectionsPerPeer` docs claim HTTP 429; implementation closes QUIC connection

**Evidence**
- Implementation closes connection when limit exceeded:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/server.rs:517`
- Docs claim 429 response behavior:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/server-limits.md:49`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/rate-limiting.md:19`

**Impact**
- Contract mismatch; clients may see transport errors, not HTTP 429.

**Remediation**
1. Decide intended semantics: transport-close or HTTP 429.
2. Align code and docs to one behavior.
3. Add explicit integration tests for chosen behavior.

**Acceptance criteria**
1. One canonical behavior documented and tested end-to-end.

---

### ISS-006 (`P1`) `maxConcurrency` docs claim 503, code currently queues/waits

**Evidence**
- Concurrency slot acquisition blocks via semaphore:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/server.rs:544`
- Docs claim overload returns 503:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/server-limits.md:48`

**Impact**
- Latency/behavior differs from advertised API contract.

**Remediation**
1. Either implement fast-fail 503 when no permit available or update docs to queue semantics.
2. Document timeout interactions for queued requests.

**Acceptance criteria**
1. Over-capacity behavior is deterministic and documented correctly.

---

### ISS-007 (`P1`) Request timeout docs claim 408; code returns generic timeout error

**Evidence**
- Timeout service returns generic boxed error string on timeout:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/server.rs:638`
- Docs claim 408:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/server-limits.md:50`

**Impact**
- Status mapping mismatch across runtimes/adapters.

**Remediation**
1. Map timeout to explicit HTTP 408 response path.
2. Ensure adapters preserve code/status semantics.

**Acceptance criteria**
1. Timeout scenario returns 408 consistently.

---

### ISS-008 (`P1`) Error taxonomy still relies heavily on string matching

**Evidence**
- Core classification based on message substrings:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/lib.rs:140`
- JS fallback regex classifier still used:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/errors.ts:262`
- Principle requires finite code-based taxonomy across FFI:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/principles.md:187`

**Impact**
- Fragile error mapping; message wording changes can break callers.

**Remediation**
1. Promote `core_error_to_json` structured code path as default everywhere.
2. Minimize/phase out free-form `classify_error_json` and regex fallback.
3. Add explicit error-code conformance tests.

**Acceptance criteria**
1. Main error pathways are code-first, not string-pattern based.

---

### ISS-009 (`P1`) Error enum has drifted/unused variants (`HeaderTooLarge`, `PeerRejected`)

**Evidence**
- Variants declared:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/lib.rs:46`
- `HeaderTooLarge`/`PeerRejected` not referenced in active logic beyond mapping.

**Impact**
- Taxonomy ambiguity; docs and behavior can diverge.

**Remediation**
1. Either implement these variants in real paths or remove/deprecate.
2. Keep enum and status mapping aligned with actual behavior.

**Acceptance criteria**
1. Every public error code has at least one concrete origin path.

---

### ISS-010 (`P2`) Server-limit docs still describe `serve()` options; implementation configures limits at `createNode()`

**Evidence**
- Docs describe `node.serve({ maxConcurrency, ... }, handler)`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/server-limits.md:10`
- `ServeOptions` type only contains lifecycle/error hooks:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/serve.ts:40`
- `makeServe` passes empty options to raw layer:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/serve.ts:159`
- Limits are provided at endpoint creation in adapters:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-node/lib.ts:374`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-deno/src/adapter.ts:447`

**Impact**
- Developer confusion and incorrect API usage.

**Remediation**
1. Update docs to `createNode({ ...limits })` model, or implement serve-time override.
2. If override added, define precedence clearly.

**Acceptance criteria**
1. Public docs and signatures match one consistent configuration model.

---

### ISS-011 (`P2`) Streaming docs claim FormData support, code rejects FormData

**Evidence**
- Docs claim `FormData` supported as BodyInit:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/streaming.md:28`
- Implementation throws on `FormData`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/streams.ts:104`

**Impact**
- Direct user-facing API mismatch.

**Remediation**
1. Either implement FormData serialization path, or correct docs to unsupported.

**Acceptance criteria**
1. FormData behavior is explicit and consistent in docs/tests/types.

---

### ISS-012 (`P2`) Observability docs describe APIs/fields not implemented

**Evidence**
- Docs advertise `stats(): Promise<NodeStats>` and richer `PeerStats` (rtt/bytes/path selected):
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/observability.md:11`
- Actual shared types expose a smaller `PeerStats` shape and no `stats()`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/bridge.ts:619`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/index.ts:282`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/endpoint.rs:455`

**Impact**
- Consumers cannot rely on documented telemetry API.

**Remediation**
1. Either implement missing observability API or narrow docs to current shape.

**Acceptance criteria**
1. Type definitions and docs for observability are fully aligned.

---

### ISS-013 (`P2`) Default-headers doc references `NodeOptions.injectHeaders` that does not exist

**Evidence**
- Docs mention optional injected `iroh-relay`/`iroh-rtt-ms` via `NodeOptions.injectHeaders`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/default-headers.md:34`
- No `injectHeaders` option in `NodeOptions`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/bridge.ts:154`

**Impact**
- Feature appears available but is not implementable.

**Remediation**
1. Implement `injectHeaders` end-to-end, or remove from docs.

**Acceptance criteria**
1. Option exists with tested behavior, or docs no longer advertise it.

---

### ISS-014 (`P2`) Ticket docs claim standard Iroh ticket format; implementation uses JSON string tickets

**Evidence**
- Docs claim Iroh standard base32/bech32 tickets and broader accepted peer formats:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/tickets.md:30`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/tickets.md:25`
- Implementation serializes `NodeAddrInfo` as JSON for `node_ticket`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/lib.rs:244`
- Shared `ticketNodeId` parses JSON first:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/index.ts:58`
- Fetch accepts `PublicKey | string`, not arbitrary NodeAddr object:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/bridge.ts:457`

**Impact**
- Interop/documentation expectations are incorrect.

**Remediation**
1. Pick one ticket format contract (JSON or Iroh-native) and enforce.
2. Update docs and helper APIs accordingly.

**Acceptance criteria**
1. Ticket format is unambiguous and documented with exact encoding.

---

### ISS-015 (`P2`) Sign/verify docs claim synchronous API, implementation is async

**Evidence**
- Docs: “Both sign and verify are synchronous”:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/sign-verify.md:20`
- Code uses async WebCrypto:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/keys.ts:98`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/keys.ts:250`

**Impact**
- Callers following docs will misuse API.

**Remediation**
1. Update docs/examples to `await` semantics.

**Acceptance criteria**
1. All sign/verify docs and examples compile and run as written.

---

### ISS-016 (`P2`) Protocol doc references wrong duplex client API shape

**Evidence**
- Protocol doc says `node.createBidirectionalStream()`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/protocol.md:80`
- Actual API: `node.connect(peer)` returns session, then `session.createBidirectionalStream()`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/bridge.ts:493`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-shared/src/session.ts:160`

**Impact**
- Incorrect usage guidance.

**Remediation**
1. Correct protocol docs to session-based API.

**Acceptance criteria**
1. Protocol examples mirror current public APIs.

---

### ISS-017 (`P2`) Trailer docs contain stale protocol internals

**Evidence**
- Trailer doc references retired ALPN variants and old custom framing functions:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/trailer-headers.md:53`
- Current wire docs define v2/hyper-native trailer handling and ALPN set:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/internals/wire-format.md:40`

**Impact**
- Misleads maintainers and users about current protocol internals.

**Remediation**
1. Rewrite trailer feature doc using current hyper-native behavior only.

**Acceptance criteria**
1. No references remain to retired trailer/full ALPN variants in feature docs.

---

### ISS-018 (`P2`) Multiple internal comments/docs still reference retired QPACK/v1 details

**Evidence**
- Endpoint comments mention old ALPN capability set and QPACK language:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/endpoint.rs:40`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/endpoint.rs:73`
- Current protocol constants are v2:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/lib.rs:182`

**Impact**
- Maintainer confusion; design intent drift.

**Remediation**
1. Update stale inline comments to current v2/hyper model.

**Acceptance criteria**
1. Inline docs match current transport/wire behavior.

---

### ISS-019 (`P2`) Compression `level` option is accepted in adapters/docs but ignored in core

**Evidence**
- Docs advertise configurable `compression.level`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/compression.md:15`
- JS adapters pass `compressionLevel`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-node/lib.ts:366`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-deno/src/adapter.ts:437`
- FFI structs carry `compression_level`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-node/src/lib.rs:93`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-deno/src/dispatch.rs:150`
- Core `CompressionOptions` only includes `min_body_bytes`; no level field:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/endpoint.rs:101`

**Impact**
- Silent no-op config surface; false expectation of tunable compression level.

**Remediation**
1. Either implement zstd level wiring end-to-end or remove level from public options/docs.

**Acceptance criteria**
1. `compression.level` either works and is tested, or is not exposed.

---

### ISS-020 (`P2`) `NodeOptions.maxHeaderBytes` docs promise `0/null` disables; implementation does not consistently support that semantics

**Evidence**
- Docs state `0` or `null` disables limits:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/server-limits.md:31`
- Current max-header path feeds raw values into parser configuration:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/server.rs:573`

**Impact**
- Potentially undefined behavior for `0` and small values.

**Remediation**
1. Define explicit semantics for `maxHeaderBytes = 0`.
2. Implement logic accordingly in both parse and post-parse enforcement.

**Acceptance criteria**
1. `0` behavior is deterministic, documented, and tested.

---

### ISS-021 (`P3`) Clippy hard gate currently fails

**Evidence**
- `cargo clippy --workspace -- -D warnings` fails with redundant closures:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/client.rs:130`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/client.rs:461`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/lib.rs:234`
- Principles require lint-clean hard gate:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/principles.md:286`

**Impact**
- CI/lint gate mismatch with documented quality standard.

**Remediation**
1. Replace redundant closures with function pointers in those sites.

**Acceptance criteria**
1. `cargo clippy --workspace -- -D warnings` passes.

---

### ISS-022 (`P3`) Principle says no sleep-based test timing; test suite still uses sleeps/timeouts

**Evidence**
- Principle: no sleep-based timing:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/principles.md:255`
- Current tests use sleeps/timeouts:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/tests/integration.rs:625`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/tests/bidi_stream.rs:159`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-node/test/e2e.mjs:157`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-deno/test/smoke.test.ts:293`

**Impact**
- Flake risk and principle non-compliance.

**Remediation**
1. Replace sleep-based synchronization with notify/oneshot/event-based coordination.

**Acceptance criteria**
1. No sleep-based synchronization remains in critical tests.

---

### ISS-023 (`P3`) `iroh-http-core` missing crates.io metadata fields called out in roadmap

**Evidence**
- Roadmap requirement:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/roadmap.md:21`
- Current `Cargo.toml` lacks `repository`, `documentation`, `keywords`, `categories`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/Cargo.toml:1`

**Impact**
- Publishing readiness gap.

**Remediation**
1. Add required metadata fields.

**Acceptance criteria**
1. Crate metadata matches roadmap checklist.

---

### ISS-024 (`P3`) `iroh-http-discovery` missing crates.io metadata fields called out in roadmap

**Evidence**
- Roadmap requirement:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/roadmap.md:21`
- Current `Cargo.toml` lacks same publish metadata fields:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-discovery/Cargo.toml:1`

**Impact**
- Publishing readiness gap.

**Remediation**
1. Add required metadata fields.

**Acceptance criteria**
1. Crate metadata matches roadmap checklist.

---

### ISS-025 (`P3`) Python `pyproject.toml` missing `[project.urls]` roadmap blocker

**Evidence**
- Roadmap requirement:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/roadmap.md:32`
- Current `pyproject.toml` has no `[project.urls]` section:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-py/pyproject.toml:5`

**Impact**
- Packaging metadata incompleteness.

**Remediation**
1. Add `[project.urls]` with repository link.

**Acceptance criteria**
1. PyPI metadata includes repository URL.

---

### ISS-026 (`P3`) Release automation workflow missing (roadmap blocker)

**Evidence**
- Roadmap calls out missing `release.yml`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/roadmap.md:39`
- Workflow dir currently contains only CI:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/.github/workflows/ci.yml:1`

**Impact**
- Manual release risk and inconsistent artifact publishing.

**Remediation**
1. Add tag-triggered release workflow for npm/JSR/PyPI/crates.io as planned.

**Acceptance criteria**
1. `v*` tag pipeline builds and publishes release artifacts.

---

## Additional documented drift items (lower priority but should be cleaned)

### DRIFT-A (`P3`) `sign-verify.md` references non-existent relative docs paths

**Evidence**
- Broken links under `See also`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/sign-verify.md:29`

**Remediation**
1. Fix links or remove stale references.

### DRIFT-B (`P3`) `docs/architecture.md` security defaults table uses `request_timeout_secs` naming

**Evidence**
- Table uses `ServeOptions::request_timeout_secs`:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/architecture.md:168`
- Code uses millisecond field names (`request_timeout_ms` / `requestTimeout`):
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/crates/iroh-http-core/src/server.rs:432`
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/packages/iroh-http-node/src/lib.rs:100`

**Remediation**
1. Normalize naming/units in architecture docs to match runtime API.

### DRIFT-C (`P3`) Server-limits doc includes stale “Status” note no longer accurate

**Evidence**
- Doc says TS `serve()` does not pass limits through:
  - `/Users/phnl320048348/Documents/local-repos/iroh-http/docs/features/server-limits.md:56`
- In current code, limits are passed in `createNode(...)` into Rust.

**Remediation**
1. Replace note with accurate description of current configuration path.

---

## Suggested execution order for engineering work

1. Fix `ISS-001` and `ISS-002` immediately.
2. Implement/decide server-limit semantics (`ISS-003`..`ISS-007`) and add integration tests for each limit.
3. Unify error-code architecture (`ISS-008`, `ISS-009`).
4. Resolve high-value API/docs mismatches (`ISS-010`..`ISS-020`).
5. Clear quality/publish debt (`ISS-021`..`ISS-026` + drift items).

---

## Verification checklist after fixes

1. `cargo check --workspace`
2. `cargo clippy --workspace -- -D warnings`
3. `cargo test --workspace`
4. `npm run typecheck`
5. Docs consistency pass: `docs/features/*`, `docs/protocol.md`, `docs/architecture.md`, `docs/principles.md` updated to match runtime behavior.

