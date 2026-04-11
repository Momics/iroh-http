---
status: done
refs: features/sign-verify.md
---

# Patch 25 — Sign / Verify Helpers on Key Types

Expose `sign`, `verify`, and `SecretKey.generate()` on the key types already
present in `iroh-http-shared`, as described in
[sign-verify.md](../features/sign-verify.md).

## Problem

`iroh::SecretKey` and `iroh::PublicKey` implement Ed25519 sign/verify in Rust,
but these operations are not exposed through the JS bindings. Higher-level
features (capability tokens, signed caching) require a byte-level signing
primitive and currently cannot be built without a separate Ed25519 dependency.

## Changes

### 1. Rust — `crates/iroh-http-core/src/bridge.rs`

Add three FFI functions:

```rust
/// Sign arbitrary bytes with a secret key. Returns a 64-byte signature.
pub fn secret_key_sign(secret_key_bytes: &[u8], data: &[u8]) -> Vec<u8> {
    let key = iroh::SecretKey::from_bytes(secret_key_bytes.try_into().unwrap());
    key.sign(data).to_bytes().to_vec()
}

/// Verify a signature. Returns true on success, false on failure.
pub fn public_key_verify(public_key_bytes: &[u8], data: &[u8], sig: &[u8]) -> bool {
    let Ok(key) = iroh::PublicKey::from_bytes(public_key_bytes.try_into().ok().as_ref()) else {
        return false;
    };
    let Ok(sig) = iroh::Signature::from_bytes(sig.try_into().ok().as_ref()) else {
        return false;
    };
    key.verify(data, &sig).is_ok()
}

/// Generate a new random secret key. Returns 32 raw bytes.
pub fn generate_secret_key() -> Vec<u8> {
    iroh::SecretKey::generate(rand::rngs::OsRng).to_bytes().to_vec()
}
```

All three are synchronous and infallible from the JS perspective.

### 2. TypeScript — `packages/iroh-http-shared/src/index.ts`

Extend the `SecretKey` and `PublicKey` interfaces:

```ts
interface SecretKey {
  /** The raw 32-byte key. */
  readonly bytes: Uint8Array;
  /** The corresponding public key. */
  readonly publicKey: PublicKey;
  /** Sign arbitrary bytes. Returns a 64-byte Ed25519 signature. */
  sign(data: Uint8Array): Uint8Array;
  /** Generate a fresh secret key without creating a node. */
  static generate(): SecretKey;
}

interface PublicKey {
  /** The raw 32-byte key. */
  readonly bytes: Uint8Array;
  /** Verify a 64-byte Ed25519 signature. Returns false on failure — does not throw. */
  verify(data: Uint8Array, signature: Uint8Array): boolean;
}
```

### 3. Platform adapters

Wire `secret_key_sign`, `public_key_verify`, and `generate_secret_key`
through each adapter:

- **Node.js napi**: `packages/iroh-http-node/src/` — add to the `SecretKey`
  and `PublicKey` napi class bindings.
- **Deno FFI**: `packages/iroh-http-deno/src/` — add FFI symbol declarations.
- **Tauri**: `packages/iroh-http-tauri/src/` — add Tauri command handlers.
- **Python**: `packages/iroh-http-py/src/` — add to the PyO3 `SecretKey` and
  `PublicKey` class bindings.

### 4. Tests

Add `sign_verify.rs` in `crates/iroh-http-core/tests/`:

```rust
#[test]
fn sign_verify_round_trip() {
    let key = iroh::SecretKey::generate(rand::rngs::OsRng);
    let data = b"hello world";
    let sig = key.sign(data);
    assert!(key.public_key().verify(data, &sig).is_ok());
}

#[test]
fn verify_rejects_bad_signature() {
    let key = iroh::SecretKey::generate(rand::rngs::OsRng);
    let data = b"hello world";
    let mut sig = key.sign(data).to_bytes();
    sig[0] ^= 0xFF;  // corrupt
    let sig = iroh::Signature::from_bytes(&sig).unwrap();
    assert!(key.public_key().verify(data, &sig).is_err());
}
```

## Files

- `crates/iroh-http-core/src/bridge.rs` — new FFI functions
- `packages/iroh-http-shared/src/index.ts` — extended key type signatures
- `packages/iroh-http-node/src/` — napi class methods
- `packages/iroh-http-deno/src/` — FFI bindings
- `packages/iroh-http-tauri/src/` — Tauri commands
- `packages/iroh-http-py/src/` — PyO3 class methods
- `crates/iroh-http-core/tests/sign_verify.rs` — integration tests

## Notes

- `iroh::SecretKey::sign` is `&self` (non-consuming). The key is not moved.
- The raw 32-byte representation of `SecretKey` is `SecretKey::to_bytes()`.
  Do not expose a human-readable form — secret key material must not be
  accidentally logged.
- `SecretKey.generate()` uses `rand::rngs::OsRng` (CSPRNG). No seeding API
  is provided.
