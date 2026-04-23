/**
 * `IrohSession` — a WebTransport-compatible session to a single remote peer.
 *
 * Created via `node.connect(peer)`.  Wraps a QUIC connection and exposes
 * bidirectional streams, unidirectional streams, and datagrams through
 * the standard WebTransport interface.
 */

import type { FfiDuplexStream, IrohAdapter } from "./IrohAdapter.js";
import type { PublicKey } from "./keys.js";
import { makeReadable } from "./streams.js";

/** WebTransport-compatible bidirectional stream. */
export interface WebTransportBidirectionalStream {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
}

/** WebTransport close info. */
export interface WebTransportCloseInfo {
  closeCode: number;
  reason: string;
}

/** WebTransport datagram duplex stream. */
export interface WebTransportDatagramDuplexStream {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
  readonly maxDatagramSize: number | null;
  incomingHighWaterMark: number;
  outgoingHighWaterMark: number;
}

/** Raw session FFI functions provided by each platform adapter. */
export interface RawSessionFns {
  /** Establish a session to a remote peer. Returns an opaque session handle. */
  connect(
    endpointHandle: number,
    nodeId: string,
    directAddrs: string[] | null,
  ): Promise<bigint>;
  /**
   * Accept an incoming session from a remote peer.
   *
   * Blocks until a peer opens a raw QUIC connection.  Returns the session
   * handle and the remote peer's node ID string, or `null` when the endpoint
   * is shutting down.
   */
  sessionAccept?(
    endpointHandle: number,
  ): Promise<{ sessionHandle: bigint; nodeId: string } | null>;
  /** Open a new bidi stream on an existing session. */
  createBidiStream(sessionHandle: bigint): Promise<FfiDuplexStream>;
  /** Accept the next incoming bidi stream. Returns `null` when the session closes. */
  nextBidiStream(sessionHandle: bigint): Promise<FfiDuplexStream | null>;
  /** Open a new unidirectional (send-only) stream. Returns a write handle. */
  createUniStream(sessionHandle: bigint): Promise<bigint>;
  /** Accept the next incoming unidirectional (receive-only) stream. Returns a read handle, or `null` when closed. */
  nextUniStream(sessionHandle: bigint): Promise<bigint | null>;
  /** Send a datagram. */
  sendDatagram(sessionHandle: bigint, data: Uint8Array): Promise<void>;
  /** Receive the next datagram. Returns `null` when the session closes. */
  recvDatagram(sessionHandle: bigint): Promise<Uint8Array | null>;
  /** Get the maximum datagram payload size. Returns `null` if datagrams are unsupported. */
  maxDatagramSize(sessionHandle: bigint): Promise<number | null>;
  /** Wait for the session to close. Returns close info. */
  closed(sessionHandle: bigint): Promise<WebTransportCloseInfo>;
  /** Close the session with an optional close code and reason. */
  close(
    sessionHandle: bigint,
    closeCode?: number,
    reason?: string,
  ): Promise<void>;
}

/**
 * A session to a single remote peer.
 *
 * Implements the WebTransport interface.
 */
export interface IrohSession {
  /** The remote peer's public key. */
  readonly remoteId: PublicKey;

  /** Resolves when the QUIC handshake completes. */
  readonly ready: Promise<undefined>;

  /**
   * Open a new bidirectional stream.
   */
  createBidirectionalStream(): Promise<WebTransportBidirectionalStream>;

  /**
   * Open a new unidirectional (send-only) stream.
   */
  createUnidirectionalStream(): Promise<WritableStream<Uint8Array>>;

  /**
   * Incoming bidirectional streams opened by the remote peer.
   */
  readonly incomingBidirectionalStreams: ReadableStream<
    WebTransportBidirectionalStream
  >;

  /**
   * Incoming unidirectional streams from the remote peer.
   */
  readonly incomingUnidirectionalStreams: ReadableStream<
    ReadableStream<Uint8Array>
  >;

  /** Datagram duplex stream for unreliable message passing. */
  readonly datagrams: WebTransportDatagramDuplexStream;

  /** Resolves when the session is fully closed, with close code and reason. */
  readonly closed: Promise<WebTransportCloseInfo>;

  /** Close the session with an optional close code and reason. */
  close(closeInfo?: WebTransportCloseInfo): void;

  /** TC39 explicit resource management. */
  [Symbol.asyncDispose](): Promise<void>;
}

/**
 * Build an `IrohSession` from raw platform handles.
 */
export function buildSession(
  adapter: IrohAdapter,
  sessionHandle: bigint,
  remoteId: PublicKey,
  rawSession: RawSessionFns,
): IrohSession {
  // The session_closed promise from the native side.
  const closedPromise = rawSession.closed(sessionHandle);

  function wrapDuplex(ffi: FfiDuplexStream): WebTransportBidirectionalStream {
    const readable = makeReadable(adapter, ffi.readHandle);
    const writable = new WritableStream<Uint8Array>({
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
  let _incomingBidiStreams:
    | ReadableStream<WebTransportBidirectionalStream>
    | null = null;

  // Lazy incoming uni streams.
  let _incomingUniStreams: ReadableStream<ReadableStream<Uint8Array>> | null =
    null;

  // Lazy datagram duplex stream.
  let _datagrams: WebTransportDatagramDuplexStream | null = null;

  const session: IrohSession = {
    remoteId,

    ready: Promise.resolve(undefined),

    async createBidirectionalStream(): Promise<
      WebTransportBidirectionalStream
    > {
      const ffi = await rawSession.createBidiStream(sessionHandle);
      return wrapDuplex(ffi);
    },

    async createUnidirectionalStream(): Promise<WritableStream<Uint8Array>> {
      const writeHandle = await rawSession.createUniStream(sessionHandle);
      return new WritableStream<Uint8Array>({
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

    get incomingBidirectionalStreams(): ReadableStream<
      WebTransportBidirectionalStream
    > {
      if (!_incomingBidiStreams) {
        _incomingBidiStreams = new ReadableStream<
          WebTransportBidirectionalStream
        >({
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
      return _incomingBidiStreams;
    },

    get incomingUnidirectionalStreams(): ReadableStream<
      ReadableStream<Uint8Array>
    > {
      if (!_incomingUniStreams) {
        _incomingUniStreams = new ReadableStream<ReadableStream<Uint8Array>>({
          async pull(controller) {
            const readHandle = await rawSession.nextUniStream(sessionHandle);
            if (readHandle === null) {
              controller.close();
            } else {
              controller.enqueue(makeReadable(adapter, readHandle));
            }
          },
        });
      }
      return _incomingUniStreams;
    },

    get datagrams(): WebTransportDatagramDuplexStream {
      if (!_datagrams) {
        const readable = new ReadableStream<Uint8Array>({
          async pull(controller) {
            const data = await rawSession.recvDatagram(sessionHandle);
            if (data === null) {
              controller.close();
            } else {
              controller.enqueue(data);
            }
          },
        });

        const writable = new WritableStream<Uint8Array>({
          async write(chunk) {
            await rawSession.sendDatagram(sessionHandle, chunk);
          },
        });

        let maxSize: number | null = null;
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

    close(closeInfo?: WebTransportCloseInfo): void {
      void rawSession
        .close(
          sessionHandle,
          closeInfo?.closeCode ?? 0,
          closeInfo?.reason || undefined,
        )
        .catch(() => {});
    },

    [Symbol.asyncDispose]() {
      session.close();
      return closedPromise.then(() => {});
    },
  };

  return session;
}
