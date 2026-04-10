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
 *
 * @param bridge          Platform bridge (nextChunk / sendChunk / finishBody).
 * @param endpointHandle  Handle to the bound Iroh endpoint.
 * @param rawServe        Low-level platform serve function.
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

      // Duplex: 'half' allows streaming request bodies in Node.js fetch.
      const reqInit: RequestInit & { duplex?: "half" } = {
        method: payload.method,
        headers,
        body: reqBody,
      };
      if (reqBody) reqInit.duplex = "half";

      const req = new Request(payload.url, reqInit);

      // Invoke the user handler.
      const res = await Promise.resolve(handler(req));

      // Pipe response body in background — JS does NOT wait for completion
      // before returning the head.  Rust reads the body concurrently via
      // sendChunk / finishBody on resBodyHandle.
      const bodyStream =
        res.body ?? emptyStream();
      pipeToWriter(bridge, bodyStream, payload.resBodyHandle).catch((err) =>
        console.error("[iroh-http] response body pipe error:", err)
      );

      // Return the response head so Rust can write the status line + headers.
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
