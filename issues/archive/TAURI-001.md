---
id: "TAURI-001"
title: "Broken CommonJS export path — dist/index.cjs is not produced"
status: closed
priority: P1
date: 2026-04-13
area: tauri
package: iroh-http-tauri
tags: [tauri, packaging, cjs, exports]
---

# [TAURI-001] Broken CommonJS export path

## Summary

`package.json` maps `require("@momics/iroh-http-tauri")` to `dist/index.cjs`, but that file is not produced by the build. CommonJS consumers receive `MODULE_NOT_FOUND` at runtime.

## Evidence

- `packages/iroh-http-tauri/package.json:10` — CJS `exports` field points to `dist/index.cjs`
- `packages/iroh-http-tauri/dist/` — `index.cjs` is absent

## Impact

Any CommonJS environment (Electron main process, older toolchains) that `require`s this package fails immediately at startup.

## Remediation

1. Add `dist/index.cjs` to the build output, or change the `exports` map to point to an existing file for the CJS condition.

## Acceptance criteria

1. `node -e "require('@momics/iroh-http-tauri')"` succeeds without `MODULE_NOT_FOUND`.
