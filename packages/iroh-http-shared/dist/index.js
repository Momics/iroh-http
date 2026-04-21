"use strict";
/**
 * iroh-http-shared — public exports.
 *
 * Platform adapters (iroh-http-node, iroh-http-tauri) import from here
 * to wire their bridge implementations into the shared layer.
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.normaliseRelayMode = exports.encodeBase64 = exports.decodeBase64 = exports.IrohStreamError = exports.IrohProtocolError = exports.IrohHandleError = exports.IrohError = exports.IrohConnectError = exports.IrohBindError = exports.IrohArgumentError = exports.IrohAbortError = exports.classifyError = exports.classifyBindError = exports.SecretKey = exports.resolveNodeId = exports.PublicKey = exports.makeServe = exports.makeFetch = exports.makeConnect = exports.pipeToWriter = exports.makeReadable = exports.bodyInitToStream = exports.buildSession = void 0;
exports.ticketNodeId = ticketNodeId;
exports.buildNode = buildNode;
// ── Internal types (used by platform adapters, not end users) ───────────────
// Adapter packages import these from "@momics/iroh-http-shared/adapter" instead.
// Bridge is kept here for use by buildNode() below.
var session_js_1 = require("./session.js");
Object.defineProperty(exports, "buildSession", { enumerable: true, get: function () { return session_js_1.buildSession; } });
var streams_js_1 = require("./streams.js");
Object.defineProperty(exports, "bodyInitToStream", { enumerable: true, get: function () { return streams_js_1.bodyInitToStream; } });
Object.defineProperty(exports, "makeReadable", { enumerable: true, get: function () { return streams_js_1.makeReadable; } });
Object.defineProperty(exports, "pipeToWriter", { enumerable: true, get: function () { return streams_js_1.pipeToWriter; } });
var fetch_js_1 = require("./fetch.js");
Object.defineProperty(exports, "makeConnect", { enumerable: true, get: function () { return fetch_js_1.makeConnect; } });
Object.defineProperty(exports, "makeFetch", { enumerable: true, get: function () { return fetch_js_1.makeFetch; } });
var serve_js_1 = require("./serve.js");
Object.defineProperty(exports, "makeServe", { enumerable: true, get: function () { return serve_js_1.makeServe; } });
var keys_js_1 = require("./keys.js");
Object.defineProperty(exports, "PublicKey", { enumerable: true, get: function () { return keys_js_1.PublicKey; } });
Object.defineProperty(exports, "resolveNodeId", { enumerable: true, get: function () { return keys_js_1.resolveNodeId; } });
Object.defineProperty(exports, "SecretKey", { enumerable: true, get: function () { return keys_js_1.SecretKey; } });
var errors_js_1 = require("./errors.js");
Object.defineProperty(exports, "classifyBindError", { enumerable: true, get: function () { return errors_js_1.classifyBindError; } });
Object.defineProperty(exports, "classifyError", { enumerable: true, get: function () { return errors_js_1.classifyError; } });
Object.defineProperty(exports, "IrohAbortError", { enumerable: true, get: function () { return errors_js_1.IrohAbortError; } });
Object.defineProperty(exports, "IrohArgumentError", { enumerable: true, get: function () { return errors_js_1.IrohArgumentError; } });
Object.defineProperty(exports, "IrohBindError", { enumerable: true, get: function () { return errors_js_1.IrohBindError; } });
Object.defineProperty(exports, "IrohConnectError", { enumerable: true, get: function () { return errors_js_1.IrohConnectError; } });
Object.defineProperty(exports, "IrohError", { enumerable: true, get: function () { return errors_js_1.IrohError; } });
Object.defineProperty(exports, "IrohHandleError", { enumerable: true, get: function () { return errors_js_1.IrohHandleError; } });
Object.defineProperty(exports, "IrohProtocolError", { enumerable: true, get: function () { return errors_js_1.IrohProtocolError; } });
Object.defineProperty(exports, "IrohStreamError", { enumerable: true, get: function () { return errors_js_1.IrohStreamError; } });
var utils_js_1 = require("./utils.js");
Object.defineProperty(exports, "decodeBase64", { enumerable: true, get: function () { return utils_js_1.decodeBase64; } });
Object.defineProperty(exports, "encodeBase64", { enumerable: true, get: function () { return utils_js_1.encodeBase64; } });
Object.defineProperty(exports, "normaliseRelayMode", { enumerable: true, get: function () { return utils_js_1.normaliseRelayMode; } });
/**
 * Extract the node ID from a ticket string without network I/O.
 *
 * Accepts a ticket string (JSON-encoded address info) or a bare node ID
 * string (returned unchanged).
 */
