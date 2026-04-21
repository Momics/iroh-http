"use strict";
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
Object.defineProperty(exports, "__esModule", { value: true });
exports.rateLimit = rateLimit;
exports.compose = compose;
function createBucket(requestsPerSecond, burst) {
    const rate = requestsPerSecond / 1000;
    return { tokens: burst, lastRefill: Date.now(), rate, capacity: burst };
}
/**
 * Attempt to consume one token.  Returns `true` if the request is allowed,
 * `false` if the bucket is empty.
 */
function consume(bucket) {
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
function retryAfterSeconds(bucket) {
    if (bucket.tokens >= 1)
        return 0;
    return Math.ceil((1 - bucket.tokens) / bucket.rate / 1000);
}
// ── rateLimit ─────────────────────────────────────────────────────────────────
/**
 * Token-bucket rate limiting middleware.
 *
 * Reads the `Peer-Id` header (injected by iroh-http on every request)
 * to identify the caller.  Maintains a per-peer token bucket in a `Map`.
 *
 * - Peers that exceed their limit receive **429 Too Many Requests** with a
 *   `Retry-After` header.
 * - Peers matched to `'block'` by `forPeer()` receive **403 Forbidden**.
 * - Peers matched to `'unlimited'` bypass all rate checks.
 */
function rateLimit(options) {
    const { requestsPerSecond, burst = requestsPerSecond, forPeer, } = options;
    if (requestsPerSecond <= 0) {
        throw new RangeError("rateLimit: requestsPerSecond must be > 0");
    }
    if (burst <= 0) {
        throw new RangeError("rateLimit: burst must be > 0");
    }
    const buckets = new Map();
    return (handler) => (req) => {
        const nodeId = req.headers.get("Peer-Id") ?? "";
        const peerConfig = forPeer ? forPeer(nodeId) : null;
        if (peerConfig === "block") {
            return new Response("Forbidden", { status: 403 });
        }
        if (peerConfig === "unlimited") {
            return handler(req);
        }
        // Resolve effective rate/burst for this peer.
        const effectiveRate = peerConfig != null
            ? peerConfig.requestsPerSecond
            : requestsPerSecond;
        const effectiveBurst = peerConfig != null
            ? (peerConfig.burst ?? effectiveRate)
            : burst;
        // Look up or create the bucket for this peer.
        // Use a composite key when per-peer config differs so peers with
        // different configs don't share buckets.
        const bucketKey = peerConfig != null
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
function compose(...middlewaresAndHandler) {
    if (middlewaresAndHandler.length === 0) {
        throw new TypeError("compose: at least one argument (the handler) is required");
    }
    const handler = middlewaresAndHandler[middlewaresAndHandler.length - 1];
    const middlewares = middlewaresAndHandler.slice(0, -1);
    return middlewares.reduceRight((next, mw) => mw(next), handler);
}
//# sourceMappingURL=middleware.js.map