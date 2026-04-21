/**
 * `IrohSession` — a WebTransport-compatible session to a single remote peer.
 *
 * Created via `node.connect(peer)`.  Wraps a QUIC connection and exposes
 * bidirectional streams, unidirectional streams, and datagrams through
 * the standard WebTransport interface.
 */
import type { Bridge, FfiDuplexStream } from "./bridge.js";
import type { PublicKey } from "./keys.js";
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
    connect(endpointHandle: number, nodeId: string, directAddrs: string[] | null): Promise<bigint>;
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
    close(sessionHandle: bigint, closeCode?: number, reason?: string): Promise<void>;
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
    readonly incomingBidirectionalStreams: ReadableStream<WebTransportBidirectionalStream>;
    /**
     * Incoming unidirectional streams from the remote peer.
     */
    readonly incomingUnidirectionalStreams: ReadableStream<ReadableStream<Uint8Array>>;
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
export declare function buildSession(bridge: Bridge, sessionHandle: bigint, remoteId: PublicKey, rawSession: RawSessionFns): IrohSession;
//# sourceMappingURL=session.d.ts.map