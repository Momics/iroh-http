---
id: "DENO-006"
title: "Serve startup and loop failures are swallowed instead of surfacing to callers"
status: fixed
priority: P2
date: 2026-04-13
area: deno
package: iroh-http-deno
tags: [deno, serve, error-handling, observability]
---

# [DENO-006] Serve startup and loop failures are swallowed instead of surfacing to callers

## Summary

The Deno adapter catches `serveStart` and serve-loop errors and only logs them. The returned `ServeHandle` still appears healthy, so callers cannot reliably detect that serving failed.

## Evidence

- `packages/iroh-http-deno/src/adapter.ts:366` — loop failures are caught and logged.
- `packages/iroh-http-deno/src/adapter.ts:370` — `serveStart` failures are caught and logged.
- `packages/iroh-http-shared/src/serve.ts:161` — `rawServe(...)` returns a `loopDone` promise.
- `packages/iroh-http-shared/src/serve.ts:300` — returned `finished` promise does not propagate `loopDone` failure.

## Impact

Applications can believe their server is running while no requests are actually being served. This degrades reliability and complicates incident diagnosis because failures only appear in logs.

## Remediation

1. Remove catch-and-log suppression in the adapter path for `serveStart` and loop errors.
2. Propagate failures through `rawServe` rejection.
3. Wire `ServeHandle.finished` to reject on serve-loop failure (or expose a dedicated error signal).

## Acceptance criteria

1. If `serveStart` fails, `serve()` reports failure to the caller.
2. If the polling loop crashes, `finished` rejects with the underlying error.
3. Smoke tests assert failure propagation, not just console output.
