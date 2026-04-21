/**
 * `makeFetch` — wraps the raw platform fetch in the web-standard signature.
 * `makeConnect` — wraps the raw platform connect in a `BidirectionalStream`.
 *
 * ```ts
 * const nodeFetch = makeFetch(adapter, endpointHandle);
 * const res = await nodeFetch(remotePeerId, '/api/data');
 *
 * const stream = await makeConnect(adapter, endpointHandle)(peerId, '/ws');
 * ```
 */
import type { BidirectionalStream, IrohAdapter, IrohFetchInit } from "./IrohAdapter.js";
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
 * @param adapter         Platform adapter implementation (nextChunk, sendChunk, etc.).
 * @param endpointHandle  Slab handle returned by the low-level bind.
 * @returns A `fetch`-like function: `(peer, url, init?) => Promise<Response>`.
 *
 * @example
 * ```ts
 * const doFetch = makeFetch(adapter, handle);
 * const res = await doFetch(peerId, '/api/data', { method: 'POST', body: 'hi' });
 * console.log(await res.text());
 * ```
 */
export declare function makeFetch(adapter: IrohAdapter, endpointHandle: number): FetchFn;
/**
 * Construct a `createBidirectionalStream`-like function that opens a full-duplex stream.
 *
 * The returned `BidirectionalStream` exposes `readable` (data from server) and
 * `writable` (data to server).  Both sides are open simultaneously.
 *
 * @param adapter         Platform adapter implementation.
 * @param endpointHandle  Slab handle returned by the low-level bind.
 * @returns A function: `(peer, path, init?) => Promise<BidirectionalStream>`.
 *
 * @throws {@link IrohConnectError} If the remote peer rejects or is unreachable.
 *
 * @example
 * ```ts
 * const connect = makeConnect(adapter, handle);
 * const { readable, writable } = await connect(peerId, '/ws');
 * const writer = writable.getWriter();
 * await writer.write(new TextEncoder().encode('ping'));
 * ```
 */
export declare function makeConnect(adapter: IrohAdapter, endpointHandle: number): (peer: PublicKey | string, path: string, init?: RequestInit) => Promise<BidirectionalStream>;
//# sourceMappingURL=fetch.d.ts.map