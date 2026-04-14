---
id: "ISS-000"
title: "Short descriptive title"
status: closed
# status options: open | in-progress | fixed | wont-fix | duplicate
priority: P1
# priority options: P0 (critical) | P1 (high) | P2 (medium) | P3 (low)
date: YYYY-MM-DD
area: core
# area options: core | node | deno | tauri | docs | infra | protocol | testing
package: ""
# package: iroh-http-core | iroh-http-discovery | iroh-http-node | iroh-http-deno | iroh-http-tauri | iroh-http-shared
tags: []
---

# [ISS-000] Short descriptive title

## Summary

One or two sentences describing the problem. What is wrong and where.

## Evidence

Specific file references, code locations, or doc excerpts that demonstrate the issue.

- `path/to/file.rs:LINE` — description of what this shows
- `docs/features/feature.md:LINE` — description of what this promises

## Impact

What breaks at runtime, how callers are affected, and the severity rationale.

## Remediation

Concrete steps to fix the issue.

1. ...
2. ...

## Acceptance criteria

How to verify the fix is complete.

1. ...
2. ...

## Regression test

- Layer: rust-core | node | deno | python | cross-runtime | type-check | N/A
- Test: `test name or file path`
- Verified failing before fix: yes | N/A
