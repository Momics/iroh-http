---
id: "A-ISS-050"
title: "Crypto API surface: export key classes, rename Python functions, add encrypt/decrypt (all platforms)"
status: open
priority: P1
date: 2026-04-14
area: core
package: "iroh-http-shared, iroh-http-node, iroh-http-deno, iroh-http-tauri, iroh-http-py"
tags: [api-design, crypto, keys, sign-verify, encrypt, parity]
---

# [A-ISS-050] Crypto API surface: export key classes, rename Python functions, add encrypt/decrypt (all platforms)

## Summary

The intended crypto API (`SecretKey.sign`, `PublicKey.verify`, `publicKey.encrypt`,
`secretKey.decrypt` in JS; `sign`, `verify`, `encrypt`, `decrypt` in Python) is
partially implemented but not consistently exposed, and encrypt/decrypt does not
exist on any platform. The JS adapter packages do not export `PublicKey` or
`SecretKey` as named imports. Raw functions (`secretKeySign`, `publicKeyVerify`,
`generateSecretKey`) are exported from some JS adapters but not others. Python
has `secret_key_sign` and `public_key_verify` as awkward compound names instead
of the clean `sign`/`verify`. No platform has sealed-box encrypt/decrypt.

## Evidence

**JS adapters:**
- `packages/iroh-http-deno/mod.ts:30-32` — exports `secretKeySign`, `publicKeyVerify`,
  `generateSecretKey` as top-level public API; `PublicKey` and `SecretKey` are not
  re-exported
- `packages/iroh-http-tauri/guest-js/index.ts:568-590` — same raw function exports;
  no key class exports
- `packages/iroh-http-node/lib.ts` — exports only `createNode`; no raw functions and
  no key class exports (closest to correct, but still missing class exports)
- `packages/iroh-http-shared/src/keys.ts:98` — `PublicKey.verify` exists and works
- `packages/iroh-http-shared/src/keys.ts:262` — `SecretKey.sign` exists and works

**Python:**
- `packages/iroh-http-py/src/lib.rs` (key operations section) — exposes
  `secret_key_sign`, `public_key_verify`, `generate_secret_key` as module-level
  functions; names carry a type prefix that is unnecessary in Python
- `packages/iroh-http-py/iroh_http/__init__.pyi` — stubs reflect the raw naming;
  no `sign`, `verify`, `encrypt`, `decrypt` signatures present
- `packages/iroh-http-py/tests/test_crypto.py` — imports `secret_key_sign`,
  `public_key_verify` directly; no encrypt/decrypt tests

**All platforms:**
- No `encrypt`/`decrypt` functionality exists on any platform
- `docs/features/sign-verify.md` — previously had no Python section and no
  encrypt/decrypt; updated in docs but implementation still missing

## API mapping

The canonical API surface expressed per platform:

| Operation | JS | Python |
|---|---|---|
| Generate key | `SecretKey.generate()` | `generate_secret_key() → bytes` |
| Sign | `await secretKey.sign(data)` | `sign(key: bytes, data: bytes) → bytes` |
| Verify | `await publicKey.verify(data, sig)` | `verify(key: bytes, data: bytes, sig: bytes) → bool` |
| Encrypt to peer | `await publicKey.encrypt(plaintext)` | `encrypt(key: bytes, plaintext: bytes) → bytes` |
| Decrypt | `await secretKey.decrypt(ciphertext)` | `decrypt(key: bytes, ciphertext: bytes) → bytes` |
| Node's signing key | `node.secretKey → SecretKey` | `node.secret_key → bytes` |
| Node's public key | `node.publicKey → PublicKey` | `node.public_key → str` (base32) |

## Impact

1. **JS — users cannot import `PublicKey` / `SecretKey` from an adapter package.**
   They can access them via `node.publicKey` / `node.secretKey`, but cannot call
   `PublicKey.fromString(someNodeId)` without importing from the internal shared
   package — an import path not guaranteed to be stable.
