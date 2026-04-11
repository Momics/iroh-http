---
status: pending
refs: features/rate-limiting.md
---

# Patch 24 — Per-Peer Rate Limiting

Add a token-bucket rate limiter in the serve accept loop, as described in
[rate-limiting.md](../features/rate-limiting.md).

Rate limiting lives in `ServeOptions`, not `NodeOptions`, because it is a
server-side concern and different serve handlers on the same node may have
different limits.

## Problem

Any node that knows a peer's public key can connect and send unlimited requests.
IP-based rate limiting is insufficient because peers can change addresses.
Identity-based keying — on `PublicKey`, not IP — survives relay hops and
address changes.

## Changes

### 1. Rust — `crates/iroh-http-core/src/`

**New file: `rate_limit.rs`**

```rust
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use iroh::PublicKey;

pub struct RateConfig {
    pub requests_per_second: f64,
    pub burst: f64,
}

// Serialisable decision returned to the JS layer for forPeer overrides.
pub enum PeerRateDecision {
    Config(RateConfig),
    Unlimited,
    Block,
    Default,
}

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(burst: f64) -> Self {
        Self { tokens: burst, last_refill: Instant::now() }
    }

    fn try_consume(&mut self, cfg: &RateConfig) -> bool {
        let elapsed = self.last_refill.elapsed().as_secs_f64();
        self.tokens = (self.tokens + elapsed * cfg.requests_per_second).min(cfg.burst);
        self.last_refill = Instant::now();
        if self.tokens >= 1.0 { self.tokens -= 1.0; true } else { false }
    }

    fn next_token_secs(&self, cfg: &RateConfig) -> f64 {
        (1.0 - self.tokens) / cfg.requests_per_second
    }
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<PublicKey, TokenBucket>>,
    default: RateConfig,
}

impl RateLimiter {
    pub fn new(default: RateConfig) -> Self {
        Self { buckets: Mutex::new(HashMap::new()), default }
    }

    /// Returns Ok(()) or Err(retry_after_secs).
    /// `peer_config` is the JS-resolved per-peer decision.
    pub fn check(&self, peer: &PublicKey, peer_decision: PeerRateDecision) -> Result<(), f64> {
        match peer_decision {
            PeerRateDecision::Unlimited => return Ok(()),
            PeerRateDecision::Block => return Err(f64::INFINITY),
            _ => {}
        }
        let cfg = match peer_decision {
            PeerRateDecision::Config(c) => c,
            _ => &self.default,  // Default
        };
        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets.entry(*peer).or_insert_with(|| TokenBucket::new(cfg.burst));
        if bucket.try_consume(cfg) { Ok(()) } else { Err(bucket.next_token_secs(cfg)) }
    }
}
```

**`server.rs`** — before dispatching each request:

1. Call the JS `forPeer` callback (via bridge) with the peer's node ID to get
   the `PeerRateDecision`.
2. Call `rate_limiter.check(peer, decision)`.
3. On `Err(retry_after)`, return:
   ```
   HTTP/1.1 429 Too Many Requests
   Retry-After: <ceil(retry_after) as integer>
   Content-Length: 0
   ```

When `retry_after` is `INFINITY` (blocked peer), return `403 Forbidden` instead
of `429` — the peer is not expected to retry.

### 2. TypeScript — `packages/iroh-http-shared/src/index.ts`

```ts
interface ServeOptions {
  rateLimit?: {
    /** Default rate applied to all peers. */
    requestsPerSecond: number;
    /** Maximum burst. Defaults to requestsPerSecond. */
    burst?: number;
    /**
     * Per-peer override. Called once per request with the peer's node ID.
     * Return a RateConfig to override the default, 'unlimited' to exempt,
     * or 'block' to reject with 403. Return null to use the default.
     */
    forPeer?: (nodeId: string) => RateConfig | 'unlimited' | 'block' | null | undefined;
  };
}

type RateConfig = { requestsPerSecond: number; burst?: number };
```

The `forPeer` callback is synchronous. Pre-load any per-peer config into a
`Map` or `Set` before calling `node.serve`.

### 3. Platform adapters

Pass `rateLimit.requestsPerSecond`, `rateLimit.burst`, and a JS callback handle
for `forPeer` from `ServeOptions` to the Rust serve loop. The Rust side calls
back into JS to resolve per-peer decisions.

