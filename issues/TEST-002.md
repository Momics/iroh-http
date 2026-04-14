---
id: "TEST-002"
title: "Deno adapter: add integration tests for error paths, limits, cancellation, and key exports"
status: fixed
priority: P1
date: 2026-04-14
area: deno
package: "iroh-http-deno"
tags: [testing, integration, ffi-boundary, regression]
---

# [TEST-002] Deno adapter: add integration tests for error paths, limits, cancellation, and key exports

## Summary

`packages/iroh-http-deno/test/smoke.test.ts` has 17 tests mixing smoke checks
and happy-path integration. Missing: error classification, cancellation,
server limits, and post-A-ISS-050 key class re-exports. The Deno adapter had
7 FFI-boundary bugs (DENO-001 through DENO-007, BUG-001 through BUG-003).

## Evidence

- `packages/iroh-http-deno/test/smoke.test.ts` — 17 tests: node creation,
  deterministic keys, ticket/addr, crypto (6 tests), serve+fetch, concurrent
  requests, URL scheme rejection
- DENO-001 (shared buffer corruption), DENO-005 (cancel token wrong endpoint),
  DENO-007 (output buffer replay), BUG-001 (response mis-routing) — all FFI
  boundary bugs

## Impact

Deno has the most complex FFI layer (JSON dispatch over C-ABI dlopen). Its bug
density per adapter is the highest. Integration tests covering error paths and
concurrency would have caught most of these.

## Remediation

Add the following tests to `smoke.test.ts`:

1. **Error classification:** handler throws → client gets 500; verify error
   type
2. **Cancellation:** fetch with `AbortSignal.timeout(1)` against slow handler
   → throws `AbortError`
3. **Server limits:** `maxRequestBodyBytes` exceeded → 413
4. **Serve lifecycle:** start serve, then `serveHandle.close()` → resolves
   cleanly
5. **Key class re-exports:** `import { PublicKey, SecretKey } from
   "@momics/iroh-http-deno"` → `PublicKey.fromString(nodeId)` works

## Acceptance criteria

1. `deno test --allow-read --allow-ffi --allow-env --allow-net packages/iroh-http-deno/test/smoke.test.ts`
   passes with ≥ 22 tests
2. Uses `Deno.test()` (same framework as existing tests)
3. No new dependencies
