/**
 * `makeServe` — wraps the raw platform serve in a Deno-compatible signature.
 *
 * ```ts
 * const serve = makeServe(bridge, handle, rawServe, nodeId, finished, stopServe);
 * const server = serve(async (req) => Response.json({ ok: true }));
 * await server.finished;
 * ```
 */

import type {
  FfiResponseHead,
  IrohAdapter,
  PeerConnectionEvent,
  RequestPayload,
} from "./IrohAdapter.js";
import { makeReadable, pipeToWriter } from "./streams.js";
import { classifyError } from "./errors.js";

/**
 * A request handler that receives a web-standard `Request` and returns a `Response`.
 *
 * The `Request` is augmented with:
 * - `req.headers.get('Peer-Id')` — the authenticated peer's public key.
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
 * Two overloaded call signatures for `serve()`:
 *
 * 1. `serve(handler)` — handler only (most common).
 * 2. `serve(options, handler)` — options + separate handler argument.
 */
export type ServeFn = {
  (handler: ServeHandler): ServeHandle;
  (options: ServeOptions, handler: ServeHandler): ServeHandle;
};

/**
 * HTTP methods that categorically cannot carry a request body per RFC 9110.
 * All other methods — including OPTIONS, custom verbs, etc. — may carry a body
 * and should have the body stream forwarded to the handler (issue-58 fix).
 */
const METHODS_WITHOUT_BODY = new Set(["GET", "HEAD", "CONNECT", "TRACE"]);

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
export function makeServe(
  adapter: IrohAdapter,
  endpointHandle: number,
  nodeId: string,
  onNodeClose: Promise<void>,
  onPeerEvent?: (event: PeerConnectionEvent) => void,
): ServeFn {
  // #114: guard against starting two polling loops on the same endpoint.
  let serveRunning = false;

  return ((...args: unknown[]): ServeHandle => {
    if (serveRunning) {
      throw new TypeError(
        "serve() is already running on this node. Call signal.abort() or node.close() to stop it first.",
      );
    }
    serveRunning = true;

    // Parse overloaded arguments.
    let handler: ServeHandler;
    let options: ServeOptions = {};

    if (typeof args[0] === "function") {
      // serve(handler)
      handler = args[0] as ServeHandler;
    } else if (args.length >= 2 && typeof args[1] === "function") {
      // serve(options, handler)
      options = (args[0] as ServeOptions) ?? {};
      handler = args[1] as ServeHandler;
    } else {
      throw new TypeError("serve() requires a handler function");
    }

    const onError = options.onError ?? defaultOnError;

    // Peer connect/disconnect events are dispatched as CustomEvents on IrohNode.
    // The dispatcher is provided by IrohNode at construction time so events fire
    // on the node regardless of which serve() call is running.
    const onConnectionEvent: ((event: PeerConnectionEvent) => void) | undefined =
      onPeerEvent;

    // #119: Track active body pipes so `finished` drains them before resolving.
    const activePipes = new Set<Promise<void>>();

    // rawServe returns a Promise<void> that resolves when its internal polling
    // loop exits (i.e. after stopServe() causes nextRequest to drain to null).
    const loopDone = adapter.rawServe(
      endpointHandle,
      { onConnectionEvent },
      async (payload: RequestPayload): Promise<FfiResponseHead> => {
        const peerId = headerValue(payload.headers, "peer-id");

        // Build a web-standard Request.
        const hasBody = !METHODS_WITHOUT_BODY.has(payload.method.toUpperCase());
        const reqBody = hasBody
          ? makeReadable(adapter, payload.reqBodyHandle)
          : null;

        // Peer-Id is stripped (spoof prevention) and re-injected from the
        // authenticated QUIC connection identity in Rust core. No duplication here.
        const headers: [string, string][] = [...payload.headers];

        const reqInit: RequestInit & { duplex?: "half" } = {
          method: payload.method,
          headers,
          body: reqBody,
        };
        if (reqBody) reqInit.duplex = "half";

        const req = new Request(
          payload.url,
          reqInit,
        );

        // Invoke the user handler with onError fallback.
        let res: Response;
        try {
          res = await Promise.resolve(handler(req));
        } catch (err) {
          try {
            res = await Promise.resolve(onError(err));
          } catch {
            res = new Response("Internal Server Error", { status: 500 });
          }
        }

        const bodyStream = res.body ?? emptyStream();
        const doPipe = async () => {
          await pipeToWriter(adapter, bodyStream, payload.resBodyHandle);
        };
        const p = doPipe().catch((err) =>
          console.error(
            "[iroh-http] response body pipe error:",
            classifyError(err),
          )
        );
        activePipes.add(p);
        p.finally(() => activePipes.delete(p));

        return {
          status: res.status,
          headers: [...res.headers] as [string, string][],
        };
      },
    );

    // ISS-029 / #59 / #115: finished resolves when the serve loop actually terminates.
    // `loopDone` is the real loop-lifetime promise returned by rawServe():
    //  - Deno: resolves when the nextRequest polling loop exits (null sentinel).
    //  - Node / Tauri: resolves when waitServeStop() confirms the Rust task drained.
    //
    // #115: finished must NOT resolve via onNodeClose alone. When the node closes,
    // close_endpoint() calls serve_registry::remove() which sends the shutdown signal
    // to the pending nextRequest Tokio task — but that task is scheduled, not yet run.
    // If finished resolved immediately on onNodeClose, the nextRequest FFI op would
    // still be in-flight (Deno sanitizeOps / process exit timing bug).
    //
    // Fix: when onNodeClose fires, chain it on loopDone. loopDone is guaranteed to
    // resolve because close_endpoint always calls serve_registry::remove first, which
    // unblocks the pending nextRequest. If loopDone resolves first (normal path when
    // stopServe was called explicitly), the race wins immediately.
    // #119: drain all in-flight body pipes before resolving so callers of
    // `await finished` see a true "all work done" guarantee.
    const finished: Promise<void> = Promise.race([loopDone, onNodeClose.then(() => loopDone)])
      .then(() => Promise.allSettled([...activePipes]))
      .then(() => {});
    // Reset guard when the loop finishes so serve() can be called again.
    finished.finally(() => { serveRunning = false; });

    const doStop = (): void => {
      adapter.stopServe(endpointHandle);
      // Rust will drain the loop and then loopDone resolves; we do NOT resolve
      // finished ourselves here.
    };

    // Wire signal → doStop for graceful shutdown.
    if (options.signal) {
      if (options.signal.aborted) {
        doStop();
      } else {
        options.signal.addEventListener("abort", () => doStop(), {
          once: true,
        });
      }
    }

    return { finished };
  }) as ServeFn;
}

function defaultOnError(error: unknown): Response {
  console.error("[iroh-http] unhandled handler error:", error);
  return new Response("Internal Server Error", { status: 500 });
}

function emptyStream(): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    start(controller) {
      controller.close();
    },
  });
}

function headerValue(
  headers: [string, string][],
  name: string,
): string | null {
  const needle = name.toLowerCase();
  for (const [k, v] of headers) {
    if (k.toLowerCase() === needle) return v;
  }
  return null;
}
