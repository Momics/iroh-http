/**
 * `makeFetch` — wraps the raw platform fetch in the web-standard signature.
 * `makeConnect` — wraps the raw platform connect in a `BidirectionalStream`.
 *
 * ```ts
 * const nodeFetch = makeFetch(bridge, endpointHandle, rawFetch, allocBodyWriter);
 * const res = await nodeFetch(remotePeerId, '/api/data');
 *
 * const stream = await makeConnect(bridge, endpointHandle, rawConnect)(peerId, '/ws');
 * ```
 */
import type { AllocBodyWriterFn, BidirectionalStream, Bridge, IrohFetchInit, RawConnectFn, RawFetchFn } from "./bridge.js";
import type { PublicKey } from "./keys.js";
export type FetchFn = {
    /** Web-standard form: peer identity is embedded in the `httpi://` URL hostname. */
    (input: string | URL, init?: IrohFetchInit): Promise<Response>;
    /** Legacy two-argument form: peer and path supplied separately. */
    (peer: PublicKey | string, input: string | URL, init?: IrohFetchInit): Promise<Response>;
};
/**
 * Construct a `fetch`-like function bound to a specific `IrohEndpoint`.
 *
 * Supports `AbortSignal` via `init.signal` (§3).
 *
 * @param bridge          Platform bridge implementation (nextChunk, sendChunk, etc.).
 * @param endpointHandle  Slab handle returned by the low-level bind.
 * @param rawFetch        Platform-specific raw fetch function.
 * @param allocBodyWriter Allocates a `BodyWriter` handle for request body streaming.
 * @returns A `fetch`-like function: `(peer, url, init?) => Promise<Response>`.
 *
 * @example
 * ```ts
 * const doFetch = makeFetch(bridge, handle, rawFetch, allocBodyWriter);
 * const res = await doFetch(peerId, '/api/data', { method: 'POST', body: 'hi' });
 * console.log(await res.text());
 * ```
 */
export declare function makeFetch(bridge: Bridge, endpointHandle: number, rawFetch: RawFetchFn, allocBodyWriter: AllocBodyWriterFn): FetchFn;
/**
 * Construct a `createBidirectionalStream`-like function that opens a full-duplex stream.
 *
 * The returned `BidirectionalStream` exposes `readable` (data from server) and
 * `writable` (data to server).  Both sides are open simultaneously.
 *
 * @param bridge          Platform bridge implementation.
 * @param endpointHandle  Slab handle returned by the low-level bind.
 * @param rawConnect      Platform-specific raw duplex connect function.
 * @returns A function: `(peer, path, init?) => Promise<BidirectionalStream>`.
 *
 * @throws {@link IrohConnectError} If the remote peer rejects or is unreachable.
 *
 * @example
 * ```ts
 * const connect = makeConnect(bridge, handle, rawConnect);
 * const { readable, writable } = await connect(peerId, '/ws');
 * const writer = writable.getWriter();
 * await writer.write(new TextEncoder().encode('ping'));
 * ```
 */
export declare function makeConnect(bridge: Bridge, endpointHandle: number, rawConnect: RawConnectFn): (peer: PublicKey | string, path: string, init?: RequestInit) => Promise<BidirectionalStream>;
//# sourceMappingURL=fetch.d.ts.map