"use strict";
/**
 * `IrohSession` — a WebTransport-compatible session to a single remote peer.
 *
 * Created via `node.connect(peer)`.  Wraps a QUIC connection and exposes
 * bidirectional streams, unidirectional streams, and datagrams through
 * the standard WebTransport interface.
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.buildSession = buildSession;
const streams_js_1 = require("./streams.js");
/**
 * Build an `IrohSession` from raw platform handles.
 */
function buildSession(adapter, sessionHandle, remoteId, rawSession) {
    // The session_closed promise from the native side.
    const closedPromise = rawSession.closed(sessionHandle);
    function wrapDuplex(ffi) {
        const readable = (0, streams_js_1.makeReadable)(adapter, ffi.readHandle);
        const writable = new WritableStream({
            async write(chunk) {
                await adapter.sendChunk(ffi.writeHandle, chunk);
            },
            async close() {
                await adapter.finishBody(ffi.writeHandle);
            },
            async abort() {
                await adapter.finishBody(ffi.writeHandle);
            },
        });
        return { readable, writable };
    }
    // Lazy incoming bidi streams — only one ReadableStream instance.
    let _incomingBidiStreams = null;
    // Lazy incoming uni streams.
    let _incomingUniStreams = null;
    // Lazy datagram duplex stream.
    let _datagrams = null;
    const session = {
        remoteId,
        ready: Promise.resolve(undefined),
        async createBidirectionalStream() {
            const ffi = await rawSession.createBidiStream(sessionHandle);
            return wrapDuplex(ffi);
        },
        async createUnidirectionalStream() {
            const writeHandle = await rawSession.createUniStream(sessionHandle);
            return new WritableStream({
                async write(chunk) {
                    await adapter.sendChunk(writeHandle, chunk);
                },
                async close() {
                    await adapter.finishBody(writeHandle);
                },
                async abort() {
                    await adapter.finishBody(writeHandle);
                },
            });
        },
        get incomingBidirectionalStreams() {
            if (!_incomingBidiStreams) {
                _incomingBidiStreams = new ReadableStream({
                    async pull(controller) {
                        const ffi = await rawSession.nextBidiStream(sessionHandle);
                        if (ffi === null) {
                            controller.close();
                        }
                        else {
                            controller.enqueue(wrapDuplex(ffi));
                        }
                    },
                });
            }
            return _incomingBidiStreams;
        },
        get incomingUnidirectionalStreams() {
            if (!_incomingUniStreams) {
                _incomingUniStreams = new ReadableStream({
                    async pull(controller) {
                        const readHandle = await rawSession.nextUniStream(sessionHandle);
                        if (readHandle === null) {
                            controller.close();
                        }
                        else {
                            controller.enqueue((0, streams_js_1.makeReadable)(adapter, readHandle));
                        }
                    },
                });
            }
            return _incomingUniStreams;
        },
        get datagrams() {
            if (!_datagrams) {
                const readable = new ReadableStream({
                    async pull(controller) {
                        const data = await rawSession.recvDatagram(sessionHandle);
                        if (data === null) {
                            controller.close();
                        }
                        else {
                            controller.enqueue(data);
                        }
                    },
                });
                const writable = new WritableStream({
                    async write(chunk) {
                        await rawSession.sendDatagram(sessionHandle, chunk);
                    },
                });
                let maxSize = null;
                // Eagerly fetch max datagram size (fire-and-forget).
                void rawSession.maxDatagramSize(sessionHandle).then((s) => {
                    maxSize = s;
                });
                _datagrams = {
                    readable,
                    writable,
                    get maxDatagramSize() {
                        return maxSize;
                    },
                    incomingHighWaterMark: 1,
                    outgoingHighWaterMark: 1,
                };
            }
            return _datagrams;
        },
        closed: closedPromise,
        close(closeInfo) {
            void rawSession
                .close(sessionHandle, closeInfo?.closeCode ?? 0, closeInfo?.reason || undefined)
                .catch(() => { });
        },
        [Symbol.asyncDispose]() {
            session.close();
            return closedPromise.then(() => { });
        },
    };
    return session;
}
//# sourceMappingURL=session.js.map