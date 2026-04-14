---
id: "TEST-003"
title: "Python adapter: expand integration tests for error paths, limits, concurrency, and large bodies"
status: superseded
priority: P1
date: 2026-04-14
area: python
package: "iroh-http-py"
tags: [testing, integration, ffi-boundary, regression]
---

# [TEST-003] Python adapter — SUPERSEDED

> **Note:** The Python adapter was removed from the project (see
> `issues/REMOVE-PYTHON.md`). This issue is no longer applicable.
> Python support may be re-added in a future horizon.

## Original Summary

`packages/iroh-http-py/tests/test_node.py` has ~15 tests covering basic
lifecycle and happy-path serve/fetch. The Python adapter had 9 FFI-boundary
and type-safety bugs (PY-001, PY-002, PY-006, PY-010, PY-011, PY-012,
PY-013). Missing: error handling, limits enforcement, concurrent requests,
and large body streaming.

## Evidence

- `packages/iroh-http-py/tests/test_node.py` — ~15 tests: create_node
  variants, keypair/addr/ticket, context manager, basic serve/fetch,
  response.text()/json(), handler exception → 500, URL scheme rejection
- PY-001 (serve panic outside Tokio), PY-011 (serve outside runtime),
  PY-012 (node.closed blocks), PY-013 (context manager lifecycle) — all
  FFI boundary bugs

## Impact

Python's PyO3 FFI layer is the thickest adapter (25 parameters on
`create_node`). Each parameter is a potential type coercion bug. Integration
tests are the only defence — Python has no compile-time type checking at the
FFI boundary.

## Remediation

Add the following tests:

1. **Invalid input:** `fetch(invalid_node_id, "/path")` → raises
   `RuntimeError`
2. **Handler exception → 500:** handler raises `ValueError` → client gets
   status 500 (may already exist — verify and strengthen)
3. **Large body round-trip:** 1 MiB POST body → echo-length returns
   `"1048576"`
4. **Concurrent requests:** 5 concurrent `asyncio.gather(fetch(...))` → all
   return correct bodies
5. **Context manager cleanup:** `async with create_node() as node:` followed
   by access → node is closed

## Acceptance criteria

1. `cd packages/iroh-http-py && python -m pytest tests/ -v` passes with ≥ 25
   tests across all test files
2. Uses pytest-asyncio (same framework as existing tests)
3. No new pip dependencies beyond pytest + pytest-asyncio
