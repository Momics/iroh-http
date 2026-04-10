/**
 * Bridge interface — the only thing that differs between Node.js and Tauri.
 *
 * Each platform (iroh-http-node / iroh-http-tauri) implements exactly these
 * three methods.  All higher-level logic lives in iroh-http-shared and is
 * independent of the underlying transport.
 */
export interface Bridge {
  /**
   * Pull the next chunk from a body reader identified by `handle`.
   * Returns `null` when the body is fully consumed (EOF).
   */
  nextChunk(handle: number): Promise<Uint8Array | null>;

  /**
   * Push `chunk` into a body writer identified by `handle`.
   */
  sendChunk(handle: number, chunk: Uint8Array): Promise<void>;

  /**
   * Signal end-of-body for the writer at `handle`.
   * After this call the writer is invalid and the associated reader will
   * eventually return `null` from `nextChunk`.
   */
  finishBody(handle: number): Promise<void>;
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
}

// ── Platform function types ───────────────────────────────────────────────────

/** Options accepted by `createNode`. */
export interface NodeOptions {
  /** 32-byte Ed25519 secret key.  Omit to generate a new identity. */
  key?: Uint8Array;
  /** Idle connection timeout in milliseconds. */
  idleTimeout?: number;
  /** Custom relay server URLs. */
  relays?: string[];
  /** DNS discovery server URL. */
  dnsDiscovery?: string;
}

/** The object returned by `createNode`. */
export interface IrohNode {
  /** The node's public key — its stable network address. */
  nodeId: string;
  /** The raw 32-byte secret key — persist this to restore identity. */
  keypair: Uint8Array;
  /**
   * Send an HTTP request to a remote node.
   * Signature mirrors `globalThis.fetch` with `nodeId` prepended.
   */
  fetch(
    nodeId: string,
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
  /** Close the endpoint and release resources. */
  close(): Promise<void>;
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
  reqBodyHandle: number | null
) => Promise<FfiResponse>;

/** Allocate a body writer handle (may be sync or async). */
export type AllocBodyWriterFn = () => number | Promise<number>;
