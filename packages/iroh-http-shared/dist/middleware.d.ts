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
/** A rate limit configuration per peer. */
export type RateConfig = {
    requestsPerSecond: number;
    burst?: number;
};
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
    forPeer?: (nodeId: string) => RateConfig | "unlimited" | "block" | null | undefined;
}
/** A middleware: wraps a handler and returns a new handler. */
export type Middleware = (handler: ServeHandler) => ServeHandler;
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
export declare function rateLimit(options: RateLimitOptions): Middleware;
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
export declare function compose(...middlewaresAndHandler: [...Middleware[], ServeHandler]): ServeHandler;
//# sourceMappingURL=middleware.d.ts.map