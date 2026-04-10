/**
 * `makeServe` — wraps the raw platform serve in a Deno-compatible signature.
 *
 * ```ts
 * const serve = makeServe(bridge, endpointHandle, rawServe);
 * serve({}, async (req) => {
 *   const peerId = req.headers.get('iroh-node-id');
 *   return Response.json({ peer: peerId });
 * });
 * ```
 */

import type {
  Bridge,
  FfiResponseHead,
  RawServeFn,
  RequestPayload,
  BidirectionalStream,
} from "./bridge.js";
import { makeReadable, pipeToWriter } from "./streams.js";

export type ServeHandler = (req: Request) => Response | Promise<Response>;

export type ServeFn = (
  options: Record<string, unknown>,
  handler: ServeHandler
) => void;

/**
 * HTTP methods that carry a request body.
 */
const METHODS_WITH_BODY = new Set(["POST", "PUT", "PATCH", "DELETE"]);

/**
 * Construct a Deno-compatible `serve` function bound to a specific endpoint.
 */
export function makeServe(
  bridge: Bridge,
  endpointHandle: number,
  rawServe: RawServeFn
): ServeFn {
  return (options, handler) => {
    rawServe(endpointHandle, options, async (payload: RequestPayload): Promise<FfiResponseHead> => {
      // Build a web-standard Request.
      const hasBody = METHODS_WITH_BODY.has(payload.method.toUpperCase());
      const reqBody = hasBody
        ? makeReadable(bridge, payload.reqBodyHandle)
        : null;

      // Inject the authenticated peer identity as a header.
      const headers: [string, string][] = [
        ...payload.headers,
        ["iroh-node-id", payload.remoteNodeId],
      ];

      const reqInit: RequestInit & { duplex?: "half" } = {
        method: payload.method,
        headers,
        body: reqBody,
      };
      if (reqBody) reqInit.duplex = "half";

      const req = new Request(payload.url, reqInit);

      // §4: Expose request trailers as req.trailers (Promise<Headers>).
      if (payload.reqTrailersHandle) {
        Object.defineProperty(req, "trailers", {
          value: bridge
            .nextTrailer(payload.reqTrailersHandle)
            .then((pairs) => (pairs ? new Headers(pairs) : new Headers())),
          configurable: true,
        });
      }

      // §2: For duplex requests, attach req.acceptWebTransport() so the handler can get both streams.
      if (payload.isBidi) {
        const acceptWebTransportFn = (): BidirectionalStream => ({
          readable: makeReadable(bridge, payload.reqBodyHandle),
          writable: new WritableStream<Uint8Array>({
            async write(chunk) {
              await bridge.sendChunk(payload.resBodyHandle, chunk);
            },
            async close() {
              await bridge.finishBody(payload.resBodyHandle);
            },
            async abort() {
              await bridge.finishBody(payload.resBodyHandle);
            },
          }),
        });
        Object.defineProperty(req, "acceptWebTransport", {
          value: acceptWebTransportFn,
          configurable: true,
        });
      }

      // Invoke the user handler.
      const res = await Promise.resolve(handler(req));

      if (payload.isBidi) {
        // Duplex mode: the handler manages both streams via req.duplex().
        // We only return the response head (101); body piping is handler-driven.
        return {
          status: res.status,
          headers: [...res.headers] as [string, string][],
        };
      }

      // §4: Collect the response trailers callback (non-standard extension).
      const trailersFn = (res as unknown as Record<string, unknown>)
        .trailers as (() => Headers | Promise<Headers>) | undefined;

      // Pipe response body in the background, then send trailers.
      const bodyStream = res.body ?? emptyStream();
      const doPipe = async () => {
        await pipeToWriter(bridge, bodyStream, payload.resBodyHandle);
        // Always call sendTrailers so the Rust pump task can proceed.
        const trailerPairs: [string, string][] = trailersFn
          ? [...(await trailersFn())] as [string, string][]
          : [];
        if (payload.resTrailersHandle) {
          await bridge.sendTrailers(payload.resTrailersHandle, trailerPairs);
        }
      };
      doPipe().catch((err) =>
        console.error("[iroh-http] response body pipe error:", err)
      );

      return {
        status: res.status,
        headers: [...res.headers] as [string, string][],
      };
    });
  };
}

function emptyStream(): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    start(controller) {
      controller.close();
    },
  });
}
