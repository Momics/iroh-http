# R9 Systemic Review Tracker

Last updated: 2026-04-12 (delegated first pass complete)

## Stream Status

| Stream | Owner | Status | Started | Target End | Notes |
|---|---|---|---|---|---|
| S1 Rust core | delegated agent | done | 2026-04-12 | 2026-04-12 | See `evidence/S1_rust_core_2026-04-12.md` |
| S2 Shared bridge contract | delegated agent | done | 2026-04-12 | 2026-04-12 | See `evidence/S2_shared_bridge_2026-04-12.md` |
| S3 Node adapter | delegated agent | done | 2026-04-12 | 2026-04-12 | See `evidence/S3_node_adapter_2026-04-12.md` |
| S4 Deno/Tauri adapters | delegated agent | done | 2026-04-12 | 2026-04-12 | See `evidence/S4_deno_tauri_2026-04-12.md` |
| S5 Python adapter | delegated agent | done | 2026-04-12 | 2026-04-12 | See `evidence/S5_python_2026-04-12.md` |
| S6 Trusted package matrix | delegated agent | done | 2026-04-12 | 2026-04-12 | See `evidence/S6_package_matrix_2026-04-12.md` |

Status values: `not_started`, `in_progress`, `blocked`, `done`

## Findings Rollup

| ID | Stream | Severity | Title | Guideline Ref | Status | Owner |
|---|---|---|---|---|---|---|
| R9-S1-01 | S1 | P1 | Malformed chunk headers treated as incomplete | guidelines.md §4 | open | TBD |
| R9-S1-02 | S1 | P1 | Trailer parsing lacks size limits | guidelines.md §4 | open | TBD |
| R9-S1-03 | S1 | P1 | Per-peer fairness not enforced per in-flight stream | guidelines.md §4 | open | TBD |
| R9-S2-01 | S2 | P1 | Trailer completion invariant can break on send failure | guidelines-javascript.md (bridge/serve) | open | TBD |
| R9-S2-02 | S2 | P1 | `acceptWebTransport()` can be called multiple times | guidelines-javascript.md (streaming) | open | TBD |
| R9-S2-06 | S2 | P1 | Shared error class/name mapping diverges from guideline contract | guidelines-javascript.md (errors) | open | TBD |
| R9-S3-01 | S3 | P1 | `disableNetworking` option ignored in Node mapping | shared NodeOptions contract | open | TBD |
| R9-S3-02 | S3 | P1 | `reconnect/lifecycle` options accepted by types but dropped | shared NodeOptions contract | open | TBD |
| R9-S4-01 | S4 | P1 | Deno `nextChunk` uses shared buffer across concurrent FFI calls | guidelines-javascript.md (streaming) | open | TBD |
| R9-S4-02 | S4 | P1 | Deno read errors collapsed into EOF | guidelines-javascript.md (errors) | open | TBD |
| R9-S4-04 | S4 | P1 | Tauri bridge surfaces raw invoke errors | guidelines-javascript.md (errors) | open | TBD |
| R9-S4-05 | S4 | P1 | Tauri serve channel failure can leave request unresolved | shared serve contract | open | TBD |
| R9-S5-01 | S5 | P1 | Unsafe raw-pointer lifetime in Python async mDNS iterator | guidelines.md §4 | open | TBD |
| R9-S5-02 | S5 | P1 | Python public API export inconsistency | guidelines-python.md (packaging) | open | TBD |
| R9-S5-03 | S5 | P1 | `IrohRequest.text()` missing vs documented contract | guidelines-python.md (serve contract) | open | TBD |
| R9-S6-01 | S6 | P2 | Adopt typed Rust error codes (`thiserror` + enum) | build-vs-buy matrix | open | TBD |
| R9-S6-02 | S6 | P2 | Adopt trusted JS Ed25519 crate (`@noble/ed25519`) | build-vs-buy matrix | open | TBD |

Status values: `open`, `in_fix`, `accepted_risk`, `resolved`

## Cross-Stream Questions

Use this section for contradictions or assumptions that need arbitration by the
review lead.

1. Question:
   - Stream: S2/S3/S4/S5
   - Proposed resolution: Canonicalize cross-platform error contract in one source (`core` error enum + adapter mapping tests), then update docs.
   - Decision: TBD
2. Question:
   - Stream: S1/S2/S5
   - Proposed resolution: Decide whether trailer support is mandatory parity across all adapters or explicitly optional capability.
   - Decision: TBD
3. Question:
   - Stream: S3/S4
   - Proposed resolution: Decide whether shared `NodeOptions` can include platform-specific no-op fields or must be fully honored per adapter.
   - Decision: TBD

## Final Backlog (Prioritized)

| Priority | Item | Source Finding(s) | Owner | Target |
|---|---|---|---|---|
| 1 | Fix unsafe Python raw-pointer async path | R9-S5-01 | TBD | 1-2 days |
| 2 | Fix Deno shared chunk buffer concurrency bug | R9-S4-01 | TBD | 1-2 days |
| 3 | Fix Deno EOF-on-error behavior for chunk reads | R9-S4-02 | TBD | 1-2 days |
| 4 | Enforce single-use `acceptWebTransport()` | R9-S2-02 | TBD | 1 day |
| 5 | Guarantee trailer completion path in shared serve pipeline | R9-S2-01 | TBD | 1-2 days |
| 6 | Add trailer size limits and invalid-chunk handling in core | R9-S1-01, R9-S1-02 | TBD | 2-3 days |
| 7 | Normalize NodeOptions behavior or hide unsupported fields | R9-S3-01, R9-S3-02, R9-S4-06, R9-S4-09 | TBD | 2 days |
| 8 | Unify error-code contract (core enum + adapter mapping tests) | R9-S1-06, R9-S2-06, R9-S4-04 | TBD | 2-3 days |
| 9 | Repair Python contract drift (`HandlerResponse`, `request.text`, stubs) | R9-S5-02, R9-S5-03, R9-S5-05 | TBD | 1-2 days |
| 10 | Implement or explicitly scope trailer/cancel parity for Python | R9-S5-06, R9-S5-07 | TBD | 2-3 days |
