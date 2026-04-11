/**
 * Bridge interface — the only thing that differs between Node.js and Tauri.
 *
 * Each platform (iroh-http-node / iroh-http-tauri) implements exactly these
 * methods.  All higher-level logic lives in iroh-http-shared and is
 * independent of the underlying transport.
 */

import type { PublicKey, SecretKey } from "./keys.js";
import type { ServeHandler, ServeOptions, ServeHandle } from "./serve.js";
import type { IrohSession, WebTransportCloseInfo } from "./session.js";

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
   * Full `httpi://<server-node-id>/path` URL.
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
  /** Full `httpi://` URL of the responding peer. */
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

/** Options for discovery configuration. */
export type DiscoveryOptions = {
  /**
   * Enable DNS discovery via n0's DNS servers.  Default: true.
   * Set to `false` for LAN-only deployments.
   */
  dns?: boolean;
};

/** Options for mDNS browse/advertise calls. */
export interface MdnsOptions {
  /** Application-specific mDNS service name.  Default: `'iroh-http'`. */
  serviceName?: string;
}

/** Options for mobile/background lifecycle management. */
export interface LifecycleOptions {
  /** Automatically reconnect if the endpoint goes dead.  Default: false. */
  autoReconnect?: boolean;
  /** Maximum reconnect attempts before marking the node dead.  Default: 3. */
  maxRetries?: number;
}

/**
 * Relay server configuration.
 *
 * Relays are QUIC-over-HTTPS servers that keep peers reachable behind NATs/firewalls.
 *
 *   `"default"`       — n0's public production relays (recommended).
 *   `"staging"`       — n0's canary relays (for testing pre-release infra).
 *   `"disabled"`      — No relay, no DNS discovery. Direct addresses only.
 *   `"https://…"`     — A single custom relay URL.
 *   `["https://…", …]` — Multiple custom relay URLs.
 */
export type RelayMode =
  | "default"
  | "staging"
  | "disabled"
  | string
  | string[];

/**
 * Options accepted by `createNode`.
 *
 * @example Basic usage — generate a new identity:
 * ```ts
 * const node = await createNode();
 * console.log(node.publicKey.toString()); // base32 node ID
 * ```
 *
 * @example Restore a saved identity:
 * ```ts
 * const node = await createNode({ key: savedKeyBytes });
 * ```
 *
 * @example Custom relay + mDNS discovery:
 * ```ts
 * const node = await createNode({
 *   relayMode: "https://my-relay.example.com",
 * });
 * // Use node.browse() / node.advertise() for mDNS peer discovery.
 * ```
 */
export interface NodeOptions {
  // ── Identity ─────────────────────────────────────────────────────────────
  /** 32-byte Ed25519 secret key or `SecretKey` object.  Omit to generate a new identity. @default undefined (new keypair) */
  key?: SecretKey | Uint8Array;

  // ── Connectivity ──────────────────────────────────────────────────────────
  /**
   * Relay server configuration. Default: `"default"`.
   *
   * Set to `"disabled"` for fully offline/direct-only mode (also disables DNS
   * discovery). Pass a URL string or array for custom relay servers.
   *
   * @see {@link RelayMode}
   */
  relayMode?: RelayMode;
  /**
   * Bind the UDP socket on a specific address and/or port.
   *
   * Default: OS-assigned port on all interfaces (`"0.0.0.0:0"`).
   * Accepts a single address or an array for multi-socket binding.
   *
   * @example `"192.168.1.5:0"`, `["0.0.0.0:0", "[::]:0"]`
   */
  bindAddr?: string | string[];
  /** Idle connection timeout in milliseconds. @default 60000 */
  idleTimeout?: number;

  // ── Discovery ─────────────────────────────────────────────────────────────
  /** DNS discovery server URL override.  Uses n0 DNS defaults when unset. */
  dnsDiscovery?: string;
  /** Local peer discovery configuration. */
  discovery?: DiscoveryOptions;

  // ── Power-user options ────────────────────────────────────────────────────
  //
  // Leave unset unless you have a specific reason. Incorrect values can
  // silently break connectivity or degrade performance.
  /**
   * HTTP proxy URL for relay traffic.  For corporate networks that route
   * UDP through an HTTP proxy.
   */
  proxyUrl?: string;
  /**
   * Read `HTTP_PROXY` / `HTTPS_PROXY` environment variables for proxy config.
   * Default: false.
   */
  proxyFromEnv?: boolean;
  /**
   * Log TLS pre-master session keys to `$SSLKEYLOGFILE`.
   * **DEV ONLY** — enables Wireshark decryption. Never enable in production.
   * Default: false.
   */
  keylog?: boolean;
  /** Capacity (in chunks) of each body channel.  Default: 32. */
  channelCapacity?: number;
  /** Maximum byte length of a single chunk.  Larger chunks are split.  Default: 65536 (64 KB). */
  maxChunkSizeBytes?: number;
  /** Number of consecutive accept errors before the serve loop gives up.  Default: 5. */
  maxConsecutiveErrors?: number;
  /** Milliseconds to wait for a slow body reader before dropping.  Default: 30 000. */
  drainTimeout?: number;
  /** TTL in milliseconds for slab handle entries.  `0` disables sweeping.  Default: 300 000. */
  handleTtl?: number;

