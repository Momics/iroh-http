---
id: "TEST-001"
title: "Node adapter: expand integration tests to cover error paths, limits, crypto, and cancellation"
status: fixed
priority: P1
date: 2026-04-14
area: node
package: "iroh-http-node"
tags: [testing, integration, ffi-boundary, regression]
---

# [TEST-001] Node adapter: expand integration tests to cover error paths, limits, crypto, and cancellation

## Summary

`packages/iroh-http-node/test/e2e.mjs` has 7 tests covering only happy-path
fetch/serve. The Node adapter boundary was the source of 5+ FFI bugs
(NODE-006, NODE-007, NODE-008, ISS-030, ISS-031). Same-process integration
tests are the most effective way to prevent these from recurring.

## Evidence

- `packages/iroh-http-node/test/e2e.mjs` — 7 tests: basic GET, POST, path
  reflection, concurrent requests, plain response, URL scheme rejection (×2)
- NODE-006 (serve unhandled rejection), NODE-007 (FFI numeric lossy-cast),
  ISS-030 (CANCELLED not mapped), ISS-031 (cancel token ep_idx=0) — all were
  FFI boundary bugs that same-process tests would have caught

## Impact

FFI boundary bugs are the #2 root cause category (19 of 106 closed issues).
Per-adapter integration tests are the highest-leverage investment to prevent
regressions.

## Remediation

Add the following tests to `e2e.mjs`:

1. **Error classification:** handler throws synchronously → client gets 500;
   verify response is an `IrohError` subclass with correct `.name`
2. **Async error:** handler rejects with async error → client gets 500
3. **Crypto round-trip:** `SecretKey.generate()`, `secretKey.sign(data)`,
   `publicKey.verify(data, sig)` via the re-exported classes from
   `@momics/iroh-http-node`
4. **Cancellation:** fetch with `AbortSignal.timeout(1)` against a 5-second
   handler → throws `AbortError`
5. **Server limits:** create node with `maxRequestBodyBytes: 100`, POST 1 KiB
   body → 413 response
6. **Node ID header:** `peer-id` header is present, valid base32 (≥52
   chars), and consistent across two sequential requests
7. **Handle lifecycle:** `node.close()` twice does not throw; close during
   active serve completes gracefully
8. **Large body streaming:** 1 MiB POST body round-trip → echoed back exactly

## Acceptance criteria

1. `node packages/iroh-http-node/test/e2e.mjs` passes with ≥ 15 tests
2. All new tests use `node:test` (same framework as existing tests)
3. No new dependencies added
