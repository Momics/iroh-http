---
id: "B-ISS-042"
title: "JS guidelines error .name table doesn't match errors.ts implementation"
status: fixed
priority: P1
date: 2026-04-14
area: docs
package: iroh-http-shared
tags: [docs, errors, javascript, correctness]
---

# [B-ISS-042] JS guidelines error .name table doesn't match errors.ts implementation

## Summary

The JS guidelines document (`docs/guidelines/javascript.md`) maps `TIMEOUT` → `IrohConnectError` with `.name = "TimeoutError"`. In the actual implementation, `IrohConnectError` sets `this.name = "NetworkError"`. A developer matching on `e.name === "TimeoutError"` as the docs instruct will never match. The same inconsistency likely affects other rows in the table.

## Evidence

- `docs/guidelines/javascript.md` — error table lists `TIMEOUT` → `.name = "TimeoutError"`, `ABORT` → `.name = "AbortError"`, `INVALID_HANDLE` → `.name = "InvalidHandle"`, `STREAM_RESET` → `.name = "StreamReset"`
- `packages/iroh-http-shared/src/errors.ts` — `IrohConnectError` sets `this.name = "NetworkError"`, not `"TimeoutError"`
- `packages/iroh-http-shared/src/errors.ts` — `IrohAbortError` sets `this.name = "AbortError"` (correct)

## Impact

Developers following the guidelines who pattern-match on `.name` will write dead error-handling branches for `TIMEOUT`. The docs describe a desired state rather than the actual implemented state.

## Remediation

1. Audit every subclass in `errors.ts` and record the actual `.name` values.
2. Update the table in `docs/guidelines/javascript.md` to match the implementation.
3. If any `.name` values are wrong in the implementation (e.g.`"NetworkError"` on a timeout is misleading), fix the implementation and update the docs to match.

## Acceptance criteria

1. The error table in `javascript.md` is a correct reflection of the `.name` property on each subclass in `errors.ts`.
2. A snapshot or type-level test asserts the `.name` on each subclass.
