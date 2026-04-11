---
status: not-implemented
scope: separate package — iroh-http-auth
priority: medium
---

# Feature: Capability Token System

## What

A lightweight, signed capability token that controls access to resources served
by an iroh-http node. Tokens are issued and signed by any node using its
Ed25519 private key, and verifiable by any peer using the public key — no
central authority required.

## Why

iroh-http nodes can receive connections from any node that knows their public
key. On a public network this means anyone can connect. A capability token
system lets a server restrict which callers can access which resources, with
unforgeable credentials rooted in the same identity model the transport already
uses.

Because the issuer's identity is cryptographically guaranteed by the transport,
token verification is zero-round-trip — the server doesn't phone home to check
anything.

## Proposed Token Format

```
base64url( nodeId  ||  expiry_u64  ||  scope_utf8  ||  signature_64 )
```

| Field | Size | Notes |
|---|---|---|
| `nodeId` | 32 bytes | Public key of the issuing node |
| `expiry` | 8 bytes | Unix timestamp (seconds), big-endian u64. `0` = no expiry |
| `scope` | variable | UTF-8 path prefix or resource identifier, e.g. `/api/data` |
| `signature` | 64 bytes | Ed25519 signature over the preceding bytes |

The token is passed as a standard HTTP header:

```
Authorization: IrohToken <base64url-token>
```

This reuses the standard `Authorization` header and a custom scheme — idiomatic
HTTP, no new header names.

## Proposed API

### Issuing (in `iroh-http-auth`)

```ts
import { issueToken } from 'iroh-http-auth';

const token = issueToken(secretKey, {
  scope: '/api/data',
  expiresIn: 3600,  // seconds; omit for no expiry
});
// token: string — ready to set as Authorization header
```

### Verifying (middleware for iroh-http serve handlers)

```ts
import { verifyToken } from 'iroh-http-auth';

node.serve({}, async (req) => {
  const result = verifyToken(req.headers.get('authorization'), {
    issuer: trustedNodeId,   // PublicKey | string — whose signature to trust
    scope: req.url,
  });
  if (!result.ok) return new Response('Forbidden', { status: 403 });
  // ...
});
```

## Dependencies

- Requires `sign` / `verify` helpers on `SecretKey` / `PublicKey`
  (see `sign-verify.md`).
- The `iroh-http-auth` package depends on `iroh-http-shared` for key types only.
  It has no Rust component; pure TypeScript.

## Notes

- Tokens are intentionally simple — no revocation, no claims beyond scope and
  expiry. Revocation can be layered on top using a short expiry + reissuance
  pattern.
- Multi-scope tokens, delegated issuance (signing a token with a scoped
  sub-key), and token refresh are out of scope for the first version.
