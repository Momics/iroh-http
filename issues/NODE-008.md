---
id: "NODE-008"
title: "Platform support signaling is ambiguous between declared targets and loader fallbacks"
status: open
priority: P2
date: 2026-04-13
area: node
package: iroh-http-node
tags: [node, packaging, platforms, napi]
---

# [NODE-008] Platform support signaling is ambiguous between declared targets and loader fallbacks

## Summary

`package.json` declares a limited NAPI target set, but the generated loader still attempts many extra platform-specific fallback packages that are not declared in dependencies.

## Evidence

- `packages/iroh-http-node/package.json:9` — NAPI targets include only Darwin + Linux GNU variants
- `packages/iroh-http-node/index.js:169` — loader branches for Linux musl variants
- `packages/iroh-http-node/index.js:190` — loader attempts `@momics/iroh-http-node-linux-x64-gnu` package fallback

## Impact

On unsupported or partially supported platforms, users may get confusing runtime `MODULE_NOT_FOUND` failures instead of clear upfront support guidance.

## Remediation

1. Define and document the explicit support matrix in package metadata and README.
2. Either publish all loader-referenced platform packages or trim loader/fallback expectations to shipped targets.
3. Improve startup error messages to indicate supported platforms and next steps.

## Acceptance criteria

1. Loader behavior and published package metadata are consistent for every advertised platform.
2. Unsupported platforms fail with a clear, actionable error message.

