/**
 * Bridge interface — the only thing that differs between Node.js and Tauri.
 *
 * Each platform (iroh-http-node / iroh-http-tauri) implements exactly these
 * methods.  All higher-level logic lives in iroh-http-shared and is
 * independent of the underlying transport.
 */

import type { PublicKey, SecretKey } from "./keys.js";

export interface Bridge {
  // ── Body streaming ─────────────────────────────────────────────────────────
  /** Pull the next chunk from a body reader. Returns `null` at EOF. */
  nextChunk(handle: number): Promise<Uint8Array | null>;
  /** Push `chunk` into a body writer. */
  sendChunk(handle: number, chunk: Uint8Array): Promise<void>;
  /** Signal end-of-body for the writer at `handle`. */
  finishBody(handle: number): Promise<void>;

  // ── §3 AbortSignal cancellation ────────────────────────────────────────────
  /** Drop a body reader from the Rust slab, cancelling an in-flight fetch. */
  cancelRequest(handle: number): Promise<void>;  /**
   * Allocate an in-flight cancellation token in the Rust fetch map.
   * Call this before `rawFetch` and wire abort → `cancelFetch(token)`.
   */
  allocFetchToken(): Promise<number>;
  /**
   * Signal the Rust fetch task to abort.  Safe to call after the fetch has
   * already completed.  Fire-and-forget (do not await).
   */
  cancelFetch(token: number): void;
  // ── §4 Trailer headers ──────────────────────────────────────────────────────
  /**
   * Await and retrieve trailers produced after the body is consumed.
   * Returns `null` when no trailers were sent.
   */
  nextTrailer(handle: number): Promise<[string, string][] | null>;
  /**
   * Deliver response trailers from the JS handler to the Rust server task.
   * Call after `finishBody`. This must be called exactly once per
   * `resTrailersHandle`; calling it resolves the waiting pump task.
   */
  sendTrailers(handle: number, trailers: [string, string][]): Promise<void>;
}

// ── FFI data types ────────────────────────────────────────────────────────────

/**
 * Raw request data as it crosses the FFI boundary.
 * The `iroh-node-id` header has already been stripped by the Rust layer.
 */
export interface FfiRequest {
  /** HTTP method, e.g. `"GET"`. */
  method: string;
  /**
   * Full `http+iroh://<server-node-id>/path` URL.
   * Use `new URL(req.url).pathname` for routing.
   */
  url: string;
  /** Request headers as `[name, value]` pairs. */
  headers: [string, string][];
  /** Authenticated remote peer identity from the QUIC connection. */
  remoteNodeId: string;
}

/** Response head returned from a user handler via the serve callback. */
export interface FfiResponseHead {
  status: number;
  headers: [string, string][];
}

/** Full response object returned by the low-level rawFetch. */
export interface FfiResponse extends FfiResponseHead {
  /** Handle to the response body reader. */
  bodyHandle: number;
  /** Full `http+iroh://` URL of the responding peer. */
  url: string;
  /** Handle to the response trailer receiver (pass to `bridge.nextTrailer`). */
  trailersHandle: number;
}

/**
 * Payload delivered to the per-request callback in `rawServe`.
 *
 * JS reads the request body via `nextChunk(reqBodyHandle)` and writes the
 * response body via `sendChunk(resBodyHandle, …)` + `finishBody(resBodyHandle)`.
 */
export interface RequestPayload extends FfiRequest {
  /** Opaque handle — pass to `respondToRequest` on the Tauri bridge. */
  reqHandle: number;
  /** Body reader handle for the request body. */
  reqBodyHandle: number;
  /** Body writer handle for the response body. */
  resBodyHandle: number;
  /** Trailer receiver handle — JS calls `bridge.nextTrailer(reqTrailersHandle)` to read request trailers. `0` in duplex mode. */
  reqTrailersHandle: number;
  /** Trailer sender handle — JS calls `bridge.sendTrailers(resTrailersHandle, pairs)` to send response trailers. `0` in duplex mode. */
  resTrailersHandle: number;
  /** True when the client sent `Upgrade: iroh-duplex`. */
  isBidi: boolean;
}

// ── Platform function types ───────────────────────────────────────────────────

