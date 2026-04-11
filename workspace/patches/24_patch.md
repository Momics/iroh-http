---
status: pending
refs: features/rate-limiting.md
---

# Patch 24 — Per-Peer Rate Limiting

Two-layer rate limiting as described in
[rate-limiting.md](../features/rate-limiting.md):

1. **`ServeOptions.maxConnectionsPerPeer`** — Rust-level hard cap before JS runs.
2. **`rateLimit()` middleware** — pure TypeScript token-bucket, composable.

## Problem

Any node that knows a peer's public key can connect and hammer the serve loop.
The JS event loop must be protected before middleware gets a chance to run
(`maxConnectionsPerPeer`), and then sophisticated per-peer logic is best
expressed in TypeScript where it can be tested, composed, and maintained
without touching native code.

## Changes

### 1. Rust — `ServeOptions.maxConnectionsPerPeer`

**`crates/iroh-http-core/src/server.rs`** — track a `HashMap<PublicKey, u32>`
of active connection counts in the accept loop. On each new connection, check
before upgrading:

```rust
if let Some(limit) = options.max_connections_per_peer {
    let count = *active_per_peer.get(&peer_key).unwrap_or(&0);
    if count >= limit {
        // send 429, close connection
        return;
    }
}
```

No token bucket, no timing — just a counter. Decrement on disconnect.

**`NodeOptions` / `ServeOptions` — Rust:**

```rust
pub struct ServeOptions {
    pub max_concurrency: Option<u32>,          // existing
    pub max_connections_per_peer: Option<u32>, // new
}
```

**TypeScript:**

```ts
interface ServeOptions {
  maxConcurrency?: number;
  maxConnectionsPerPeer?: number;  // new
}
```

### 2. TypeScript — `rateLimit()` middleware

**New file: `packages/iroh-http-shared/src/middleware/rate-limit.ts`**

```ts
type Handler = (req: Request) => Response | Promise<Response>;
type RateConfig = { requestsPerSecond: number; burst?: number };

interface RateLimitOptions {
  requestsPerSecond: number;
  burst?: number;
  forPeer?: (nodeId: string) => RateConfig | 'unlimited' | 'block' | null | undefined;
}

interface TokenBucket {
  tokens: number;
  lastRefill: number;  // Date.now() ms
}

export function rateLimit(options: RateLimitOptions): (handler: Handler) => Handler {
  const buckets = new Map<string, TokenBucket>();
  const defaultConfig: RateConfig = {
    requestsPerSecond: options.requestsPerSecond,
    burst: options.burst ?? options.requestsPerSecond,
  };

  return (handler) => (req) => {
    const nodeId = req.headers.get('iroh-node-id') ?? '';
    const decision = options.forPeer?.(nodeId) ?? null;

    if (decision === 'block') {
      return new Response('Forbidden', { status: 403 });
    }
    if (decision === 'unlimited') {
      return handler(req);
    }

    const cfg = (decision as RateConfig | null) ?? defaultConfig;
    const now = Date.now();
    let bucket = buckets.get(nodeId);
    if (!bucket) {
      bucket = { tokens: cfg.burst ?? cfg.requestsPerSecond, lastRefill: now };
      buckets.set(nodeId, bucket);
    }

    const elapsed = (now - bucket.lastRefill) / 1000;
    bucket.tokens = Math.min(
      cfg.burst ?? cfg.requestsPerSecond,
      bucket.tokens + elapsed * cfg.requestsPerSecond,
    );
    bucket.lastRefill = now;

    if (bucket.tokens >= 1) {
      bucket.tokens -= 1;
      return handler(req);
    }

    const retryAfter = Math.ceil((1 - bucket.tokens) / cfg.requestsPerSecond);
    return new Response('Too Many Requests', {
      status: 429,
      headers: { 'Retry-After': String(retryAfter) },
    });
  };
}
```

### 3. `compose()` helper

**New file: `packages/iroh-http-shared/src/middleware/compose.ts`**

```ts
type Middleware = (handler: Handler) => Handler;

export function compose(...fns: [...Middleware[], Handler]): Handler {
  const handler = fns[fns.length - 1] as Handler;
  const middlewares = fns.slice(0, -1) as Middleware[];
  return middlewares.reduceRight((h, m) => m(h), handler);
}
```

### 4. Barrel export

```ts
// packages/iroh-http-shared/src/middleware/index.ts
export { rateLimit } from './rate-limit.ts';
export { compose } from './compose.ts';
```

## Files

- `crates/iroh-http-core/src/server.rs` — `maxConnectionsPerPeer` counter
- `packages/iroh-http-shared/src/index.ts` — `ServeOptions.maxConnectionsPerPeer`
- `packages/iroh-http-shared/src/middleware/rate-limit.ts` — new
- `packages/iroh-http-shared/src/middleware/compose.ts` — new
- `packages/iroh-http-shared/src/middleware/index.ts` — barrel export
- All four adapter packages — pass `maxConnectionsPerPeer` through `ServeOptions`

## Notes

- `rateLimit` has no native component — it reads `iroh-node-id`, which is
  already injected by the Rust layer on every request.
- The `compose` helper is also the right home for future middlewares:
  auth token verification, logging, CORS headers, etc.
- `rateLimit` and `maxConnectionsPerPeer` are complementary: the hard cap
  protects the event loop; the middleware manages request cadence for
  already-connected peers.



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
