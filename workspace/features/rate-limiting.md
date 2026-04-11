---
status: not-implemented
scope: core — serve option
priority: medium
---

# Feature: Per-Peer Rate Limiting

## What

A token-bucket rate limiter applied per connected peer identity in the serve
accept loop. Each peer gets its own bucket; the bucket is keyed on the peer's
verified public key, which is unforgeable at the transport level.

## Why

In a P2P network, any node that knows your public key can connect and send
requests. Without a rate limit, a single misbehaving or compromised peer can
exhaust server resources — connection slots, CPU, body channel memory — and
crowd out legitimate peers.

Standard IP-based rate limiting is insufficient in a P2P context because peers
can change IP addresses. Keying on node identity is strictly stronger: the
key is cryptographically tied to a specific node, and rate limits survive IP
changes, NAT traversal, and relay hops.

## Proposed API

```ts
// In NodeOptions / ServeOptions:
rateLimit?: {
  /**
   * Maximum number of requests per second per peer.
   * Uses a token-bucket algorithm: `burst` tokens are available immediately;
   * they refill at `requestsPerSecond` per second.
   */
  requestsPerSecond: number;
  /**
   * Maximum burst size. Defaults to `requestsPerSecond`.
   * A burst of 10 allows 10 simultaneous requests even at a low average rate.
   */
  burst?: number;
}
```

When a peer exceeds its rate limit, the server responds with `429 Too Many
Requests` and a `Retry-After` header indicating when the next token will
be available. The connection is not dropped — only the request is rejected.

## Rust side

The token bucket lives in the server accept loop (`server.rs`), keyed on the
`PublicKey` of the incoming connection (available immediately on accept).

A simple thread-safe bucket map:

```rust
struct RateLimiter {
    buckets: Mutex<HashMap<PublicKey, TokenBucket>>,
    config: RateLimitConfig,
}
```

`TokenBucket` tracks `tokens: f64` and `last_refill: Instant`. On each
request, subtract one token; refill proportionally to elapsed time since last
check. If `tokens < 1.0`, reject.

No external crate is needed; the algorithm is a dozen lines of Rust.

## Notes

- Rate limits are **per connection** in the current pool design (one connection
  per node). If the pool allows multiple connections per node in the future,
  the bucket key must remain `PublicKey`, not connection handle.
- Limits apply only to the serve path. The `fetch` (client) path is not
  rate-limited — the remote server controls that.
- Per-peer limits are separate from the existing `max_concurrency` setting,
  which caps total concurrent in-flight requests regardless of source.
- Persistent rate limit state across reconnections (e.g. to punish a peer that
  kept hammering and reconnected) is a possible future extension but out of
  scope here.