/** Options accepted by `createNode`. */
export interface NodeOptions {
  /** 32-byte Ed25519 secret key or `SecretKey` object.  Omit to generate a new identity. */
  key?: SecretKey | Uint8Array;
  /** Idle connection timeout in milliseconds. */
  idleTimeout?: number;
  /** Custom relay server URLs. */
  relays?: string[];
  /** DNS discovery server URL. */
  dnsDiscovery?: string;
  /** Capacity (in chunks) of each body channel.  Default: 32. */
  channelCapacity?: number;
  /** Maximum byte length of a single chunk.  Larger chunks are split.  Default: 65536 (64 KB). */
  maxChunkSizeBytes?: number;
  /** Number of consecutive accept errors before the serve loop gives up.  Default: 5. */
  maxConsecutiveErrors?: number;
}

/** The object returned by `createNode`. */
export interface IrohNode {
  /**
   * The node's public identity.
   */
  publicKey: PublicKey;
  /**
   * The node's secret key — persist `secretKey.toBytes()` to restore identity
   * across restarts.
   */
  secretKey: SecretKey;
  /** @deprecated Use `publicKey.toString()` instead. */
  nodeId: string;
  /** @deprecated Use `secretKey.toBytes()` instead. */
  keypair: Uint8Array;
  /**
   * Send an HTTP request to a remote node.
   * Signature mirrors `globalThis.fetch` with `peer` prepended.
   */
  fetch(
    peer: PublicKey | string,
    input: string | URL,
    init?: RequestInit
  ): Promise<Response>;
  /**
   * Start listening for incoming HTTP requests.
   * Deno-compatible `serve` signature.
   */
  serve(
    options: Record<string, unknown>,
    handler: (req: Request) => Response | Promise<Response>
  ): void;
  /**
   * Open a bidirectional streaming connection to a remote node (§2).
   *
   * The peer must advertise `iroh-http/1-duplex` capability.  After the
   * handshake both sides can read and write concurrently without waiting for
   * the other to finish.  Mirrors `WebTransportSession.createBidirectionalStream()`.
   */
  createBidirectionalStream(peer: PublicKey | string, path: string, init?: RequestInit): Promise<BidirectionalStream>;
  /**
   * Resolves when the node has been closed (either via `close()` or due to
   * a fatal error).  Mirrors `WebTransportSession.closed`.
   */
  readonly closed: Promise<void>;
  /** Close the endpoint and release resources. */
  close(): Promise<void>;
  /** Enables `await using node = await createNode()` (TC39 explicit resource management). */
  [Symbol.asyncDispose](): Promise<void>;
}

/** Result of the low-level `createEndpoint` FFI call. */
export interface EndpointInfo {
  endpointHandle: number;
  nodeId: string;
  keypair: Uint8Array;
}

/** Raw serve function provided by each platform bridge. */
export type RawServeFn = (
  endpointHandle: number,
  options: Record<string, unknown>,
  callback: (payload: RequestPayload) => Promise<FfiResponseHead>
) => void;

/** Raw fetch function provided by each platform bridge. */
export type RawFetchFn = (
  endpointHandle: number,
  nodeId: string,
  url: string,
  method: string,
  headers: [string, string][],
  reqBodyHandle: number | null,
  fetchToken: number
) => Promise<FfiResponse>;

/** Allocate a body writer handle (may be sync or async). */
export type AllocBodyWriterFn = () => number | Promise<number>;

// ── §2 Bidirectional streaming types ─────────────────────────────────────────

/** Raw duplex stream handles returned by `rawConnect`. */
export interface FfiDuplexStream {
  /** Handle for reading data sent by the server. */
  readHandle: number;
  /** Handle for writing data to the server. */
  writeHandle: number;
}

/** Full-duplex stream returned by `node.createBidirectionalStream()`. Mirrors `WebTransportBidirectionalStream`. */
export interface BidirectionalStream {
  /** Receive data from the server. */
  readable: ReadableStream<Uint8Array>;
  /** Send data to the server. */
  writable: WritableStream<Uint8Array>;
}

/** @deprecated Use {@link BidirectionalStream} instead. */
export type DuplexStream = BidirectionalStream;

/** Raw connect function provided by each platform bridge. */
export type RawConnectFn = (
  endpointHandle: number,
  nodeId: string,
  path: string,
  headers: [string, string][],
) => Promise<FfiDuplexStream>;
