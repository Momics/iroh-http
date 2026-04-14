---
id: "TEST-006"
title: "Establish regression test policy and update issue template"
status: open
priority: P2
date: 2026-04-14
area: testing
package: ""
tags: [testing, policy, process, regression]
---

# [TEST-006] Establish regression test policy and update issue template

## Summary

106 issues have been closed without a systematic policy for regression tests.
Future issues risk the same pattern: fix goes in, no test is written, the same
bug class re-emerges. A lightweight policy ensures every fix leaves a
permanent test receipt in the appropriate layer.

## Evidence

- `issues/_template.md` — no `## Regression test` section
- `.github/copilot-instructions.md` — no mention of test requirements for
  issue resolution
- 19 FFI-boundary bugs were fixed without per-adapter regression tests
  covering the specific failure mode

## Impact

Without a policy, the cost of fixing bugs is paid repeatedly. Agents and
contributors close issues without proof the fix is durable.

## Remediation

### 1. Update `issues/_template.md`

Add after the `## Acceptance criteria` section:

```markdown
## Regression test

- Layer: rust-core | node | deno | python | cross-runtime | type-check | N/A
- Test: `test name or file path`
- Verified failing before fix: yes | N/A
```

### 2. Update `.github/copilot-instructions.md`

Add a `## Issue Resolution Policy` section:

```markdown
## Issue Resolution Policy

Every fixed issue must leave a regression test in the appropriate layer:
- FFI boundary bugs → per-adapter integration test (e2e.mjs, smoke.test.ts, test_node.py)
- Rust core bugs → cargo test (integration.rs or new test file)
- Type/export bugs → verified by tsc/pyright (no new test needed if CI gates it)
- Protocol behavior → cases.json entry
- Docs/build/config → N/A (document in issue)
```

### 3. Backfill 5–10 high-value regression tests

Scan the 19 FFI-boundary issues for any whose specific failure mode is not
covered by existing tests. Write targeted regression tests for the highest-risk
ones (e.g., DENO-001 buffer corruption, NODE-007 numeric lossy-cast,
PY-011 serve outside Tokio).

## Acceptance criteria

1. `issues/_template.md` has `## Regression test` section
2. `.github/copilot-instructions.md` has `## Issue Resolution Policy`
3. At least 5 closed FFI-boundary issues have a corresponding regression test
   in the appropriate adapter test suite
