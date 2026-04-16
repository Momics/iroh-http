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
  BidirectionalStream,
  Bridge,
  FfiResponseHead,
  IrohServeResponse,
  PeerConnectionEvent,
  RawServeFn,
  RequestPayload,
} from "./bridge.js";
import { makeReadable, pipeToWriter } from "./streams.js";
import { classifyError } from "./errors.js";

/**
 * A request handler that receives a web-standard `Request` and returns a `Response`.
 *
 * The `Request` is augmented with:
 * - `req.headers.get('Peer-Id')` — the authenticated peer's public key.
 * - `req.trailers` — a `Promise<Headers>` resolving to request trailer headers.
 * - `req.acceptWebTransport()` — (duplex only) returns `{ readable, writable }`.
 *
 * The `Response` may be an `IrohServeResponse` with an optional `trailers` callback:
 * `trailers: () => Headers | Promise<Headers>` called after the body completes.
 */
export type ServeHandler = (req: Request) => Response | IrohServeResponse | Promise<Response | IrohServeResponse>;

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
  onListen?: (info: { nodeId: string }) => void;

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
  (options: ServeOptions & { handler: ServeHandler }): ServeHandle;
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
  bridge: Bridge,
  endpointHandle: number,
  rawServe: RawServeFn,
  nodeId: string,
  onNodeClose: Promise<void>,
  stopServe: () => void,
): ServeFn {
  return ((...args: unknown[]): ServeHandle => {
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
    } else if (
      args.length === 1 && typeof args[0] === "object" && args[0] !== null
    ) {
      // serve({ handler, ...options })
      const opts = args[0] as ServeOptions & { handler: ServeHandler };
      if (typeof opts.handler !== "function") {
        throw new TypeError("serve() requires a handler function");
      }
      handler = opts.handler;
      options = opts;
    } else {
      throw new TypeError("serve() requires a handler function");
    }

    const onError = options.onError ?? defaultOnError;

    // Build a unified connection event callback from onPeerConnect / onPeerDisconnect.
    const onConnectionEvent: ((event: PeerConnectionEvent) => void) | undefined =
      (options.onPeerConnect || options.onPeerDisconnect)
        ? (ev: PeerConnectionEvent) => {
          if (ev.connected) {
            options.onPeerConnect?.(ev.peerId);
          } else {
            options.onPeerDisconnect?.(ev.peerId);
          }
        }
        : undefined;

    // rawServe returns a Promise<void> that resolves when its internal polling
    // loop exits (i.e. after stopServe() causes nextRequest to drain to null).
    const loopDone = rawServe(
      endpointHandle,
      { onConnectionEvent },
      async (payload: RequestPayload): Promise<FfiResponseHead> => {
        // Build a web-standard Request.
        const hasBody = !METHODS_WITHOUT_BODY.has(payload.method.toUpperCase());
        const reqBody = (hasBody && !payload.isBidi)
          ? makeReadable(bridge, payload.reqBodyHandle)
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

        if (!payload.isBidi) {
          Object.defineProperty(req, "trailers", {
            value: bridge
              .nextTrailer(payload.reqTrailersHandle)
              .then((pairs) => (pairs ? new Headers(pairs) : new Headers())),
            configurable: true,
          });
        }

        if (payload.isBidi) {
          // Issue-61: acceptWebTransport() must be single-use per request.
          // Each duplex request has exactly one pair of body handles; creating
          // multiple stream wrappers over the same handles causes undefined
          // behaviour. The flag is captured in the closure so it is scoped to
          // this one request.
          let accepted = false;
          const acceptWebTransportFn = (): BidirectionalStream => {
            if (accepted) {
              throw new TypeError(
                "acceptWebTransport() has already been called on this request. " +
                  "Each duplex request can only be accepted once.",
              );
            }
            accepted = true;
            return {
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
            };
          };
          Object.defineProperty(req, "acceptWebTransport", {
            value: acceptWebTransportFn,
            configurable: true,
          });
        }

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

        if (payload.isBidi) {
          return {
            status: res.status,
            headers: [...res.headers] as [string, string][],
          };
        }

        const trailersFn = (res as IrohServeResponse).trailers;

        const bodyStream = res.body ?? emptyStream();
        const doPipe = async () => {
          await pipeToWriter(bridge, bodyStream, payload.resBodyHandle);
          // The server only keeps the trailer sender handle live when the response
          // includes a `Trailer:` header — if that header is absent it removes the
          // handle from the slab before JS gets to call sendTrailers.
          // Also skip in bidi mode (resTrailersHandle === 0).
          const hasTrailerHeader = res.headers.has("trailer");
          if (payload.resTrailersHandle !== 0n && hasTrailerHeader) {
            const trailerPairs: [string, string][] = trailersFn
              ? [...(await trailersFn())] as [string, string][]
              : [];
            await bridge.sendTrailers(payload.resTrailersHandle, trailerPairs);
          }
        };
        doPipe().catch((err) =>
          console.error(
            "[iroh-http] response body pipe error:",
            classifyError(err),
          )
        );

        return {
          status: res.status,
          headers: [...res.headers] as [string, string][],
        };
      },
    );

    // Fire onListen synchronously — iroh binds during createNode, so the
    // serve loop is immediately live after rawServe returns.
    options.onListen?.({ nodeId });

    // ISS-029 / #59: finished resolves when the serve loop actually terminates.
    // `loopDone` is the real loop-lifetime promise returned by rawServe():
    //  - Deno: resolves when the nextRequest polling loop exits (null sentinel).
    //  - Node / Tauri: resolves when waitServeStop() confirms the Rust task drained.
    // We also race against onNodeClose so that closing the node unblocks callers
    // even when stopServe() was never called explicitly.
    const finished = Promise.race([loopDone, onNodeClose]);

    const doStop = (): void => {
      stopServe();
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
