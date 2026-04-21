/**
 * `makeServe` — wraps the raw platform serve in a Deno-compatible signature.
 *
 * ```ts
 * const serve = makeServe(bridge, handle, rawServe, nodeId, finished, stopServe);
 * const server = serve(async (req) => Response.json({ ok: true }));
 * await server.finished;
 * ```
 */
import type { IrohAdapter } from "./IrohAdapter.js";
/**
 * A request handler that receives a web-standard `Request` and returns a `Response`.
 *
 * The `Request` is augmented with:
 * - `req.headers.get('Peer-Id')` — the authenticated peer's public key.
 * - `req.acceptWebTransport()` — (duplex only) returns `{ readable, writable }`.
 *
 * ## Security
 *
 * `serve()` opens a **public endpoint** on the Iroh overlay network. Unlike
 * regular HTTP (where binding on localhost keeps you private), any peer that
 * knows or discovers your node's public key can connect and send requests.
 * Iroh QUIC authenticates the peer's *identity*, but not *authorization*.
 *
 * Always check `Peer-Id` and reject requests from untrusted peers:
 *
 * ```ts
 * const ALLOWED_PEERS = new Set(["<peer-public-key>"]);
 * node.serve({}, (req) => {
 *   const peerId = req.headers.get("Peer-Id");
 *   if (!ALLOWED_PEERS.has(peerId)) return new Response("Forbidden", { status: 403 });
 *   return new Response("ok");
 * });
 * ```
 */
export type ServeHandler = (req: Request) => Response | Promise<Response>;
/**
 * Options for the `serve()` call.
 *
 * All fields are optional.  The handler can be passed here (single-argument
 * form) or as a separate second argument.
 */
export interface ServeOptions {
    /**
     * Called once when the serve loop is ready to accept connections.
     *
     * Iroh binds during `createNode`, not during `serve`, so the loop is
     * immediately live after `serve()` returns.
     */
    onListen?: (info: {
        nodeId: string;
    }) => void;
    /**
     * Called when a request handler throws or rejects.
     *
     * The returned `Response` is sent to the client.  If this callback also
     * throws, the request receives a bare `500 Internal Server Error`.
     *
     * @default Returns `500 Internal Server Error` with no body.
     */
    onError?: (error: unknown) => Response | Promise<Response>;
    /**
     * When the signal is aborted, the serve loop stops accepting new
     * connections and drains in-flight requests (graceful shutdown).
     *
     * This only stops the serve loop — the node itself stays alive.
     */
    signal?: AbortSignal;
    /**
     * Called when a peer establishes its first QUIC connection to this node
     * (0 → 1 connection count transition).
     *
     * @param peerId Base32-encoded public key of the peer.
     */
    onPeerConnect?: (peerId: string) => void;
    /**
     * Called when a peer's last QUIC connection to this node closes
     * (1 → 0 connection count transition).
     *
     * @param peerId Base32-encoded public key of the peer.
     */
    onPeerDisconnect?: (peerId: string) => void;
    /**
     * Inline handler — allows the single-argument `serve({ handler })` form.
     * Mutually exclusive with passing `handler` as the second argument.
     */
    handler?: ServeHandler;
}
/**
 * Handle returned by `serve()`.
 */
export interface ServeHandle {
    /**
     * Resolves when the serve loop terminates — either because `node.close()`
     * was called, `signal` was aborted, or a fatal error occurred.
     */
    readonly finished: Promise<void>;
}
/**
 * Three overloaded call signatures for `serve()`:
 *
 * 1. `serve(handler)` — handler only (most common).
 * 2. `serve(options, handler)` — options + handler.
 * 3. `serve(optionsWithHandler)` — handler inside options object.
 */
export type ServeFn = {
    (handler: ServeHandler): ServeHandle;
    (options: ServeOptions, handler: ServeHandler): ServeHandle;
    (options: ServeOptions & {
        handler: ServeHandler;
    }): ServeHandle;
};
/**
 * Construct a Deno-compatible `serve` function bound to a specific endpoint.
 *
 * @param bridge          Platform bridge implementation (sendChunk, finishBody, etc.).
 * @param endpointHandle  Slab handle returned by the low-level bind.
 * @param rawServe        Platform-specific raw serve function.
 * @param nodeId          The node's base32 public key string.
 * @param finished        Promise that resolves when the serve loop terminates.
 * @param stopServe       Calls the platform's stopServe FFI to gracefully shut down.
 * @returns A `serve` function with three overloaded call signatures.
 *
 * @example
 * ```ts
 * const server = serve(async (req) => {
 *   const peer = req.headers.get('Peer-Id');
 *   return Response.json({ echo: await req.text(), peer });
 * });
 * await server.finished;
 * ```
 */
export declare function makeServe(adapter: IrohAdapter, endpointHandle: number, nodeId: string, onNodeClose: Promise<void>): ServeFn;
//# sourceMappingURL=serve.d.ts.map