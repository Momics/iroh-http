---
id: "TEST-005"
title: "CI: add Node compliance and cross-runtime gate"
status: fixed
priority: P1
date: 2026-04-14
area: infra
package: ""
tags: [testing, ci, type-checking, cross-runtime]
---

# [TEST-005] CI: add Node compliance and cross-runtime gate

> **Note:** Python tests and pyright were originally part of this issue but
> the Python adapter has been removed. The remaining CI additions (Node
> compliance, cross-runtime gate) are still in place.

## Summary

CI currently runs: `cargo test`, `cargo clippy`, `tsc`, Node e2e, and Deno
smoke. Missing from CI: Python pytest (35+ tests exist but never run
automatically), pyright type checking, Node compliance (12 cases), and
cross-runtime compliance (node↔deno via `run.sh`). This means Python
regressions and type-safety bugs are only caught manually.

## Evidence

- `.github/workflows/ci.yml` — 4 jobs: rust-check, rust-check-no-default-features,
  typescript-check, e2e
- `packages/iroh-http-py/tests/` — 5 test files, ~35 tests, not referenced
  in CI
- `packages/iroh-http-node/test/compliance.mjs` — 12 compliance cases, not
  in CI
- `tests/http-compliance/run.sh` — cross-runtime orchestrator, not in CI

## Impact

- Python bugs can reach main without any automated check
- Type-safety bugs (9 closed issues) in Python are undetectable without pyright
- The 12 compliance cases run only when a developer remembers to invoke them
- Wire protocol regressions between Node and Deno are not gated

## Remediation

Add 3 new CI steps/jobs to `.github/workflows/ci.yml`:

### 1. Python check job (after rust-check)
```yaml
python-check:
  runs-on: ubuntu-latest
  needs: [rust-check]
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
    - uses: actions/setup-python@v5
      with: { python-version: "3.11" }
    - run: pip install maturin pytest pytest-asyncio pyright
    - run: cd packages/iroh-http-py && maturin develop
    - run: cd packages/iroh-http-py && pyright iroh_http/
    - run: cd packages/iroh-http-py && python -m pytest tests/ -v
```

### 2. Node compliance step (in existing e2e job)
```yaml
- name: Node compliance
  run: node packages/iroh-http-node/test/compliance.mjs
```

### 3. Cross-runtime step (in existing e2e job)
```yaml
- name: Cross-runtime compliance (node↔deno)
  run: bash tests/http-compliance/run.sh --pairs node-deno,deno-node
```

### 4. Update `docs/build-and-test.md`
Document new CI jobs and local run commands.

## Acceptance criteria

1. CI has a `python-check` job that runs pyright and pytest
2. Node compliance (12 cases) runs in the e2e job
3. Cross-runtime node↔deno runs in the e2e job
4. A PR that breaks Python type stubs fails on `python-check`
5. A PR that breaks Node compliance fails on `e2e`
6. `docs/build-and-test.md` documents all test commands
