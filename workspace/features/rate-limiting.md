# Per-Peer Rate Limiting

A token-bucket rate limiter applied per connected peer identity in the serve
accept loop. Each peer gets its own bucket keyed on its verified public key —
unforgeable at the transport level, surviving IP changes and relay hops.

Rate limiting is a `ServeOptions` concern — it applies to the serve handler,
not the node as a whole.

## API

```ts
node.serve({
  rateLimit: {
    /** Default rate applied to all peers not matched by forPeer. */
    requestsPerSecond: 10,
    /**
     * Maximum burst size. Defaults to requestsPerSecond.
     * A burst of 20 allows 20 requests at once even at a low average rate.
     */
    burst: 20,
    /**
     * Per-peer override. Return a rate config to replace the default,
     * 'unlimited' to exempt this peer, or 'block' to reject all requests.
     * Return null (or omit) to use the default rate.
     */
    forPeer: (nodeId: string) => RateConfig | 'unlimited' | 'block' | null,
  },
}, handler);
```

```ts
type RateConfig = {
  requestsPerSecond: number;
  burst?: number;
};
```

### Examples

```ts
// Tiered rates based on a known allowlist:
const PREMIUM = new Set(['abc123...', 'def456...']);

node.serve({
  rateLimit: {
    requestsPerSecond: 5,
    forPeer: (nodeId) => {
      if (PREMIUM.has(nodeId)) return { requestsPerSecond: 100 };
      return null; // use default
    },
  },
}, handler);

// Block specific peers:
const BLOCKLIST = new Set(['bad123...']);

node.serve({
  rateLimit: {
    requestsPerSecond: 10,
    forPeer: (nodeId) => BLOCKLIST.has(nodeId) ? 'block' : null,
  },
}, handler);

// Exempt internal nodes from rate limiting:
node.serve({
  rateLimit: {
    requestsPerSecond: 10,
    forPeer: (nodeId) => INTERNAL_NODES.has(nodeId) ? 'unlimited' : null,
  },
}, handler);
```

When a peer exceeds its rate limit, the server responds with `429 Too Many
Requests` and a `Retry-After` header. The connection is not dropped — only the
request is rejected.

## Notes

- `forPeer` is synchronous. Pre-load any per-peer config into a `Map` or
  `Set` before starting the serve loop.
- Rate limits are keyed on `PublicKey` regardless of how many connections a
  peer opens. Multiple connections from the same peer share one bucket.
- `rateLimit` and `maxConcurrency` are orthogonal: `rateLimit` rejects bursts;
  `maxConcurrency` caps total simultaneous in-flight requests.
- Persistent rate limit state across reconnections is a possible future
  extension.

→ [Patch 24](../patches/24_patch.md)
