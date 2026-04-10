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

import type { Bridge, RawFetchFn, AllocBodyWriterFn, RawConnectFn, BidirectionalStream } from "./bridge.js";
import { makeReadable, pipeToWriter, bodyInitToStream } from "./streams.js";
import type { PublicKey } from "./keys.js";
import { resolveNodeId } from "./keys.js";
import { classifyError } from "./errors.js";

export type FetchFn = (
  peer: PublicKey | string,
  input: string | URL,
  init?: RequestInit
) => Promise<Response>;

/**
 * Construct a `fetch`-like function bound to a specific `IrohEndpoint`.
 *
 * Supports `AbortSignal` via `init.signal` (§3) and populates the
 * non-standard `res.trailers` promise with response trailer headers (§4).
 */
export function makeFetch(
  bridge: Bridge,
  endpointHandle: number,
  rawFetch: RawFetchFn,
  allocBodyWriter: AllocBodyWriterFn
): FetchFn {
  return async (
    peer: PublicKey | string,
    input: string | URL,
    init?: RequestInit
  ): Promise<Response> => {
    const nodeId = resolveNodeId(peer);
    const url = typeof input === "string" ? input : input.toString();
    const method = init?.method ?? "GET";
    const signal = init?.signal ?? null;

    // Reject immediately if already aborted.
    if (signal?.aborted) {
      throw new DOMException("The operation was aborted", "AbortError");
    }

    const headers: [string, string][] = normaliseHeaders(init?.headers);

    // Allocate request body writer if needed.
    let reqBodyHandle: number | null = null;
    let bodyPipePromise: Promise<void> | null = null;
    const bodyStream = init?.body ? bodyInitToStream(init.body) : null;
    if (bodyStream) {
      reqBodyHandle = await allocBodyWriter();
      bodyPipePromise = pipeToWriter(bridge, bodyStream, reqBodyHandle);
    }

    // Allocate a Rust-side cancellation token so that AbortSignal can cancel
    // the transport even before the response head arrives (§3 enhanced).
    const fetchToken = await bridge.allocFetchToken();

    // Wire AbortSignal → cancelFetch as early as possible (fire-and-forget).
    // This fires even if the signal is already aborted.
    let cancelAbortListener: (() => void) | null = null;
    if (signal) {
      if (signal.aborted) {
        bridge.cancelFetch(fetchToken);
        throw new DOMException("The operation was aborted", "AbortError");
      }
      cancelAbortListener = () => bridge.cancelFetch(fetchToken);
      signal.addEventListener("abort", cancelAbortListener, { once: true });
    }

    // Build an abort promise for the JS-side race (still needed so the Promise
    // rejects immediately while the Rust cancel propagates in the background).
    let onAbort: (() => void) | null = null;
    const abortPromise = signal
      ? new Promise<never>((_, reject) => {
          onAbort = () =>
            reject(new DOMException("The operation was aborted", "AbortError"));
          signal.addEventListener("abort", onAbort);
        })
      : null;

    let rawRes: Awaited<ReturnType<RawFetchFn>>;
    try {
      rawRes = abortPromise
        ? await Promise.race([
            rawFetch(endpointHandle, nodeId, url, method, headers, reqBodyHandle, fetchToken),
            abortPromise,
          ])
        : await rawFetch(endpointHandle, nodeId, url, method, headers, reqBodyHandle, fetchToken);
    } catch (err) {
      if (err instanceof DOMException && err.name === "AbortError") throw err;
      throw classifyError(err);
    } finally {
      if (signal && onAbort) signal.removeEventListener("abort", onAbort);
      // Remove the transport-cancel listener once the response head is received.
      if (signal && cancelAbortListener) {
        signal.removeEventListener("abort", cancelAbortListener);
        cancelAbortListener = null;
      }
    }

    if (bodyPipePromise) {
      bodyPipePromise.catch((err) =>
        console.error("[iroh-http] request body pipe error:", err)
      );
    }

    // Wire AbortSignal to cancel the body reader (§3).
    // Keep a reference to the listener so we can remove it when the body closes (§1.2).
    let cancelOnAbort: (() => void) | null = null;
    if (signal) {
      cancelOnAbort = () => bridge.cancelRequest(rawRes.bodyHandle);
      signal.addEventListener("abort", cancelOnAbort);
    }

    // Wrap response body in a ReadableStream.
    // When the stream closes (EOF or cancel), remove the abort listener to avoid a leak.
    const resBody = makeReadable(bridge, rawRes.bodyHandle, () => {
      if (signal && cancelOnAbort) {
        signal.removeEventListener("abort", cancelOnAbort!);
        cancelOnAbort = null;
      }
    });

    const response = new Response(resBody, {
      status: rawRes.status,
      headers: rawRes.headers,
    });

    // Shadow the read-only Response.url with the http+iroh:// address (§brief).
    Object.defineProperty(response, "url", {
      value: rawRes.url,
      writable: false,
      enumerable: false,
      configurable: true,
    });

    // Populate res.trailers as a cached lazy Promise<Headers> (§4).
    // Caching is required because the Rust slab entry is consumed on first access.
    const trailersHandle = rawRes.trailersHandle;
    let cachedTrailers: Promise<Headers> | null = null;
    Object.defineProperty(response, "trailers", {
      get: () => {
        if (!cachedTrailers) {
          cachedTrailers = bridge
            .nextTrailer(trailersHandle)
            .then((pairs) => (pairs ? new Headers(pairs) : new Headers()));
        }
        return cachedTrailers;
      },
      configurable: true,
    });

    return response;
  };
}

// ── §2 Bidirectional streaming ────────────────────────────────────────────────

/**
 * Construct a `createBidirectionalStream`-like function that opens a full-duplex stream.
 *
 * The returned `BidirectionalStream` exposes `readable` (data from server) and
 * `writable` (data to server).  Both sides are open simultaneously.
 */
export function makeConnect(
  bridge: Bridge,
  endpointHandle: number,
  rawConnect: RawConnectFn
): (peer: PublicKey | string, path: string, init?: RequestInit) => Promise<BidirectionalStream> {
  return async (peer, path, init) => {
    const nodeId = resolveNodeId(peer);
    const headers = normaliseHeaders(init?.headers);
    const ffi = await rawConnect(endpointHandle, nodeId, path, headers)
      .catch((err) => { throw classifyError(err); });

    const readable = makeReadable(bridge, ffi.readHandle);
    const writable = new WritableStream<Uint8Array>({
      async write(chunk) {
        await bridge.sendChunk(ffi.writeHandle, chunk);
      },
      async close() {
        await bridge.finishBody(ffi.writeHandle);
      },
      async abort() {
        await bridge.finishBody(ffi.writeHandle);
      },
    });

    return { readable, writable };
  };
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function normaliseHeaders(
  h: HeadersInit | undefined | null
): [string, string][] {
  if (!h) return [];
  if (h instanceof Headers) {
    const pairs: [string, string][] = [];
    h.forEach((v, k) => pairs.push([k, v]));
    return pairs;
  }
  if (Array.isArray(h)) return h as [string, string][];
  return Object.entries(h) as [string, string][];
}
