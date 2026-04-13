---
id: "DENO-001"
title: "Shared buffer in nonblocking nextChunk can corrupt concurrent body reads"
status: open
priority: P1
date: 2026-04-13
area: deno
package: iroh-http-deno
tags: [deno, concurrency, buffer, race-condition, safety]
---

# [DENO-001] Shared buffer in nonblocking `nextChunk` can corrupt concurrent body reads

## Summary

`iroh_http_next_chunk` is registered as `nonblocking: true`, but `bridge.nextChunk` reuses a single module-global `chunkBuf`. Two concurrent calls write into the same memory region, so one stream can receive bytes belonging to another stream.

## Evidence

- `packages/iroh-http-deno/src/adapter.ts:121-122` — module-global `chunkBuf` reused across calls

## Impact

Concurrent streaming reads silently corrupt each other's data. This is the same class of race previously fixed for `call()` output buffers, now present in the chunk path.

## Remediation

1. Allocate a per-call buffer instead of reusing the module-global `chunkBuf`.

## Acceptance criteria

1. Two concurrent body reads on different streams each receive the correct bytes without corruption.
