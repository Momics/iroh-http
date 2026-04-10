/**
 * `makeFetch` — wraps the raw platform fetch in the web-standard signature.
 *
 * ```ts
 * const nodeFetch = makeFetch(bridge, endpointHandle, rawFetch, allocBodyWriter);
 * const res = await nodeFetch(remotePeerId, '/api/data');
 * ```
 */

import type { Bridge, RawFetchFn, AllocBodyWriterFn } from "./bridge.js";
import { makeReadable, pipeToWriter, bodyInitToStream } from "./streams.js";

export type FetchFn = (
  nodeId: string,
  input: string | URL,
  init?: RequestInit
) => Promise<Response>;

/**
 * Construct a `fetch`-like function bound to a specific `IrohEndpoint`.
 *
 * @param bridge          Platform bridge (nextChunk / sendChunk / finishBody).
 * @param endpointHandle  Handle to the bound Iroh endpoint.
 * @param rawFetch        Low-level platform fetch function.
 * @param allocBodyWriter Allocates a body writer handle for request bodies.
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

    // Normalise headers into [string, string][] pairs.
    const headers: [string, string][] = normaliseHeaders(init?.headers);

    // If there is a request body, allocate a writer handle and pipe in background.
    let reqBodyHandle: number | null = null;
    let bodyPipePromise: Promise<void> | null = null;

    const bodyStream = init?.body ? bodyInitToStream(init.body) : null;
    if (bodyStream) {
      reqBodyHandle = await allocBodyWriter();
      // Start piping immediately; rawFetch will read from the channel while
      // the pipeToWriter call is in progress.
      bodyPipePromise = pipeToWriter(bridge, bodyStream, reqBodyHandle);
    }

    const rawRes = await rawFetch(
      endpointHandle,
      nodeId,
      url,
      method,
      headers,
      reqBodyHandle
    );

    // Ensure body piping errors surface even if the fetch succeeds.
    if (bodyPipePromise) {
      bodyPipePromise.catch((err) =>
        console.error("[iroh-http] request body pipe error:", err)
      );
    }

    // Wrap the response body handle in a ReadableStream.
    const resBody = makeReadable(bridge, rawRes.bodyHandle);

    const response = new Response(resBody, {
      status: rawRes.status,
      headers: rawRes.headers,
    });

    return response;
  };
}

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
