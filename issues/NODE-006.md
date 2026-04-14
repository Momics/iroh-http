---
id: "NODE-006"
title: "Node serve bridge can surface unhandled promise rejections when rawRespond throws"
status: open
priority: P1
date: 2026-04-13
area: node
package: iroh-http-node
tags: [node, serve, error-handling, async]
---

# [NODE-006] Node serve bridge can surface unhandled promise rejections when `rawRespond` throws

## Summary

The Node `rawServe` wrapper handles handler errors with `.catch(...)`, but both success and error paths call `rawRespond` without local `try/catch`. If `rawRespond` throws, the rejection is not safely contained.

## Evidence

- `packages/iroh-http-node/lib.ts:150` — success path calls `napiRawRespond(...)` directly
- `packages/iroh-http-node/lib.ts:158` — error path also calls `napiRawRespond(...)` directly
- `packages/iroh-http-node/src/lib.rs:644` — `raw_respond` can return `napi::Result<()>` (error path exists)

## Impact

Serve-path failures can leak as unhandled promise rejections or noisy runtime errors, reducing reliability under exceptional conditions.

## Remediation

1. Wrap both `napiRawRespond(...)` calls in `try/catch` at the JS boundary.
2. Log fallback failures with enough request context to debug.
3. Add a regression test that forces `rawRespond` failure and asserts no unhandled rejection is emitted.

## Acceptance criteria

1. Exceptions thrown from `rawRespond` are caught in the adapter and do not produce unhandled rejections.
2. A targeted test verifies the failure path is contained and logged.

