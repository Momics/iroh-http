---
id: "A-ISS-042"
title: "normaliseRelayMode and base64 helpers duplicated across JS/TS adapters"
status: open
priority: P2
date: 2026-04-14
area: core
package: "iroh-http-shared"
tags: [architecture, duplication, shared]
---

# [A-ISS-042] normaliseRelayMode and base64 helpers duplicated across JS/TS adapters

## Summary

Two categories of utility code are duplicated across adapter TypeScript files instead of being centralized in `iroh-http-shared`:

1. **`normaliseRelayMode()`** — identical relay mode normalization logic in Node, Deno, and Tauri (~40 lines × 3).
2. **`encodeBase64()` / `decodeBase64()`** — identical base64 helpers in Deno and Tauri (~15 lines × 2).

## Evidence

**normaliseRelayMode duplicates:**
- `packages/iroh-http-node/lib.ts:190` — full implementation
- `packages/iroh-http-deno/src/adapter.ts:379` — identical copy
- `packages/iroh-http-tauri/guest-js/index.ts:355` — identical copy

**base64 helper duplicates:**
- `packages/iroh-http-deno/src/adapter.ts:75-83` — `encodeBase64` / `decodeBase64`
- `packages/iroh-http-tauri/guest-js/index.ts:20-29` — identical copy

## Impact

- When relay mode semantics change (e.g., adding a new strategy), three files must be updated independently.
- Bug fixes in relay mode normalization must be manually ported across adapters.
- Violates Principle 3 ("Leverage, Don't Reinvent") — `iroh-http-shared` exists precisely for this purpose.

## Remediation

1. Move `normaliseRelayMode()` to `iroh-http-shared` (e.g., in a new `relay.ts` or in the existing `bridge.ts`).
2. Move `encodeBase64()` / `decodeBase64()` to `iroh-http-shared` (e.g., in an existing utility module or alongside `keys.ts`).
3. Update Node, Deno, and Tauri adapters to import from `@momics/iroh-http-shared`.

## Acceptance criteria

1. `normaliseRelayMode` exists in exactly one location (`iroh-http-shared`).
2. Base64 utilities exist in exactly one location (`iroh-http-shared`).
3. All three JS/TS adapters import these from the shared package.
