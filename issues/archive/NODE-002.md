---
id: "NODE-002"
title: "Windows packaging path is broken — missing .node binaries and optionalDependencies"
status: closed
priority: P1
date: 2026-04-13
area: node
package: iroh-http-node
tags: [node, windows, packaging, binaries]
---

# [NODE-002] Windows packaging path is broken

## Summary

`package.json` declares Windows targets, but no Windows `.node` binaries are shipped and there are no `optionalDependencies` fallback package declarations. The generated loader still attempts `require('@momics/iroh-http-node-win32-x64-msvc')`, which fails.

## Evidence

- `packages/iroh-http-node/package.json:9` — declares Windows targets
- `packages/iroh-http-node/package.json:34` — no Windows `optionalDependencies`
- `packages/iroh-http-node/index.js:65` — loader attempts `require('@momics/iroh-http-node-win32-x64-msvc')`

## Impact

`require('@momics/iroh-http-node')` fails on any Windows machine with a `MODULE_NOT_FOUND` error.

## Remediation

1. Either build and ship Windows `.node` binaries with proper `optionalDependencies` declarations, or remove Windows targets from `package.json` and the loader until binaries are available.

## Acceptance criteria

1. `require('@momics/iroh-http-node')` succeeds on Windows, or the Windows targets are explicitly removed with a clear note.
