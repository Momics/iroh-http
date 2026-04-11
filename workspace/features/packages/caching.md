# Signed Response Caching

> **Ecosystem package** (`iroh-http-cache`) — not part of iroh-http core.
> Built on top of the [sign/verify primitives](../sign-verify.md) that core provides.

A caching pattern — and optional middleware package — that makes cache
invalidation tractable in a P2P context by anchoring cache validity to the
sender's cryptographic identity rather than a trusted CDN. Because the
sender's identity is unforgeable at the transport level:

- A receiver can cache a response and later revalidate it against the original
  node with cryptographic certainty that the content hasn't been tampered with.
- A node can sign its response bodies. Peers store the signature alongside the
  cached bytes and verify authenticity before serving from cache.
- Cache poisoning is impossible: a malicious intermediary cannot produce a
  valid signature for a resource it did not originate.

## Design

No core API changes are needed. The pattern composes existing primitives:

1. **Standard HTTP caching headers** (`ETag`, `Cache-Control`, `Last-Modified`)
   work as-is on iroh-http responses.

2. **Signed ETags** — an optional convention where the `ETag` value is the
   base64url-encoded Ed25519 signature of the response body:

   ```
   ETag: "<base64url-signature>"
   ```

   A receiver caches `(body, etag)`. On revalidation it sends the standard
   `If-None-Match: "<etag>"` header. The origin node checks whether the current
   body produces the same signature; if so, responds `304 Not Modified`.

3. **Middleware** (`iroh-http-cache`) wraps a serve handler and:
   - On outbound responses: signs the body and injects `ETag`.
   - On inbound `If-None-Match` requests: verifies the cached ETag against
     the current body before deciding whether to serve `304`.

## Sketch of Middleware API

```ts
import { signedCache } from 'iroh-http-cache';

node.serve({}, signedCache(secretKey, async (req) => {
  return new Response(await getData(), { headers: { 'Cache-Control': 'max-age=300' } });
}));
```

## Dependencies

- Requires `sign` / `verify` helpers on `SecretKey` / `PublicKey`
  (see `sign-verify.md`).
- `iroh-http-cache` would be a pure TypeScript package with no native component.

## Notes

- This is a **pattern first**. Document the convention clearly before building
  the middleware package — many use cases can implement it in a few lines of
  handler code without an additional dependency.
- Large streaming bodies cannot be signed before sending (the signature would
  require buffering). For streaming responses, sign a hash of the body instead
  and include it as a trailer.
- This does not address cache storage — storage is entirely the caller's
  concern (in-memory `Map`, `localStorage`, a proper cache API).

Part of the `iroh-http-cache` package.

→ Requires [sign/verify helpers](../sign-verify.md) and
  [Patch 25](../../patches/25_patch.md).
