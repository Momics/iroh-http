"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.IrohNode = void 0;
const keys_js_1 = require("./keys.js");
const fetch_js_1 = require("./fetch.js");
const serve_js_1 = require("./serve.js");
const session_js_1 = require("./session.js");
const _INTERNAL = Symbol('IrohNode._create');
class IrohNode extends EventTarget {
    publicKey;
    secretKey;
    closed;
    #adapter;
    #endpointHandle;
    #nodeId;
    #nativeClosed;
    #resolveClose;
    #fetchFn;
    #serveFn;
    constructor(guard, adapter, info, _options, nativeClosed) {
        if (guard !== _INTERNAL) {
            throw new TypeError('IrohNode must be created via IrohNode._create()');
        }
        super();
        this.#adapter = adapter;
        this.#endpointHandle = info.endpointHandle;
        this.#nodeId = info.nodeId;
        this.#nativeClosed = nativeClosed;
        let resolveClose;
        this.closed = new Promise((r) => { resolveClose = r; });
        this.#resolveClose = resolveClose;
        nativeClosed.then(() => resolveClose({ closeCode: 0, reason: 'native shutdown' }));
        this.publicKey = keys_js_1.PublicKey.fromString(info.nodeId);
        this.secretKey = keys_js_1.SecretKey._fromBytesWithPublicKey(info.keypair, this.publicKey);
        this.#fetchFn = (0, fetch_js_1.makeFetch)(adapter, info.endpointHandle);
        this.#serveFn = (0, serve_js_1.makeServe)(adapter, info.endpointHandle, info.nodeId, this.closed.then(() => { }));
    }
    static _create(adapter, info, options, nativeClosed) {
        return new IrohNode(_INTERNAL, adapter, info, options, nativeClosed);
    }
    fetch(...args) {
        return this.#fetchFn(...args);
    }
    serve(...args) {
        return this.#serveFn(...args);
    }
    async connect(peer, init) {
        const sessionFns = this.#adapter.sessionFns;
        if (!sessionFns) {
            throw new Error('connect() not supported by this platform adapter');
        }
        const nodeId = (0, keys_js_1.resolveNodeId)(peer);
        const directAddrs = init?.directAddrs ?? null;
        const sessionHandle = await sessionFns.connect(this.#endpointHandle, nodeId, directAddrs);
        const remotePk = keys_js_1.PublicKey.fromString(nodeId);
        return (0, session_js_1.buildSession)(this.#adapter, sessionHandle, remotePk, sessionFns);
    }
    browse(options) {
        const adapter = this.#adapter;
        const handle = this.#endpointHandle;
        const svcName = options?.serviceName ?? 'iroh-http';
        const signal = options?.signal;
        return {
            [Symbol.asyncIterator]() {
                let browseHandle = null;
                return {
                    async next() {
                        if (browseHandle === null) {
                            browseHandle = await adapter.mdnsBrowse(handle, svcName);
                        }
                        if (signal?.aborted) {
                            adapter.mdnsBrowseClose(browseHandle);
                            browseHandle = null;
                            return { done: true, value: undefined };
                        }
                        let event;
                        if (signal) {
                            const abortPromise = new Promise((resolve) => {
                                if (signal.aborted) {
                                    resolve(null);
                                    return;
                                }
                                signal.addEventListener('abort', () => resolve(null), { once: true });
                            });
                            event = await Promise.race([
                                adapter.mdnsNextEvent(browseHandle),
                                abortPromise,
                            ]);
                            if (signal.aborted && browseHandle !== null) {
                                adapter.mdnsBrowseClose(browseHandle);
                                browseHandle = null;
                                return { done: true, value: undefined };
                            }
                        }
                        else {
                            event = await adapter.mdnsNextEvent(browseHandle);
                        }
                        if (event === null) {
                            return { done: true, value: undefined };
                        }
                        const discovered = {
                            nodeId: event.nodeId,
                            addrs: event.addrs ?? [],
                            isActive: event.type === 'discovered',
                        };
                        return { done: false, value: discovered };
                    },
                    return() {
                        if (browseHandle !== null) {
                            adapter.mdnsBrowseClose(browseHandle);
                            browseHandle = null;
                        }
                        return Promise.resolve({ done: true, value: undefined });
                    },
                };
            },
        };
    }
    async advertise(options) {
        const svcName = options?.serviceName ?? 'iroh-http';
        const signal = options?.signal;
        const advHandle = await this.#adapter.mdnsAdvertise(this.#endpointHandle, svcName);
        if (signal) {
            return new Promise((resolve) => {
                signal.addEventListener('abort', () => {
                    this.#adapter.mdnsAdvertiseClose(advHandle);
                    resolve();
                }, { once: true });
                if (signal.aborted) {
                    this.#adapter.mdnsAdvertiseClose(advHandle);
                    resolve();
                }
            });
        }
    }
    async addr() {
        return this.#adapter.nodeAddr(this.#endpointHandle);
    }
    async ticket() {
        return this.#adapter.nodeTicket(this.#endpointHandle);
    }
    async homeRelay() {
        return this.#adapter.homeRelay(this.#endpointHandle);
    }
    async peerInfo(peer) {
        return this.#adapter.peerInfo(this.#endpointHandle, (0, keys_js_1.resolveNodeId)(peer));
    }
    async peerStats(peer) {
        return this.#adapter.peerStats(this.#endpointHandle, (0, keys_js_1.resolveNodeId)(peer));
    }
    async stats() {
        return this.#adapter.stats(this.#endpointHandle);
    }
    pathChanges(peer, pollIntervalMs = 500) {
        const nodeId = (0, keys_js_1.resolveNodeId)(peer);
        const adapter = this.#adapter;
        const endpointHandle = this.#endpointHandle;
        let cancelled = false;
        let lastPath = null;
        return new ReadableStream({
            async pull(controller) {
                while (!cancelled) {
                    const stats = await adapter.peerStats(endpointHandle, nodeId).catch(() => null);
                    if (cancelled)
                        break;
                    if (stats) {
                        const selected = stats.paths.find((p) => p.active);
                        if (selected) {
                            const key = `${selected.relay}:${selected.addr}`;
                            if (key !== lastPath) {
                                lastPath = key;
                                controller.enqueue(selected);
                                return;
                            }
                        }
                    }
                    await new Promise((resolve) => setTimeout(resolve, pollIntervalMs));
                }
                controller.close();
            },
            cancel() {
                cancelled = true;
            },
        });
    }
    async close(options) {
        await this.#adapter.closeEndpoint(this.#endpointHandle, options?.force);
        this.#resolveClose({ closeCode: 0, reason: '' });
        await this.#nativeClosed;
    }
    [Symbol.asyncDispose]() {
        return this.close();
    }
}
exports.IrohNode = IrohNode;
//# sourceMappingURL=IrohNode.js.map