2. **JS — three adapter packages export inconsistent APIs.** Deno and Tauri expose
   raw byte-array functions; Node does not. Code written for one adapter does not
   work on another.
3. **Python — `secret_key_sign` / `public_key_verify` are confusingly named.** The
   type prefix is unnecessary because Python functions always take typed arguments;
   `sign(key, data)` and `verify(key, data, sig)` are idiomatic.
4. **All platforms — no sealed-box encrypt/decrypt.** The `sealed-messages` recipe
   shows ~80 lines of manual ECIES to achieve what should be two function calls.
5. **Parity gap.** The goal is identical API surfaces across all platforms (with only
   per-platform naming-convention adjustments). Crypto is the most lagging area.

## Remediation

### Phase 1 — JS API surface cleanup (no new functionality)

1. Add `export { PublicKey, SecretKey } from "@momics/iroh-http-shared"` to:
   - `packages/iroh-http-deno/mod.ts`
   - `packages/iroh-http-tauri/guest-js/index.ts`
   - `packages/iroh-http-node/lib.ts`

2. Remove the following from all public JS adapter exports:
   - `secretKeySign` (Deno `mod.ts` and Tauri `guest-js/index.ts`)
   - `publicKeyVerify` (Deno `mod.ts` and Tauri `guest-js/index.ts`)
   - `generateSecretKey` (Deno `mod.ts` and Tauri `guest-js/index.ts`)

   These are adapter-internal implementation details. They remain in `adapter.ts` /
   Tauri commands as private implementation. Replace with `SecretKey.generate()`.

### Phase 2 — Python API rename (no new functionality)

3. Add clean-named functions to `packages/iroh-http-py/src/lib.rs`:
   - `fn sign(secret_key: Vec<u8>, data: Vec<u8>) -> PyResult<Vec<u8>>` — thin
     wrapper calling the existing `secret_key_sign` logic
   - `fn verify(public_key: Vec<u8>, data: Vec<u8>, signature: Vec<u8>) -> PyResult<bool>` —
     thin wrapper calling the existing `public_key_verify` logic

   Keep `secret_key_sign` and `public_key_verify` registered in the module as
   **deprecated aliases** (emit `PyDeprecationWarning` via PyO3) until the next
   major version. Do not remove them in this issue.

   `generate_secret_key` stays unchanged — the name is already idiomatic.

4. Update `packages/iroh-http-py/iroh_http/__init__.pyi`:
   - Add `def sign(secret_key: bytes, data: bytes) -> bytes: ...`
   - Add `def verify(public_key: bytes, data: bytes, signature: bytes) -> bool: ...`
   - Mark `secret_key_sign` and `public_key_verify` stubs with a deprecation note.

### Phase 3 — Sealed-box encrypt/decrypt (JS)

5. Add `async encrypt(plaintext: Uint8Array): Promise<Uint8Array>` to `PublicKey` in
   `packages/iroh-http-shared/src/keys.ts`.

   Algorithm (sealed-box ECIES, pure WebCrypto + inline BigInt math):
   - Convert Ed25519 public key bytes → X25519 public key
     (Edwards-to-Montgomery: `u = (1+y)/(1−y) mod p`, inline BigInt)
   - Generate ephemeral X25519 keypair via `crypto.getRandomValues` + inline
     Curve25519 scalar multiplication (no external library)
   - ECDH: `sharedSecret = x25519(ephemPriv, recipientX25519Pub)`
   - KDF: `HKDF-SHA-256(ikm=sharedSecret, salt=ephemPub, info="iroh-http seal v1")`
   - Encrypt: AES-GCM-256, random 12-byte IV
   - Wire format: `[32B ephemPub] || [12B IV] || [ciphertext + 16B GCM tag]`
     (minimum output length: 60 bytes)

