# Per-Peer Rate Limiting

iroh-http provides two complementary layers of rate control:

1. **`maxConnectionsPerPeer`** in `ServeOptions` — a Rust-level hard cap on
   simultaneous connections from any one peer, enforced before JavaScript runs.
   This is the DoS baseline.

2. **`rateLimit()` middleware** in `iroh-http-shared` — a token-bucket rate
   limiter implemented in TypeScript, composable with other middleware.

## `ServeOptions.maxConnectionsPerPeer`

```ts
node.serve({ maxConnectionsPerPeer: 3 }, handler);
```

When a peer exceeds the limit, the excess connection is **closed at the QUIC
level** — no JS overhead, no `Request` object created. The remote receives a
transport-level close rather than an HTTP response (the connection was never
fully upgraded to HTTP).

This is the only rate control that lives inside `ServeOptions`. Everything
else is middleware.

## `rateLimit()` middleware

```ts
import { rateLimit } from 'iroh-http-shared/middleware';

node.serve({}, rateLimit({
  requestsPerSecond: 10,
  burst: 20,
  forPeer: (nodeId) => {
    if (PREMIUM.has(nodeId)) return { requestsPerSecond: 100 };
    if (BLOCKLIST.has(nodeId)) return 'block';
    return null; // use default
  },
})(handler));
```

`rateLimit` reads the `iroh-node-id` header injected on every request and
maintains a per-peer token bucket in a `Map`. No native component — pure
TypeScript.

See [`RateLimitOptions` in the specification](../specification.md#rate-limiting) for the full type.

When a peer exceeds its limit, the middleware returns `429 Too Many Requests`
with a `Retry-After` header. A `'block'` decision returns `403 Forbidden`.
The handler is never called.

## Middleware composition

Middlewares are plain functions `(handler) => handler`, so they compose
directly:

```ts
import { compose, rateLimit, verifyToken } from 'iroh-http-shared/middleware';

node.serve({}, compose(
  rateLimit({ requestsPerSecond: 10 }),
  verifyToken(trustedKey),
  handler,
));
```

`compose` applies middlewares left-to-right (outermost first).

## Notes

- `forPeer` is synchronous. Pre-load any per-peer config into a `Map` or `Set`.
- `rateLimit` and `maxConnectionsPerPeer` are complementary: the hard cap
  prevents connection floods; the middleware manages request rate from connected
  peers.
- `maxConcurrency` (total in-flight requests, all peers) remains a separate
  `ServeOptions` field.

→ [Patch 24](../patches/24_patch.md)