  // ── Compression ───────────────────────────────────────────────────────────
  /**
   * Enable zstd body compression.
   *
   * - `true` — enable with default settings (level 3, 512 B threshold).
   * - `false` or omitted — no compression (default).
   * - Object — enable with custom settings.
   *
   * Requires the Rust `compression` feature to be compiled in.
   */
  compression?: boolean | {
    /** zstd compression level 1–22.  Default: 3. */
    level?: number;
    /** Skip compression for bodies smaller than this many bytes.  Default: 512. */
    minBodyBytes?: number;
  };

  // ── Server limits ─────────────────────────────────────────────────────────
  /**
   * Maximum simultaneous in-flight requests, all peers combined.
   * @default 64
   */
  maxConcurrency?: number;

  /**
   * Maximum simultaneous connections from a single peer.
   * @default 8
   */
  maxConnectionsPerPeer?: number;

  /**
   * Per-request timeout in milliseconds.  Set to `0` to disable.
   * @default 60000
   */
  requestTimeout?: number;

  /**
   * Reject request bodies larger than this many bytes with 413.
   * `undefined` means unlimited (default).
   */
  maxRequestBodyBytes?: number;

  /**
   * Maximum header block size in bytes.  Requests with headers exceeding
   * this limit are rejected.
   * @default 65536
   */
  maxHeaderBytes?: number;

  // ── Mobile / background lifecycle ─────────────────────────────────────────
  /** Mobile/background lifecycle options. */
  lifecycle?: LifecycleOptions;
}

/**
 * Extended `RequestInit` for iroh-http fetch.
 *
 * Adds iroh-specific options alongside the standard web fetch init.
 * Unknown properties are ignored by the web standard, so this is
 * forward-compatible with plain `RequestInit`.
 */
export interface IrohFetchInit extends RequestInit {
  /**
   * Direct socket addresses to try when connecting to this peer.
   *
   * When set, the client will attempt to connect directly to these addresses
   * instead of relying solely on DNS/relay discovery.  Useful for tests or
   * when the peer's address is already known out-of-band.
   *
   * Each entry must be an `"ip:port"` string, e.g. `"127.0.0.1:12345"`.
   */
  directAddrs?: string[];
}