Remove `rateLimit` from `NodeOptions` if it was previously added there.

## Files

- `crates/iroh-http-core/src/rate_limit.rs` — new file
- `crates/iroh-http-core/src/server.rs` — call rate limiter on each request
- `packages/iroh-http-shared/src/index.ts` — `ServeOptions.rateLimit` + `RateConfig` type
- All four adapter packages — pass `ServeOptions.rateLimit` through to Rust

## Notes

- Buckets are keyed on `PublicKey`. Multiple connections from the same peer
  share one bucket.
- Bucket eviction: a periodic sweep removes entries where `tokens >= burst`
  (fully refilled; no recent activity from that peer).
- `rateLimit` and `maxConcurrency` are orthogonal. Use both for defence in depth.


Add a token-bucket rate limiter keyed on peer `PublicKey` in the serve accept
loop, as described in [rate-limiting.md](../features/rate-limiting.md).

## Problem

Any node knowing a peer's public key can connect and send unlimited requests.
A single misbehaving peer can exhaust server resources and crowd out legitimate
traffic. IP-based rate limiting is insufficient because peers can change
addresses; identity-based keying is required.

## Changes

### 1. Rust — `crates/iroh-http-core/src/`

**New file: `rate_limit.rs`**

```rust
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use iroh::PublicKey;

pub struct RateLimitConfig {
    pub requests_per_second: f64,
    pub burst: f64,
}

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(burst: f64) -> Self {
        Self { tokens: burst, last_refill: Instant::now() }
    }

    /// Returns true if a token was consumed; false if rate limited.
    fn try_consume(&mut self, cfg: &RateLimitConfig) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * cfg.requests_per_second).min(cfg.burst);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn next_token_secs(&self, cfg: &RateLimitConfig) -> f64 {
        (1.0 - self.tokens) / cfg.requests_per_second
    }
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<PublicKey, TokenBucket>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(cfg: RateLimitConfig) -> Self {
        Self { buckets: Mutex::new(HashMap::new()), config: cfg }
    }

    /// Returns Ok(()) or Err(retry_after_secs).
    pub fn check(&self, peer: &PublicKey) -> Result<(), f64> {
        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets.entry(*peer).or_insert_with(|| TokenBucket::new(self.config.burst));
        if bucket.try_consume(&self.config) {
            Ok(())
        } else {
            Err(bucket.next_token_secs(&self.config))
        }
    }
}
```

**`server.rs`** — call `rate_limiter.check(&peer_public_key)` at the top of
the request handler. On `Err(retry_after)`, return:

```
HTTP/1.1 429 Too Many Requests
Retry-After: <ceil(retry_after) as integer seconds>
Content-Length: 0
```

No body, no further processing.

### 2. `NodeOptions` — Rust

```rust
pub struct NodeOptions {
    // ... existing fields ...
    pub rate_limit: Option<RateLimitConfig>,
}
```

`RateLimitConfig` is `None` by default (no rate limiting).

### 3. TypeScript — `packages/iroh-http-shared/src/index.ts`

```ts
interface NodeOptions {
  // ... existing fields ...
  rateLimit?: {
    /** Maximum requests per second per peer. */
    requestsPerSecond: number;
    /**
     * Maximum burst size. Defaults to requestsPerSecond.
     * Allows short bursts above the average rate.
     */
    burst?: number;
  };
}
```

### 4. Platform adapters

Pass `rate_limit` from JS `NodeOptions` to Rust `NodeOptions` in each adapter's
`createNode` / node creation path.

## Files

- `crates/iroh-http-core/src/rate_limit.rs` — new file
- `crates/iroh-http-core/src/server.rs` — call rate limiter on each request
- `crates/iroh-http-core/src/lib.rs` — expose `RateLimitConfig` in `NodeOptions`
- `packages/iroh-http-shared/src/index.ts` — `ServeOptions.rateLimit` + `RateConfig` type
- All four adapter packages — pass config through `createNode`

## Notes

- The `RateLimiter` is created once per node and held in the server state.
- Bucket entries for disconnected peers are not evicted immediately; a periodic
  cleanup pass (e.g. every 60 s) removes entries where `tokens == burst`
  (i.e. fully refilled, no recent traffic).
- `max_concurrency` and `rateLimit` are orthogonal: the rate limiter rejects
  request bursts; `max_concurrency` caps total simultaneous in-flight requests.
