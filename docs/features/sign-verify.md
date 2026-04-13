# Sign / Verify

`SecretKey` and `PublicKey` expose sign and verify operations directly. Combined
with the transport's peer authentication, these are the building blocks for
signed responses, capability tokens, and caching patterns.

## API

```ts
// Sign arbitrary bytes with a secret key:
const sig: Uint8Array = await secretKey.sign(data);

// Verify a signature against a public key:
const ok: boolean = await publicKey.verify(data, sig);

// Generate a fresh key without starting a node:
const key = SecretKey.generate();
```

Both `sign` and `verify` are **async** — they use the WebCrypto subtle API
which returns `Promise` values. Always `await` them.

## Types

Signatures are 64-byte `Uint8Array` values. `PublicKey.verify` returns
`boolean` — it does not throw on an invalid signature.

## See also

- [Recipes index](../recipes/index.md) — sealed messages, capability tokens, and other sign/verify patterns
