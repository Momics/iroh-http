---
id: "TAURI-006"
title: "npm package name does not follow tauri-plugin-{name} convention"
status: open
priority: P2
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["packaging", "convention"]
---

# [TAURI-006] npm package name does not follow tauri-plugin-{name} convention

## Summary

The npm package is named `@momics/iroh-http-tauri` but Tauri v2 requires the npm package to follow the `tauri-plugin-{name}` pattern so that the JavaScript runtime can locate the plugin by the same name used in the Rust `Builder::new("iroh-http")` call.

## Evidence

- `packages/iroh-http-tauri/package.json:3` — `"name": "@momics/iroh-http-tauri"`
- `packages/iroh-http-tauri/src/lib.rs:10` — `Builder::new("iroh-http")` → expects JS package `tauri-plugin-iroh-http`
- Tauri v2 docs: "The package name has to follow the pattern `tauri-plugin-{name}`" (https://v2.tauri.app/develop/plugins/)

## Impact

Consumers importing via the Tauri JS bridge will be unable to resolve the plugin package by the conventional name. Tooling that scaffolds or validates plugin references will not recognise it, and the package cannot be published to npm with discoverability under the Tauri plugin namespace.

## Remediation

1. Rename `"name"` in `packages/iroh-http-tauri/package.json` from `"@momics/iroh-http-tauri"` to `"tauri-plugin-iroh-http"`.
2. Update any internal workspace references (e.g. in root `package.json` or apps that depend on it) to use the new name.
3. Update imports in `packages/iroh-http-tauri/guest-js/index.ts` if it references the package name.

## Acceptance criteria

1. `packages/iroh-http-tauri/package.json` has `"name": "tauri-plugin-iroh-http"`.
2. All workspace `package.json` files that depend on the tauri plugin reference `"tauri-plugin-iroh-http"`.
3. `npm run build` succeeds in the tauri package.