6. Add `async decrypt(ciphertext: Uint8Array): Promise<Uint8Array>` to `SecretKey`.

   Reverse of encrypt:
   - Parse wire format; throw `IrohArgumentError` if `ciphertext.length < 60`
   - Convert Ed25519 secret key → X25519 private key
     (`sha512(seed)[0..32]` with Curve25519 clamping)
   - ECDH + HKDF (identical parameters)
   - AES-GCM-256 decrypt; throw `IrohError` on authentication failure

   No new npm dependencies — uses only `crypto.subtle`.

### Phase 4 — Sealed-box encrypt/decrypt (Python)

7. Add two new `#[pyfunction]` entries in `packages/iroh-http-py/src/lib.rs`:
   - `fn encrypt(public_key: Vec<u8>, plaintext: Vec<u8>) -> PyResult<Vec<u8>>`
   - `fn decrypt(secret_key: Vec<u8>, ciphertext: Vec<u8>) -> PyResult<Vec<u8>>`

   Implement using the same algorithm as Phase 3 via Rust crates. Suggested
   dependencies: `x25519-dalek`, `aes-gcm`, `hkdf`, `sha2`. Verify workspace
   compatibility before adding. The wire format must be **byte-for-byte identical**
   to the JS implementation so messages sealed in Python can be opened in any
   JS adapter and vice versa.

   `decrypt` raises `PyValueError` on authentication failure and on input shorter
   than 60 bytes.

8. Update `packages/iroh-http-py/iroh_http/__init__.pyi`:
   - Add `def encrypt(public_key: bytes, plaintext: bytes) -> bytes: ...`
   - Add `def decrypt(secret_key: bytes, ciphertext: bytes) -> bytes: ...`

### Phase 5 — Documentation

9. Update `docs/recipes/sealed-messages.md` to show the 2-call version for all
   platforms. Move the manual ECIES construction to a "custom cipher suite" footnote.

10. `docs/features/sign-verify.md` updated in this session — verify it matches the
    final implementation after phases above land.

11. Update `docs/guidelines/python.md` to document `sign`/`verify`/`encrypt`/`decrypt`,
    the deprecation of the old names, and that `node.public_key` returns a base32
    string which must be decoded to bytes before passing to `verify` or `encrypt`.

## Acceptance criteria

**JS platforms (Node, Deno, Tauri):**
1. `import { PublicKey, SecretKey } from "@momics/iroh-http-deno"` compiles and
   `PublicKey.fromString(nodeId)` works at runtime. Same for Node and Tauri.
2. `secretKeySign`, `publicKeyVerify`, and `generateSecretKey` are no longer named
   exports of any adapter package — calling them is a TypeScript compile error.
3. `await node.publicKey.encrypt(data)` returns a `Uint8Array` with length ≥ 60.
4. `await node.secretKey.decrypt(ciphertext)` returns the original plaintext.
5. `node.secretKey.decrypt(tamperedCiphertext)` throws `IrohError`.
6. `node.secretKey.sign` and `node.publicKey.verify` still work (non-regression).
7. `npm run typecheck` passes.

**Python:**
8. `from iroh_http import sign, verify, encrypt, decrypt` works.
9. `sign(node.secret_key, data)` returns 64 bytes.
10. `verify(public_key_bytes, data, sig)` returns `True` for valid, `False` for
    invalid — does not raise on a bad signature.
11. `encrypt(public_key_bytes, plaintext)` returns bytes with length ≥ 60.
12. `decrypt(secret_key_bytes, ciphertext)` returns original plaintext.
13. `decrypt(secret_key_bytes, tampered)` raises `ValueError`.
14. `secret_key_sign(key, data)` still works but emits `DeprecationWarning`.
15. `pytest packages/iroh-http-py/tests/` passes.

**Cross-platform interop:**
16. A message encrypted with Python `encrypt(pub_key_bytes, msg)` can be decrypted
    with JS `await node.secretKey.decrypt(ciphertext)` and vice versa.

## Regression test

- `cases.json` case IDs: `reg-a-iss-050-sign-verify`, `reg-a-iss-050-encrypt-decrypt`
- Verified failing before fix: N/A (new features / missing exports / renames)
