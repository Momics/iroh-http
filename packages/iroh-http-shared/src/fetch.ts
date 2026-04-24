/**
 * `makeFetch` — wraps the raw platform fetch in the web-standard signature.
 *
 * ```ts
 * const nodeFetch = makeFetch(adapter, endpointHandle);
 * const res = await nodeFetch(remotePeerId, '/api/data');
 * ```
 */

import type {
  FfiResponse,
  IrohAdapter,
  IrohFetchInit,
} from "./IrohAdapter.js";
import { bodyInitToStream, makeReadable, pipeToWriter } from "./streams.js";
import type { PublicKey } from "./PublicKey.js";
import { resolveNodeId } from "./PublicKey.js";
import { classifyError } from "./errors.js";

export type FetchFn = {
  /** Web-standard form: peer identity is embedded in the `httpi://` URL hostname. */
  (input: string | URL, init?: IrohFetchInit): Promise<Response>;
  /** Legacy two-argument form: peer and path supplied separately. */
  (
    peer: PublicKey | string,
    input: string | URL,
    init?: IrohFetchInit,
  ): Promise<Response>;
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
export function makeFetch(
  adapter: IrohAdapter,
  endpointHandle: number,
): FetchFn {
  return async function irohFetch(
    peerOrInput: PublicKey | string | URL,
    inputOrInit?: string | URL | IrohFetchInit,
    maybeInit?: IrohFetchInit,
  ): Promise<Response> {
    let nodeId: string;
    let url: string;
    let init: IrohFetchInit | undefined;

    if (typeof inputOrInit === "string" || inputOrInit instanceof URL) {
      // Old form: fetch(peer, path, init?)
      nodeId = resolveNodeId(peerOrInput as PublicKey | string);
      url = typeof inputOrInit === "string" ? inputOrInit : inputOrInit.href;
      init = maybeInit;
    } else {
      // New form: fetch("httpi://peerId/path", init?)
      const raw = peerOrInput instanceof URL
        ? peerOrInput.href
        : String(peerOrInput);
      if (!/^httpi:\/\//i.test(raw)) {
        throw new TypeError(
          `iroh-http fetch() requires either an httpi:// URL or (peer, path) arguments. ` +
            `Got: "${raw.slice(0, 80)}"`,
        );
      }
      const parsed = new URL(raw);
      nodeId = parsed.hostname;
      url = raw;
      init = inputOrInit as IrohFetchInit | undefined;
    }

    // Reject standard web schemes — iroh-http uses httpi://, not https:// or http://.
    if (/^https?:\/\//i.test(url)) {
      const scheme = url.slice(0, url.indexOf("://") + 3);
      throw new TypeError(
        `iroh-http URLs must use the "httpi://" scheme, not "${scheme}". ` +
          `Example: httpi://nodeId/path — or pass a bare path like "/api/data".`,
      );
    }

    const method = init?.method ?? "GET";
    const signal = init?.signal ?? null;
    const directAddrs = init?.directAddrs ?? null;

    // Reject GET and HEAD request bodies — matches web-platform fetch semantics
    // (https://fetch.spec.whatwg.org/#concept-method-normalize, issue-58).
    if (
      (method.toUpperCase() === "GET" || method.toUpperCase() === "HEAD") &&
      init?.body != null
    ) {
      throw new TypeError(
        `Request body is not allowed for ${method} requests. ` +
          `The web-platform fetch specification forbids bodies on GET and HEAD.`,
      );
    }

    // Reject immediately if already aborted.
    if (signal?.aborted) {
      throw new DOMException("The operation was aborted", "AbortError");
    }

    const headers: [string, string][] = normaliseHeaders(init?.headers);

    // Allocate request body writer if needed.
    let reqBodyHandle: bigint | null = null;
    let bodyPipePromise: Promise<void> | null = null;
    const bodyStream = init?.body ? bodyInitToStream(init.body) : null;
    if (bodyStream) {
      reqBodyHandle = await adapter.allocBodyWriter(endpointHandle);
      bodyPipePromise = pipeToWriter(adapter, bodyStream, reqBodyHandle);
    }

    // Allocate a Rust-side cancellation token so that AbortSignal can cancel
    // the transport even before the response head arrives (§3 enhanced).
    const fetchToken = await adapter.allocFetchToken(endpointHandle);

    // Wire AbortSignal → cancelFetch as early as possible (fire-and-forget).
    // This fires even if the signal is already aborted.
    let cancelAbortListener: (() => void) | null = null;
    if (signal) {
      if (signal.aborted) {
        adapter.cancelFetch(fetchToken);
        throw new DOMException("The operation was aborted", "AbortError");
      }
      cancelAbortListener = () => adapter.cancelFetch(fetchToken);
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

    let rawRes: FfiResponse;
    try {
      rawRes = abortPromise
        ? await Promise.race([
          adapter.rawFetch(
            endpointHandle,
            nodeId,
            url,
            method,
            headers,
            reqBodyHandle,
            fetchToken,
            directAddrs,
          ),
          abortPromise,
        ])
        : await adapter.rawFetch(
          endpointHandle,
          nodeId,
          url,
          method,
          headers,
          reqBodyHandle,
          fetchToken,
          directAddrs,
        );
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

    // Core returns body_handle = 0 (the slotmap null sentinel) for null-body
    // status codes (RFC 9110 §6.3: 204, 205, 304).  No channel was allocated
    // on the Rust side, so there is nothing to wire up here.
    let responseBody: ReadableStream<Uint8Array> | null = null;
    if (rawRes.bodyHandle !== 0n) {
      // Wire AbortSignal to cancel the body reader (§3).
      // Uses `once: true` so the listener auto-removes after firing — prevents
      // leaks when the response body is never consumed by the caller.
      let cancelOnAbort: (() => void) | null = null;
      if (signal) {
        cancelOnAbort = () => adapter.cancelRequest(rawRes.bodyHandle);
        signal.addEventListener("abort", cancelOnAbort, { once: true });
      }

      // Wrap response body in a ReadableStream.
      // When the stream closes (EOF or cancel), remove the abort listener to avoid a leak.
      responseBody = makeReadable(adapter, rawRes.bodyHandle, () => {
        if (signal && cancelOnAbort) {
          signal.removeEventListener("abort", cancelOnAbort!);
          cancelOnAbort = null;
        }
      });
    }

    const response = new Response(responseBody, {
      status: rawRes.status,
      headers: rawRes.headers,
    });

    // Shadow the read-only Response.url with the httpi:// address (§brief).
    Object.defineProperty(response, "url", {
      value: rawRes.url,
      writable: false,
      enumerable: false,
      configurable: true,
    });

    return response;
  };
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function normaliseHeaders(
  h: HeadersInit | undefined | null,
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
