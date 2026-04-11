# Sign / Verify

`SecretKey` and `PublicKey` expose sign and verify operations directly. Combined
with the transport's peer authentication, these are the building blocks for
signed responses, capability tokens, and caching patterns.

## API

```ts
// Sign arbitrary bytes with a secret key:
const sig: Uint8Array = secretKey.sign(data);

// Verify a signature against a public key:
const ok: boolean = publicKey.verify(data, sig);

// Generate a fresh key without starting a node:
const key = SecretKey.generate();
```

Both `sign` and `verify` are synchronous — Ed25519 is fast and non-blocking.

## Types

Signatures are 64-byte `Uint8Array` values. `PublicKey.verify` returns
`boolean` — it does not throw on an invalid signature.

## See also

- [Capability tokens](packages/capability-tokens.md) — uses sign/verify for token issuance
- [Signed response caching](packages/caching.md) — uses sign/verify for cache validity

→ [Patch 25](../patches/25_patch.md)