/**
 * The object returned by `createNode`.
 *
 * @example Fetch from a peer:
 * ```ts
 * const node = await createNode();
 * const res = await node.fetch(peerId, '/api/data');
 * console.log(await res.json());
 * ```
 *
 * @example Serve requests:
 * ```ts
 * const server = node.serve((req) => {
 *   const peer = req.headers.get('iroh-node-id');
 *   return Response.json({ hello: peer });
 * });
 * await server.finished;
 * ```
 *
 * @example Automatic cleanup with `await using` (TC39):
 * ```ts
 * await using node = await createNode();
 * // node.close() is called automatically when leaving scope.
 * ```
 */
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
   *
   * Pass `directAddrs` in init to provide known socket addresses for the peer
   * (useful in tests or when addresses are already known out-of-band).
   *
   * @param peer - Remote node's public key or base32 node ID string.
   * @param input - Request URL path, e.g. `"/api/data"` or `"httpi://nodeId/path"`.
   * @param init - Standard `RequestInit` options plus iroh-specific `directAddrs`.
   * @returns A standard `Response` with an additional `trailers` promise.
   * @throws {IrohConnectError} If the peer is unreachable.
   * @throws {IrohAbortError} If `init.signal` is aborted.
   */
  fetch(
    peer: PublicKey | string,
    input: string | URL,
    init?: IrohFetchInit
  ): Promise<Response>;
  /**
   * Start listening for incoming HTTP requests.
   *
   * Supports three call signatures:
   * - `serve(handler)` — handler only (most common)
   * - `serve(options, handler)` — options + handler
   * - `serve({ handler, ...options })` — handler inside options object
   *
   * Returns a `ServeHandle` whose `finished` promise resolves when the
   * serve loop terminates.
   */
  serve: {
    (handler: ServeHandler): ServeHandle;
    (options: ServeOptions, handler: ServeHandler): ServeHandle;
    (options: ServeOptions & { handler: ServeHandler }): ServeHandle;
  };
  /**
   * Open a session (QUIC connection) to a remote peer.
   *
   * Returns an `IrohSession` that mirrors the WebTransport API:
   * `session.createBidirectionalStream()`, `session.incomingBidirectionalStreams`,
   * `session.close()`, `session.closed`.
   *
   * Sessions are pooled — calling `connect` for the same peer may reuse an
   * existing QUIC connection.
   *
   * @param peer - Remote node's public key or base32 node ID string.
   * @param init - Optional configuration with `directAddrs`.
   * @returns An `IrohSession` scoped to the remote peer.
   * @throws {IrohConnectError} If the peer is unreachable.
   */
  connect(peer: PublicKey | string, init?: { directAddrs?: string[] }): Promise<IrohSession>;
  /**
   * Discover peers on the local network via mDNS.
   *
   * Returns an `AsyncIterable` of discovery events.  Breaking from the loop
   * or aborting the signal stops the underlying browse session.
   *
   * @example
   * ```ts
   * for await (const ev of node.browse({ serviceName: 'my-app' })) {
   *   if (ev.isActive) console.log('found', ev.nodeId);
   * }
   * ```
   */
  browse(options?: MdnsOptions, signal?: AbortSignal): AsyncIterable<PeerDiscoveryEvent>;
  /**
   * Advertise this node on the local network via mDNS.
   *
   * Returns a `Promise<void>` that resolves when advertising stops.
   * Pass a signal to cancel advertising.
   *
   * @example
   * ```ts
   * const ac = new AbortController();
   * void node.advertise({ serviceName: 'my-app' }, ac.signal);
   * // ... later:
   * ac.abort(); // stop advertising
   * ```
   */
  advertise(options?: MdnsOptions, signal?: AbortSignal): Promise<void>;
  /**
   * Resolves when the node has been closed (either via `close()` or due to
   * a fatal error).  Mirrors `WebTransportSession.closed`.
   */
  readonly closed: Promise<WebTransportCloseInfo>;
  /**
   * Full node address: node ID + relay URL(s) + direct socket addresses.
   * Share this with remote peers so they can connect directly.
   */
  addr(): Promise<NodeAddrInfo>;
  /**
   * Generate a ticket string encoding this node's current address.
   *
   * The ticket contains the node ID and all known addresses (relay URLs +
   * direct IPs). Share it with peers — they can pass it to `fetch()` or
   * `connect()` in place of a bare node ID.
   *
   * Tickets become stale when addresses change (e.g. after a network
   * change). They always remain usable via the embedded public key + DNS
   * fallback, but the direct path hint may be out of date.
   */
  ticket(): Promise<string>;
  /**
   * Home relay URL, or `null` if not connected to a relay.
   */
  homeRelay(): Promise<string | null>;
  /**
   * Known addresses for a remote peer, or `null` if unknown.
   */
  peerInfo(peer: PublicKey | string): Promise<NodeAddrInfo | null>;
  /**
   * Per-peer connection statistics with path information.
   *
   * Returns `null` if the peer is not known to this endpoint.
   * Use this to determine whether a connection is relayed or direct.
   */
  peerStats(peer: PublicKey | string): Promise<PeerStats | null>;
  /** Close the endpoint and release resources. */
  close(options?: CloseOptions): Promise<void>;
  /** Enables `await using node = await createNode()` (TC39 explicit resource management). */
  [Symbol.asyncDispose](): Promise<void>;
}

/** Options for closing an endpoint. */
export interface CloseOptions {
  /** If `true`, abort immediately without draining in-flight requests. */
  force?: boolean;
}

/** Result of the low-level `createEndpoint` FFI call. */
export interface EndpointInfo {
  endpointHandle: number;
  nodeId: string;
  keypair: Uint8Array;
}

/**
 * Node address information: node ID + relay URL(s) + direct socket addresses.
 * Used for sharing connection info with remote peers.
 */
export interface NodeAddrInfo {
  /** Base32-encoded public key (node ID). */
  id: string;
  /** Relay URLs and/or `ip:port` direct addresses. */
  addrs: string[];
}

// ── Observability types ──────────────────────────────────────────────────────

/**
 * Per-peer connection statistics.
 */
export interface PeerStats {
  /** Whether the active path goes through a relay server. */
  relay: boolean;
  /** Active relay URL, or `null` if using a direct path. */
  relayUrl: string | null;
  /** All known paths to this peer. */
  paths: PathInfo[];
}

/**
 * Network path information for a single transport address.
 */
export interface PathInfo {
  /** Whether this path goes through a relay server. */
  relay: boolean;
  /** The relay URL (if relay) or `ip:port` (if direct). */
  addr: string;
  /** Whether this is the currently active path. */
  active: boolean;
}

/**
 * Peer discovery event from mDNS local network discovery.
 */
export interface PeerDiscoveryEvent {
  /** Whether this peer was just discovered or has left the network. */
  type: "discovered" | "expired";
  /** Base32-encoded public key of the discovered peer. */
  nodeId: string;
  /** Known addresses for this peer (relay URLs and/or `ip:port`). */
  addrs?: string[];
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
  fetchToken: number,
  directAddrs: string[] | null
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

/** Full-duplex stream returned by `session.createBidirectionalStream()`. Mirrors `WebTransportBidirectionalStream`. */
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
