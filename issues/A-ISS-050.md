---
id: "A-ISS-050"
title: "Crypto API surface: export key classes, remove raw functions, add encrypt/decrypt"
status: open
priority: P1
date: 2026-04-14
area: core
package: "iroh-http-shared, iroh-http-node, iroh-http-deno, iroh-http-tauri"
tags: [api-design, crypto, keys, sign-verify, encrypt]
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

### Phase 2 — Sealed-box encrypt/decrypt

3. Add `async encrypt(plaintext: Uint8Array): Promise<Uint8Array>` to `PublicKey` in
   `packages/iroh-http-shared/src/keys.ts`.

   Algorithm (sealed-box, ECIES variant):
   - Convert Ed25519 public key bytes → X25519 public key (Birkhoff–Edwards to
     Montgomery: `u = (1+y)/(1-y) mod p`)
   - Generate ephemeral X25519 keypair using `crypto.getRandomValues`
   - ECDH: `sharedSecret = x25519(ephemPriv, recipientX25519Pub)`
   - KDF: `key = HKDF-SHA256(sharedSecret, salt=ephemPub, info="")`
   - Encrypt: `ct = AES-GCM-256(key, iv=random(12), plaintext)`
   - Output: `[32B ephemPub] || [12B IV] || [ct] || [16B AES-GCM tag]`

4. Add `async decrypt(ciphertext: Uint8Array): Promise<Uint8Array>` to `SecretKey`.

   Algorithm:
   - Split: `ephemPub = ciphertext[0..32]`, `iv = ciphertext[32..44]`, `ct = ciphertext[44..]`
   - Convert Ed25519 secret key bytes → X25519 private key
     (`sha512(seed)[0..32]` with standard clamping)
   - ECDH: `sharedSecret = x25519(myX25519Priv, ephemPub)`
   - KDF: `key = HKDF-SHA256(sharedSecret, salt=ephemPub, info="")`
   - Decrypt: AES-GCM-256 decryption; throw `IrohError` on authentication failure

   Minimum ciphertext length is 60 bytes; throw `IrohArgumentError` if shorter.

5. Both operations use only `crypto.subtle` (WebCrypto) and inline BigInt math for the
   key conversion. No new dependencies, no new Rust required.

6. Update `docs/recipes/sealed-messages.md` to show the simplified 2-call version and
   cross-reference the manual ECIES construction as a footnote for callers who need a
   different cipher suite.

## Acceptance criteria

1. `import { PublicKey, SecretKey } from "@momics/iroh-http-deno"` compiles and
   `PublicKey.fromString(nodeId)` works at runtime.
2. Same for `@momics/iroh-http-node` and `@momics/iroh-http-tauri`.
3. `secretKeySign`, `publicKeyVerify`, and `generateSecretKey` are no longer named
   exports of any adapter package. Calling them produces a TypeScript compiler error.
4. `await node.publicKey.encrypt(data)` returns a `Uint8Array` ≥ 60 bytes.
5. `await node.secretKey.decrypt(ciphertext)` returns the original plaintext exactly.
6. `node.secretKey.decrypt(tamperedCiphertext)` throws `IrohError`.
7. `await node.secretKey.sign(data)` and `await node.publicKey.verify(data, sig)` still
   work (non-regression).
8. All three adapters (Node, Deno, Tauri) pass a new compliance case `crypto-sign-verify-encrypt-decrypt`.
9. `npm run typecheck` passes.

## Regression test

- `cases.json` case IDs: `reg-a-iss-050-sign-verify`, `reg-a-iss-050-encrypt-decrypt`
- Verified failing before fix: N/A (new features / missing exports)
