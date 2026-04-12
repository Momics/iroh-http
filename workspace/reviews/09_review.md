---
status: proposed
source: docs/guidelines*.md + repo structure audit
date: 2026-04-12
---

# Systemic Multi-Reviewer Plan (Rust Core + Bridge + JS/Python Adapters)

This plan is designed for parallel execution by multiple people or agents while
keeping a single quality bar and one final synthesis.

## Goal

Deliver an end-to-end assessment of:

- Rust core correctness and safety
- Adherence to `docs/guidelines.md` and language-specific guidelines
- Bridge contract consistency across Node, Deno, Tauri, and Python
- Build-vs-buy opportunities to replace custom logic with trusted packages

## Operating Model

- One review lead owns orchestration and final synthesis.
- Work is split into six independent streams.
- Every stream uses the same finding template and severity rubric.
- All findings roll up into one tracker and one prioritized backlog.

## Streams

| Stream | Owner | Scope | Primary Output |
|---|---|---|---|
| S1 | Rust core reviewer | `crates/iroh-http-core`, `crates/iroh-http-framing`, `crates/iroh-http-discovery` | `streams/01_rust_core.md` + findings |
| S2 | Bridge contract reviewer | `packages/iroh-http-shared/src` | `streams/02_shared_bridge_contract.md` + findings |
| S3 | Node reviewer | `packages/iroh-http-node` | `streams/03_node_adapter.md` + findings |
| S4 | Deno/Tauri reviewer | `packages/iroh-http-deno`, `packages/iroh-http-tauri` | `streams/04_deno_tauri_adapters.md` + findings |
| S5 | Python reviewer | `packages/iroh-http-py` | `streams/05_python_adapter.md` + findings |
| S6 | Build-vs-buy reviewer | cross-cutting (Rust + JS/Python) | `streams/06_trusted_package_matrix.md` |

## Shared Standards

- Severity: `P0` (critical), `P1` (high), `P2` (medium), `P3` (low).
- Finding IDs: `R9-S<stream>-NN` (example: `R9-S1-03`).
- Every finding must include:
  - guideline reference
  - concrete evidence (file + line)
  - impact
  - proposed fix
  - required tests

Use:

- `templates/finding.md`
- `templates/stream_handoff.md`
- `templates/package_decision_matrix.md`

## Execution Sequence

1. Review lead creates initial tracker entries in `09_systemic/tracker.md`.
2. All streams run baseline checks in their own scope first.
3. Streams submit findings using the shared template.
4. S2 validates cross-platform bridge invariants using S3/S4/S5 evidence.
5. S6 produces package decision matrix with adopt/keep rationale.
6. Review lead resolves conflicts and publishes one consolidated backlog.

## Output Location

- Master tracker: `workspace/reviews/09_systemic/tracker.md`
- Stream briefs: `workspace/reviews/09_systemic/streams/`
- Templates: `workspace/reviews/09_systemic/templates/`
- Collected evidence and logs: `workspace/reviews/09_systemic/evidence/`

## Definition of Done

- All six streams marked complete in tracker.
- No unresolved contradictions between stream findings.
- Each P0/P1 finding has an explicit owner and remediation path.
- Package decision matrix exists for major custom subsystems.
- Final synthesis document created by review lead.
