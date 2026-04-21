"use strict";
/**
 * `makeServe` — wraps the raw platform serve in a Deno-compatible signature.
 *
 * ```ts
 * const serve = makeServe(bridge, handle, rawServe, nodeId, finished, stopServe);
 * const server = serve(async (req) => Response.json({ ok: true }));
 * await server.finished;
 * ```
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.makeServe = makeServe;
const streams_js_1 = require("./streams.js");
const errors_js_1 = require("./errors.js");
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
function makeServe(bridge, endpointHandle, rawServe, nodeId, onNodeClose, stopServe) {
    return ((...args) => {
        // Parse overloaded arguments.
        let handler;
        let options = {};
        if (typeof args[0] === "function") {
            // serve(handler)
            handler = args[0];
        }
        else if (args.length >= 2 && typeof args[1] === "function") {
            // serve(options, handler)
            options = args[0] ?? {};
            handler = args[1];
        }
        else if (args.length === 1 && typeof args[0] === "object" && args[0] !== null) {
            // serve({ handler, ...options })
            const opts = args[0];
            if (typeof opts.handler !== "function") {
                throw new TypeError("serve() requires a handler function");
            }
            handler = opts.handler;
            options = opts;
        }
        else {
            throw new TypeError("serve() requires a handler function");
        }
        const onError = options.onError ?? defaultOnError;
        // Build a unified connection event callback from onPeerConnect / onPeerDisconnect.
        const onConnectionEvent = (options.onPeerConnect || options.onPeerDisconnect)
            ? (ev) => {
                if (ev.connected) {
                    options.onPeerConnect?.(ev.peerId);
                }
                else {
                    options.onPeerDisconnect?.(ev.peerId);
                }
            }
            : undefined;
        // rawServe returns a Promise<void> that resolves when its internal polling
        // loop exits (i.e. after stopServe() causes nextRequest to drain to null).
        const loopDone = rawServe(endpointHandle, { onConnectionEvent }, async (payload) => {
            const peerId = headerValue(payload.headers, "peer-id");
            // Build a web-standard Request.
            const hasBody = !METHODS_WITHOUT_BODY.has(payload.method.toUpperCase());
            const reqBody = (hasBody && !payload.isBidi)
                ? (0, streams_js_1.makeReadable)(bridge, payload.reqBodyHandle)
                : null;
            // Peer-Id is stripped (spoof prevention) and re-injected from the
            // authenticated QUIC connection identity in Rust core. No duplication here.
            const headers = [...payload.headers];
            const reqInit = {
                method: payload.method,
                headers,
                body: reqBody,
            };
            if (reqBody)
                reqInit.duplex = "half";
            const req = new Request(payload.url, reqInit);
            if (payload.isBidi) {
                // Issue-61: acceptWebTransport() must be single-use per request.
                // Each duplex request has exactly one pair of body handles; creating
                // multiple stream wrappers over the same handles causes undefined
                // behaviour. The flag is captured in the closure so it is scoped to
                // this one request.
                let accepted = false;
                const acceptWebTransportFn = () => {
                    if (accepted) {
                        throw new TypeError("acceptWebTransport() has already been called on this request. " +
                            "Each duplex request can only be accepted once.");
                    }
                    accepted = true;
                    return {
                        readable: (0, streams_js_1.makeReadable)(bridge, payload.reqBodyHandle),
                        writable: new WritableStream({
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
            let res;
            try {
                res = await Promise.resolve(handler(req));
            }
            catch (err) {
                try {
                    res = await Promise.resolve(onError(err));
                }
                catch {
                    res = new Response("Internal Server Error", { status: 500 });
                }
            }
            if (payload.isBidi) {
                return {
                    status: res.status,
                    headers: [...res.headers],
                };
            }
            const bodyStream = res.body ?? emptyStream();
            const doPipe = async () => {
                await (0, streams_js_1.pipeToWriter)(bridge, bodyStream, payload.resBodyHandle);
            };
            doPipe().catch((err) => console.error("[iroh-http] response body pipe error:", (0, errors_js_1.classifyError)(err)));
            return {
                status: res.status,
                headers: [...res.headers],
            };
        });
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
        const doStop = () => {
            stopServe();
            // Rust will drain the loop and then loopDone resolves; we do NOT resolve
            // finished ourselves here.
        };
        // Wire signal → doStop for graceful shutdown.
        if (options.signal) {
            if (options.signal.aborted) {
                doStop();
            }
            else {
                options.signal.addEventListener("abort", () => doStop(), {
                    once: true,
                });
            }
        }
        return { finished };
    });
}
function defaultOnError(error) {
    console.error("[iroh-http] unhandled handler error:", error);
    return new Response("Internal Server Error", { status: 500 });
}
function emptyStream() {
    return new ReadableStream({
        start(controller) {
            controller.close();
        },
    });
}
function headerValue(headers, name) {
    const needle = name.toLowerCase();
    for (const [k, v] of headers) {
        if (k.toLowerCase() === needle)
            return v;
    }
    return null;
}
//# sourceMappingURL=serve.js.map