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

export type FetchFn = (
  nodeId: string,
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
    nodeId: string,
    input: string | URL,
    init?: RequestInit
  ): Promise<Response> => {
    const url = typeof input === "string" ? input : input.toString();
    const method = init?.method ?? "GET";
    const signal = init?.signal ?? null;

    // Reject immediately if already aborted.
    if (signal?.aborted) {
      throw Object.assign(new Error("The operation was aborted"), { name: "AbortError" });
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

    // Build an abort promise so we can race it against rawFetch (§3).
    let onAbort: (() => void) | null = null;
    const abortPromise = signal
      ? new Promise<never>((_, reject) => {
          onAbort = () =>
            reject(
              Object.assign(new Error("The operation was aborted"), {
                name: "AbortError",
              })
            );
          signal.addEventListener("abort", onAbort);
        })
      : null;

    let rawRes: Awaited<ReturnType<RawFetchFn>>;
    try {
      rawRes = abortPromise
        ? await Promise.race([
            rawFetch(endpointHandle, nodeId, url, method, headers, reqBodyHandle),
            abortPromise,
          ])
        : await rawFetch(endpointHandle, nodeId, url, method, headers, reqBodyHandle);
    } finally {
      if (signal && onAbort) signal.removeEventListener("abort", onAbort);
    }

    if (bodyPipePromise) {
      bodyPipePromise.catch((err) =>
        console.error("[iroh-http] request body pipe error:", err)
      );
    }

    // Wrap response body in a ReadableStream.
    const resBody = makeReadable(bridge, rawRes.bodyHandle);

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

    // Populate res.trailers as a lazy Promise<Headers> (§4).
    const trailersHandle = rawRes.trailersHandle;
    Object.defineProperty(response, "trailers", {
      get: () =>
        bridge.nextTrailer(trailersHandle).then(
          (pairs) => (pairs ? new Headers(pairs) : new Headers())
        ),
      configurable: true,
    });

    // Wire post-response AbortSignal to cancel the body reader (§3).
    if (signal) {
      const bodyHandle = rawRes.bodyHandle;
      signal.addEventListener("abort", () => {
        bridge.cancelRequest(bodyHandle);
      });
    }

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
): (nodeId: string, path: string, init?: RequestInit) => Promise<BidirectionalStream> {
  return async (nodeId, path, init) => {
    const headers = normaliseHeaders(init?.headers);
    const ffi = await rawConnect(endpointHandle, nodeId, path, headers);

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