function ticketNodeId(ticket) {
    try {
        const info = JSON.parse(ticket);
        if (info && typeof info.id === "string")
            return info.id;
    }
    catch {
        // Not JSON — treat as bare node ID
    }
    return ticket;
}
const session_js_2 = require("./session.js");
const fetch_js_2 = require("./fetch.js");
const serve_js_2 = require("./serve.js");
const keys_js_2 = require("./keys.js");
/**
 * Factory that constructs an `IrohNode` from platform primitives.
 *
 * Each platform adapter calls this after binding an endpoint.
 *
 * @returns A fully wired `IrohNode` ready for `fetch`, `serve`, and `close`.
 *
 * @example
 * ```ts
 * // Platform adapter wiring (typically internal):
 * const node = buildNode({ bridge, info, rawFetch, rawServe, rawConnect, allocBodyWriter, closeEndpoint, stopServe });
 * const res = await node.fetch(peerId, '/hello');
 * ```
 */
function buildNode(config) {
    const { bridge, info, rawFetch, rawServe, rawConnect, allocBodyWriter, closeEndpoint, stopServe, addrFns, discoveryFns, sessionFns, nativeClosed, } = config;
    let resolveClosed;
    const closedPromise = new Promise((resolve) => {
        resolveClosed = resolve;
    });
    // #60: also resolve node.closed when the native endpoint signals shutdown,
    // so that callers awaiting node.closed are not left hanging on fatal exits.
    if (nativeClosed) {
        nativeClosed.then(() => resolveClosed({ closeCode: 0, reason: "native shutdown" }));
    }
    const publicKey = keys_js_2.PublicKey.fromString(info.nodeId);
    const secretKey = keys_js_2.SecretKey._fromBytesWithPublicKey(info.keypair, publicKey);
    const node = {
        publicKey,
        secretKey,
        fetch: (0, fetch_js_2.makeFetch)(bridge, info.endpointHandle, rawFetch, allocBodyWriter),
        serve: (0, serve_js_2.makeServe)(bridge, info.endpointHandle, rawServe, info.nodeId, closedPromise.then(() => { }), () => stopServe(info.endpointHandle)),
        async connect(peer, init) {
            if (!sessionFns) {
                throw new Error("connect() not supported by this platform adapter");
            }
            const nodeId = (0, keys_js_2.resolveNodeId)(peer);
            const directAddrs = init?.directAddrs ?? null;
            const sessionHandle = await sessionFns.connect(info.endpointHandle, nodeId, directAddrs);
            const remotePk = keys_js_2.PublicKey.fromString(nodeId);
            return (0, session_js_2.buildSession)(bridge, sessionHandle, remotePk, sessionFns);
        },
        browse(options, signal) {
            if (!discoveryFns) {
                throw new Error("browse() not supported by this platform adapter");
            }
            const fns = discoveryFns;
            const handle = info.endpointHandle;
            const svcName = options?.serviceName ?? "iroh-http";
            return {
                [Symbol.asyncIterator]() {
                    let browseHandle = null;
                    return {
                        async next() {
                            if (browseHandle === null) {
                                browseHandle = await fns.mdnsBrowse(handle, svcName);
                            }
                            if (signal?.aborted) {
                                fns.mdnsBrowseClose(browseHandle);
                                browseHandle = null;
                                return { done: true, value: undefined };
                            }
                            // Issue-62: race mdnsNextEvent against AbortSignal so that
                            // aborting on a quiet network unblocks iteration immediately
                            // without waiting for the next discovery event to arrive.
                            let event;
                            if (signal) {
                                const abortPromise = new Promise((resolve) => {
                                    if (signal.aborted) {
                                        resolve(null);
                                        return;
                                    }
                                    signal.addEventListener("abort", () => resolve(null), {
                                        once: true,
                                    });
                                });
                                event = await Promise.race([
                                    fns.mdnsNextEvent(browseHandle),
                                    abortPromise,
                                ]);
                                // Close the native handle immediately on abort so the Rust
                                // side can clean up even while the other branch is pending.
                                if (signal.aborted && browseHandle !== null) {
                                    fns.mdnsBrowseClose(browseHandle);
                                    browseHandle = null;
                                    return { done: true, value: undefined };
                                }
                            }
                            else {
                                event = await fns.mdnsNextEvent(browseHandle);
                            }
                            if (event === null) {
                                return { done: true, value: undefined };
                            }
                            return { done: false, value: event };
                        },
                        return() {
                            if (browseHandle !== null) {
                                fns.mdnsBrowseClose(browseHandle);
                                browseHandle = null;
                            }
                            return Promise.resolve({ done: true, value: undefined });
                        },
                    };
                },
            };
        },
        async advertise(options, signal) {
            if (!discoveryFns) {
                throw new Error("advertise() not supported by this platform adapter");
            }
            const svcName = options?.serviceName ?? "iroh-http";
            const advHandle = await discoveryFns.mdnsAdvertise(info.endpointHandle, svcName);
            if (signal) {
                return new Promise((resolve) => {
                    signal.addEventListener("abort", () => {
                        discoveryFns.mdnsAdvertiseClose(advHandle);
                        resolve();
                    }, { once: true });
                    if (signal.aborted) {
                        discoveryFns.mdnsAdvertiseClose(advHandle);
                        resolve();
                    }
                });
            }
            // No signal — advertise until the node closes.
        },
        addr: async () => {
            if (!addrFns) {
                throw new Error("addr() not supported by this platform adapter");
            }
            return addrFns.nodeAddr(info.endpointHandle);
        },
        ticket: async () => {
            if (!addrFns) {
                throw new Error("ticket() not supported by this platform adapter");
            }
            return addrFns.nodeTicket(info.endpointHandle);
        },
        homeRelay: async () => {
            if (!addrFns)
                return null;
            return addrFns.homeRelay(info.endpointHandle);
        },
        peerInfo: async (peer) => {
            if (!addrFns)
                return null;
            const nodeId = (0, keys_js_2.resolveNodeId)(peer);
            return addrFns.peerInfo(info.endpointHandle, nodeId);
        },
        peerStats: async (peer) => {
            if (!addrFns)
                return null;
            const nodeId = (0, keys_js_2.resolveNodeId)(peer);
            return addrFns.peerStats(info.endpointHandle, nodeId);
        },
        stats: async () => {
            if (!addrFns?.stats) {
                throw new Error("stats() not supported by this platform adapter");
            }
            return addrFns.stats(info.endpointHandle);
        },
        pathChanges(peer, pollIntervalMs = 500) {
            const nodeId = (0, keys_js_2.resolveNodeId)(peer);
            const endpointHandle = info.endpointHandle;
            return {
                [Symbol.asyncIterator]() {
                    let stopped = false;
                    let lastPath = null;
                    let timeoutId = null;
                    let wakeResolve = null;
                    // Schedule a wake-up after pollIntervalMs.
                    function scheduleWake() {
                        timeoutId = setTimeout(() => {
                            timeoutId = null;
                            const r = wakeResolve;
                            wakeResolve = null;
                            r?.();
                        }, pollIntervalMs);
                    }
                    function cancelWake() {
                        if (timeoutId !== null) {
                            clearTimeout(timeoutId);
                            timeoutId = null;
                        }
                        const r = wakeResolve;
                        wakeResolve = null;
                        r?.();
                    }
                    return {
                        async next() {
                            while (!stopped) {
                                const stats = addrFns
                                    ? await addrFns.peerStats(endpointHandle, nodeId)
                                    : null;
                                if (stats) {
                                    const selected = stats.paths.find((p) => p.active);
                                    if (selected) {
                                        const key = `${selected.relay}:${selected.addr}`;
                                        if (key !== lastPath) {
                                            lastPath = key;
                                            scheduleWake();
                                            return { done: false, value: selected };
                                        }
                                    }
                                }
                                // Wait for the next poll interval.
                                await new Promise((resolve) => {
                                    wakeResolve = resolve;
                                    scheduleWake();
                                });
                            }
                            return { done: true, value: undefined };
                        },
                        return() {
                            stopped = true;
                            cancelWake();
                            return Promise.resolve({ done: true, value: undefined });
                        },
                    };
                },
            };
        },
        closed: closedPromise,
        close: async (options) => {
            await closeEndpoint(info.endpointHandle, options?.force);
            resolveClosed({ closeCode: 0, reason: "" });
            // Await nativeClosed so that the waitEndpointClosed async FFI op is fully
            // drained from the runtime's op queue before close() settles. Without this,
            // Deno's sanitize-ops checker sees the pending non-blocking FFI op as a leak
            // because closeEndpoint triggers closed_tx in Rust (resolving the Rust
            // wait_closed future) but the JS side has not yet processed that op result.
            if (nativeClosed)
                await nativeClosed;
        },
        [Symbol.asyncDispose]() {
            return node.close();
        },
    };
    return node;
}
//# sourceMappingURL=index.js.map