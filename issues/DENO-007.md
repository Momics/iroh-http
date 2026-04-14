---
id: "DENO-007"
title: "Output-buffer resize path replays FFI methods and can duplicate side effects"
status: open
priority: P1
date: 2026-04-13
area: deno
package: iroh-http-deno
tags: [deno, ffi, buffering, correctness]
---

# [DENO-007] Output-buffer resize path replays FFI methods and can duplicate side effects

## Summary

When `call()` receives a negative length (buffer too small), it allocates a larger buffer and invokes `iroh_http_call` again. Because Rust dispatch runs before size validation, the method body executes twice on this retry path.

## Evidence

- `packages/iroh-http-deno/src/adapter.ts:149` — first `iroh_http_call` invocation.
- `packages/iroh-http-deno/src/adapter.ts:161` — second invocation after buffer resize.
- `packages/iroh-http-deno/src/lib.rs:73` — dispatch executes method logic before output-size check.
- `packages/iroh-http-deno/src/lib.rs:80` — buffer-capacity check happens after dispatch result is produced.

## Impact

Any non-idempotent method can run twice when responses exceed the initial buffer hint, causing duplicate side effects and hard-to-diagnose behavior under large payloads or error responses.

## Remediation

1. Introduce a two-step protocol: query required size without executing side effects twice, then copy the already-produced response.
2. Alternatively, redesign FFI to write into caller-owned buffers via explicit status codes that do not re-dispatch method logic.
3. Add invariants/tests for large responses on state-mutating methods.

## Acceptance criteria

1. Buffer-resize retries do not execute method handlers more than once per logical call.
2. A regression test forces a small initial output buffer and confirms non-idempotent methods are not replayed.
3. Existing smoke/compliance tests continue to pass with the new FFI contract.
