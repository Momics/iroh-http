---
id: "A-ISS-050"
title: "Crypto API surface: export key classes, remove raw functions"
status: resolved
resolution: "Phase 1 completed. Phase 2 (encrypt/decrypt) deferred — sealed-box encryption uses X25519 keys, not Ed25519 identity keys; mixing them on the same class conflates two distinct cryptographic roles. The sealed-messages recipe documents the pattern for users who need it."
priority: P1
date: 2026-04-14
area: core
package: "iroh-http-shared, iroh-http-node, iroh-http-deno, iroh-http-tauri"
tags: [api-design, crypto, keys, sign-verify]
---

# [A-ISS-050] Crypto API surface: export key classes, remove raw functions, add encrypt/decrypt

## Summary

The intended crypto API (`SecretKey.sign`, `PublicKey.verify`, `publicKey.encrypt`,
`secretKey.decrypt`) is partially implemented but not consistently exposed. The
adapter packages do not export `PublicKey` or `SecretKey` as named imports, so
callers cannot construct standalone keys. Raw functions (`secretKeySign`,
`publicKeyVerify`, `generateSecretKey`) are exported from some adapters but not
others, producing an inconsistent API surface and a confusing discovery experience.
Encrypted sealed-box functionality does not exist.

## Evidence

- `packages/iroh-http-deno/mod.ts:30-32` — exports `secretKeySign`, `publicKeyVerify`,
  `generateSecretKey` as top-level public API; `PublicKey` and `SecretKey` are not
  re-exported
- `packages/iroh-http-tauri/guest-js/index.ts:568-590` — same raw function exports;
  no key class exports
- `packages/iroh-http-node/lib.ts` — exports only `createNode`; no raw functions and
  no key class exports (closest to correct, but still missing class exports)
- `packages/iroh-http-shared/src/keys.ts:98` — `PublicKey.verify` exists and works
- `packages/iroh-http-shared/src/keys.ts:262` — `SecretKey.sign` exists and works
- `docs/features/sign-verify.md` — shows `PublicKey.fromString(nodeIdString)` without
  showing how to import `PublicKey`; no encrypt/decrypt documented

## Impact

1. **Users cannot import `PublicKey` / `SecretKey` from an adapter package.** They can
   access them via `node.publicKey` / `node.secretKey`, but cannot call
   `PublicKey.fromString(someNodeId)` without importing from the internal shared
   package directly — an import path not guaranteed to be stable.
2. **Three adapter packages export inconsistent APIs.** Deno and Tauri expose raw
   functions; Node does not. Code written for one adapter does not work on another.
3. **No sealed-box encrypt/decrypt.** The `sealed-messages` recipe shows ~80 lines of
   manual ECIES to achieve what should be two method calls. There is no first-class
   API for `publicKey.encrypt` / `secretKey.decrypt`.
4. **False sense of completeness.** The doc says "sign and verify operations" but
   encrypt/decrypt — equally fundamental — is absent.

## Remediation

### Phase 1 — API surface cleanup (no new functionality)

1. Add `export { PublicKey, SecretKey } from "@momics/iroh-http-shared"` to:
   - `packages/iroh-http-deno/mod.ts`
   - `packages/iroh-http-tauri/guest-js/index.ts`
   - `packages/iroh-http-node/lib.ts`

2. Remove the following from all public adapter exports:
   - `secretKeySign` (Deno `mod.ts` and Tauri `guest-js/index.ts`)
   - `publicKeyVerify` (Deno `mod.ts` and Tauri `guest-js/index.ts`)
   - `generateSecretKey` (Deno `mod.ts` and Tauri `guest-js/index.ts`)

   These are adapter-internal implementation details. They remain in `adapter.ts` /
   Tauri commands as private implementation. Replace with `SecretKey.generate()`.

### Phase 2 — Sealed-box encrypt/decrypt (DEFERRED)

**Decision:** encrypt/decrypt will NOT be added to `PublicKey`/`SecretKey`.

Sealed-box encryption requires X25519 keys (Diffie-Hellman), not Ed25519 keys
(signing). Adding `encrypt()`/`decrypt()` to identity key classes would conflate
two distinct cryptographic roles on one type. The correct approach is either a
separate library or the recipe pattern documented in
[sealed-messages.md](../docs/recipes/sealed-messages.md).

## Acceptance criteria

1. `import { PublicKey, SecretKey } from "@momics/iroh-http-deno"` compiles and
   `PublicKey.fromString(nodeId)` works at runtime.
2. Same for `@momics/iroh-http-node` and `@momics/iroh-http-tauri`.
3. `secretKeySign`, `publicKeyVerify`, and `generateSecretKey` are no longer named
   exports of any adapter package.
4. `await node.secretKey.sign(data)` and `await node.publicKey.verify(data, sig)` still
   work (non-regression).
5. Sealed-box encrypt/decrypt remains documented in `docs/recipes/sealed-messages.md`.
