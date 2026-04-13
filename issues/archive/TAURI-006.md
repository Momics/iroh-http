---
id: "TAURI-006"
title: "npm package name does not follow Tauri scoped naming convention"
status: wont-fix
priority: P2
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["packaging", "convention"]
---

# [TAURI-006] npm package name does not follow Tauri scoped naming convention

## Summary

The npm package is named `@momics/iroh-http-tauri` but the Tauri v2 convention for scoped packages is `@scope-name/plugin-{plugin-name}`. The correct name would be `@momics/plugin-iroh-http`.

**Resolution:** The name `@momics/iroh-http-tauri` is intentionally kept. The `tauri` suffix makes it immediately clear the package is a Tauri plugin, which `plugin-` alone does not convey when the package is encountered outside of a Tauri project context. npm names are freeform; the Tauri convention is a recommendation for public discoverability, not a runtime requirement.

## Evidence

- `packages/iroh-http-tauri/package.json:3` — `"name": "@momics/iroh-http-tauri"`
- Tauri v2 docs (https://v2.tauri.app/develop/plugins/#naming-convention): _"The Tauri naming convention for NPM packages is `@scope-name/plugin-{plugin-name}`"_. For an unscoped package the convention is `tauri-plugin-{name}-api`.
- The Rust crate name `tauri-plugin-iroh-http` in `Cargo.toml` is already correct.

## Impact

Deviating from the convention reduces discoverability and may confuse tooling or consumers expecting the standard pattern. It is a cosmetic issue today but should be fixed before any public release.

## Remediation

1. Rename `"name"` in `packages/iroh-http-tauri/package.json` from `"@momics/iroh-http-tauri"` to `"@momics/plugin-iroh-http"`.
2. Update any internal workspace references (e.g. in root `package.json` or apps that depend on it) to use the new name.
3. Update imports in `packages/iroh-http-tauri/guest-js/index.ts` if it references the package name.

## Acceptance criteria

1. `packages/iroh-http-tauri/package.json` has `"name": "@momics/plugin-iroh-http"`.
2. All workspace `package.json` files that depend on the tauri plugin reference `"@momics/plugin-iroh-http"`.
3. `npm run build` succeeds in the tauri package.
