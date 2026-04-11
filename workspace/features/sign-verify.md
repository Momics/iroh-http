---
status: not-implemented
scope: core — key types
priority: high
---

# Feature: Sign / Verify Helpers on Key Types

## What

Expose `sign` and `verify` operations as first-class methods on the `SecretKey`
and `PublicKey` types already present in `iroh-http-shared`.

## Why

The transport already authenticates the peer's identity cryptographically —
the connection _is_ proof of identity. But higher-level features (caching,
capability tokens, signed responses) need a byte-level primitive to attach
proofs to arbitrary data outside of a live connection.

The signing primitive is also the building block for every other
cryptographic feature in this ecosystem. Not exposing it forces consumers to
bring a separate Ed25519 library and manually wire it to the Iroh key bytes —
fragile and unnecessary.

## Proposed API

```ts
// SecretKey
sign(data: Uint8Array): Uint8Array
// PublicKey
verify(data: Uint8Array, signature: Uint8Array): boolean
```

Both are synchronous — Ed25519 sign/verify is fast and non-blocking by nature.
No `Promise` wrapper needed.

`SecretKey.generate()` should also be added here: a static factory that
generates a fresh key without creating a full node.

## Rust side

`iroh::SecretKey::sign(&self, msg: &[u8]) -> Signature` and
`iroh::PublicKey::verify(&self, msg: &[u8], sig: &Signature) -> Result<()>`
are already available in the `iroh` crate. Exposure through napi / FFI is
straightforward.

## Notes

- Signatures are 64-byte Ed25519 signatures. Expose as `Uint8Array`, not
  a custom type.
- `PublicKey.verify` returns `boolean` (not throws) to keep usage ergonomic
  in JS. The Rust `Err` case maps to `false`.
- This feature is a prerequisite for capability tokens (see `capability-tokens.md`)
  and the caching pattern (see `caching.md`).
