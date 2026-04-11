/**
 * `IrohSession` — a WebTransport-compatible session to a single remote peer.
 *
 * Created via `node.connect(peer)`.  Wraps a QUIC connection and exposes
 * bidirectional streams through the standard WebTransport interface.
 */

import type { Bridge, FfiDuplexStream } from "./bridge.js";
import type { PublicKey } from "./keys.js";
import { makeReadable } from "./streams.js";

/** WebTransport-compatible bidirectional stream. */
export interface WebTransportBidirectionalStream {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
}

/** Raw session FFI functions provided by each platform adapter. */
export interface RawSessionFns {
  /** Establish a session to a remote peer. Returns an opaque session handle. */
  connect(endpointHandle: number, nodeId: string, directAddrs: string[] | null): Promise<number>;
  /** Open a new bidi stream on an existing session. */
  createBidiStream(sessionHandle: number): Promise<FfiDuplexStream>;
  /** Accept the next incoming bidi stream. Returns `null` when the session closes. */
  nextBidiStream(sessionHandle: number): Promise<FfiDuplexStream | null>;
  /** Close the session. */
  close(sessionHandle: number): Promise<void>;
}

/**
 * A session to a single remote peer.
 *
 * Mirrors the `WebTransport` interface for bidirectional streams.
 */
export interface IrohSession {
  /** The remote peer's public key. */
  readonly remoteId: PublicKey;

  /**
   * Open a new bidirectional stream.
   *
   * Returns a `WebTransportBidirectionalStream` with `.readable` and `.writable`.
   */
  createBidirectionalStream(): Promise<WebTransportBidirectionalStream>;

  /**
   * Incoming bidirectional streams opened by the remote peer.
   *
   * Yields streams as the remote opens them.  The `ReadableStream` closes
   * when the session ends.
   */
  readonly incomingBidirectionalStreams: ReadableStream<WebTransportBidirectionalStream>;

  /** Resolves when the session is fully closed. */
  readonly closed: Promise<void>;

  /** Close the session. */
  close(): Promise<void>;

  /** TC39 explicit resource management. */
  [Symbol.asyncDispose](): Promise<void>;
}

/**
 * Build an `IrohSession` from raw platform handles.
 */
export function buildSession(
  bridge: Bridge,
  sessionHandle: number,
  remoteId: PublicKey,
  rawSession: RawSessionFns,
): IrohSession {
  let resolveClosed!: () => void;
  const closedPromise = new Promise<void>((resolve) => {
    resolveClosed = resolve;
  });

  function wrapDuplex(ffi: FfiDuplexStream): WebTransportBidirectionalStream {
    const readable = makeReadable(bridge, ffi.readHandle);
    const writable = new WritableStream<Uint8Array>({
      async write(chunk) {
        await bridge.sendChunk(ffi.writeHandle, chunk);
      },
      async close() {
        await bridge.finishBody(ffi.writeHandle);
      },
      async abort() {
        await bridge.finishBody(ffi.writeHandle);
      },
    });
    return { readable, writable };
  }

  // Lazy incoming bidi streams — only one ReadableStream instance.
  let _incomingStreams: ReadableStream<WebTransportBidirectionalStream> | null = null;

  const session: IrohSession = {
    remoteId,

    async createBidirectionalStream(): Promise<WebTransportBidirectionalStream> {
      const ffi = await rawSession.createBidiStream(sessionHandle);
      return wrapDuplex(ffi);
    },

    get incomingBidirectionalStreams(): ReadableStream<WebTransportBidirectionalStream> {
      if (!_incomingStreams) {
        _incomingStreams = new ReadableStream<WebTransportBidirectionalStream>({
          async pull(controller) {
            const ffi = await rawSession.nextBidiStream(sessionHandle);
            if (ffi === null) {
              controller.close();
            } else {
              controller.enqueue(wrapDuplex(ffi));
            }
          },
        });
      }
      return _incomingStreams;
    },

    closed: closedPromise,

    async close(): Promise<void> {
      await rawSession.close(sessionHandle);
      resolveClosed();
    },

    [Symbol.asyncDispose]() {
      return session.close();
    },
  };

  return session;
}
