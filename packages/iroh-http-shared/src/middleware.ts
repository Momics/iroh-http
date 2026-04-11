/**
 * Middleware utilities for iroh-http request handlers.
 *
 * @example
 * ```ts
 * import { rateLimit, compose } from 'iroh-http-shared/middleware';
 *
 * node.serve(compose(
 *   rateLimit({ requestsPerSecond: 10, burst: 20 }),
 *   myHandler,
 * ));
 * ```
 */

import type { ServeHandler } from "./serve.js";

// ── Types ─────────────────────────────────────────────────────────────────────

/** A rate limit configuration per peer. */
export type RateConfig = { requestsPerSecond: number; burst?: number };

/** Options for the {@link rateLimit} middleware. */
export interface RateLimitOptions {
  /**
   * Default maximum requests per second for all peers.
   * Must be a positive number.
   */
  requestsPerSecond: number;
  /**
   * Maximum burst size (initial and maximum token count).
   * Defaults to `requestsPerSecond` if not specified.
   */
  burst?: number;
  /**
   * Per-peer override.  Called once per request with the peer's node ID.
   * Return:
   *  - a `RateConfig` to override the defaults for that peer
   *  - `'unlimited'` to exempt the peer entirely
   *  - `'block'` to reject the peer with 403 regardless of rate
   *  - `null` / `undefined` to use the default config
   */
  forPeer?: (
    nodeId: string,
  ) => RateConfig | "unlimited" | "block" | null | undefined;
}

/** A middleware: wraps a handler and returns a new handler. */
export type Middleware = (handler: ServeHandler) => ServeHandler;

// ── Token bucket implementation ───────────────────────────────────────────────

interface Bucket {
  tokens: number;
  lastRefill: number; // ms timestamp
  rate: number;       // tokens per ms
  capacity: number;   // max tokens
}

function createBucket(requestsPerSecond: number, burst: number): Bucket {
  const rate = requestsPerSecond / 1000;
  return { tokens: burst, lastRefill: Date.now(), rate, capacity: burst };
}

/**
 * Attempt to consume one token.  Returns `true` if the request is allowed,
 * `false` if the bucket is empty.
 */
function consume(bucket: Bucket): boolean {
  const now = Date.now();
  const elapsed = now - bucket.lastRefill;
  bucket.tokens = Math.min(bucket.capacity, bucket.tokens + elapsed * bucket.rate);
  bucket.lastRefill = now;

  if (bucket.tokens >= 1) {
    bucket.tokens -= 1;
    return true;
  }
  return false;
}

/** Seconds until at least one token is available. */
function retryAfterSeconds(bucket: Bucket): number {
  if (bucket.tokens >= 1) return 0;
  return Math.ceil((1 - bucket.tokens) / bucket.rate / 1000);
}

// ── rateLimit ─────────────────────────────────────────────────────────────────

/**
 * Token-bucket rate limiting middleware.
 *
 * Reads the `iroh-node-id` header (injected by iroh-http on every request)
 * to identify the caller.  Maintains a per-peer token bucket in a `Map`.
 *
 * - Peers that exceed their limit receive **429 Too Many Requests** with a
 *   `Retry-After` header.
 * - Peers matched to `'block'` by `forPeer()` receive **403 Forbidden**.
 * - Peers matched to `'unlimited'` bypass all rate checks.
 */
export function rateLimit(options: RateLimitOptions): Middleware {
  const {
    requestsPerSecond,
    burst = requestsPerSecond,
    forPeer,
  } = options;

  if (requestsPerSecond <= 0) {
    throw new RangeError("rateLimit: requestsPerSecond must be > 0");
  }
  if (burst <= 0) {
    throw new RangeError("rateLimit: burst must be > 0");
  }

  const buckets = new Map<string, Bucket>();

  return (handler: ServeHandler): ServeHandler =>
    (req: Request): Response | Promise<Response> => {
      const nodeId = req.headers.get("iroh-node-id") ?? "";

      const peerConfig = forPeer ? forPeer(nodeId) : null;

      if (peerConfig === "block") {
        return new Response("Forbidden", { status: 403 });
      }

      if (peerConfig === "unlimited") {
        return handler(req);
      }

      // Resolve effective rate/burst for this peer.
      const effectiveRate =
        peerConfig != null ? peerConfig.requestsPerSecond : requestsPerSecond;
      const effectiveBurst =
        peerConfig != null ? (peerConfig.burst ?? effectiveRate) : burst;

      // Look up or create the bucket for this peer.
      // Use a composite key when per-peer config differs so peers with
      // different configs don't share buckets.
      const bucketKey =
        peerConfig != null
          ? `${nodeId}:${effectiveRate}:${effectiveBurst}`
          : nodeId;

      let bucket = buckets.get(bucketKey);
      if (!bucket) {
        bucket = createBucket(effectiveRate, effectiveBurst);
        buckets.set(bucketKey, bucket);
      }

      if (!consume(bucket)) {
        const retryAfter = retryAfterSeconds(bucket);
        return new Response("Too Many Requests", {
          status: 429,
          headers: { "Retry-After": String(retryAfter) },
        });
      }

      return handler(req);
    };
}

// ── compose ───────────────────────────────────────────────────────────────────

/**
 * Compose multiple middleware functions and a final handler into a single
 * `ServeHandler`.  Middlewares execute in left-to-right order (outermost first).
 *
 * @example
 * ```ts
 * node.serve(compose(
 *   rateLimit({ requestsPerSecond: 10 }),
 *   authMiddleware,
 *   myHandler,
 * ));
 * ```
 *
 * @param middlewaresAndHandler - Any number of `Middleware` functions followed
 *   by a final `ServeHandler`.  The last argument must be a `ServeHandler`.
 */
export function compose(
  ...middlewaresAndHandler: [...Middleware[], ServeHandler]
): ServeHandler {
  if (middlewaresAndHandler.length === 0) {
    throw new TypeError("compose: at least one argument (the handler) is required");
  }
  const handler = middlewaresAndHandler[middlewaresAndHandler.length - 1] as ServeHandler;
  const middlewares = middlewaresAndHandler.slice(0, -1) as Middleware[];

  return middlewares.reduceRight(
    (next, mw) => mw(next),
    handler,
  );
}
