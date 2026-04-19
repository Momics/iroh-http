/**
 * Bridge interface — the only thing that differs between Node.js and Tauri.
 *
 * Each platform (iroh-http-node / iroh-http-tauri) implements exactly these
 * methods.  All higher-level logic lives in iroh-http-shared and is
 * independent of the underlying transport.
 */

import type { PublicKey, SecretKey } from "./keys.js";
import type { ServeHandle, ServeHandler, ServeOptions } from "./serve.js";
import type { IrohSession, WebTransportCloseInfo } from "./session.js";

export interface Bridge {
  // ── Body streaming ─────────────────────────────────────────────────────────
  /** Pull the next chunk from a body reader. Returns `null` at EOF. */
  nextChunk(handle: bigint): Promise<Uint8Array | null>;
  /** Push `chunk` into a body writer. */
  sendChunk(handle: bigint, chunk: Uint8Array): Promise<void>;
  /** Signal end-of-body for the writer at `handle`. */
  finishBody(handle: bigint): Promise<void>;

  // ── §3 AbortSignal cancellation ────────────────────────────────────────────
  /** Drop a body reader from the Rust slab, cancelling an in-flight fetch. */
  cancelRequest(handle: bigint): Promise<void>; /**
   * Allocate an in-flight cancellation token in the Rust fetch map.
   * Call this before `rawFetch` and wire abort → `cancelFetch(token)`.
   */

  allocFetchToken(endpointHandle: number): Promise<bigint>;
  /**
   * Signal the Rust fetch task to abort.  Safe to call after the fetch has
   * already completed.  Fire-and-forget (do not await).
   */
  cancelFetch(token: bigint): void;
  // ── §4 Trailer headers ──────────────────────────────────────────────────────
  /**
   * Await and retrieve trailers produced after the body is consumed.
   * Returns `null` when no trailers were sent.
   */
  nextTrailer(handle: bigint): Promise<[string, string][] | null>;
  /**
   * Deliver trailers from the JS handler to the Rust server task (response),
   * or from the JS caller to the Rust body encoder (request trailers).
   * Call after `finishBody`. This must be called exactly once per handle;
   * calling it resolves the waiting pump task.
   */
  sendTrailers(handle: bigint, trailers: [string, string][]): Promise<void>;
  /**
   * Allocate a request trailer sender handle.
   * Call before `rawFetch` when the caller wants to send request trailers.
   * Pass the returned handle as `reqTrailersHandle` to `rawFetch`, then call
   * `sendTrailers(handle, trailers)` after `finishBody`.
   */
  allocTrailerSender(endpointHandle: number): bigint | Promise<bigint>;
}

// ── FFI data types ────────────────────────────────────────────────────────────

/**
 * Raw request data as it crosses the FFI boundary.
 * The `Peer-Id` header has already been stripped by the Rust layer.
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
  bodyHandle: bigint;
  /** Full `httpi://` URL of the responding peer. */
  url: string;
  /** Handle to the response trailer receiver (pass to `bridge.nextTrailer`). */
  trailersHandle: bigint;
}

/**
 * Payload delivered to the per-request callback in `rawServe`.
 *
 * JS reads the request body via `nextChunk(reqBodyHandle)` and writes the
 * response body via `sendChunk(resBodyHandle, …)` + `finishBody(resBodyHandle)`.
 */
export interface RequestPayload extends FfiRequest {
  /** Opaque handle — pass to `respondToRequest` on the Tauri bridge. */
  reqHandle: bigint;
  /** Body reader handle for the request body. */
  reqBodyHandle: bigint;
  /** Body writer handle for the response body. */
  resBodyHandle: bigint;
  /** Trailer receiver handle — JS calls `bridge.nextTrailer(reqTrailersHandle)` to read request trailers. `0n` in duplex mode. */
  reqTrailersHandle: bigint;
  /** Trailer sender handle — JS calls `bridge.sendTrailers(resTrailersHandle, pairs)` to send response trailers. `0n` in duplex mode. */
  resTrailersHandle: bigint;
  /** True when the client sent `Upgrade: iroh-duplex`. */
  isBidi: boolean;
}

// ── Platform function types ───────────────────────────────────────────────────

/** @internal Options for mDNS browse/advertise calls. */
export interface MdnsOptions {
  /** Application-specific mDNS service name.  Default: `'iroh-http'`. */
  serviceName?: string;
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
  /**
   * QUIC connection-level idle timeout in milliseconds. If no new streams are
   * opened within this window, the connection closes. Does not affect the
   * lifetime of individual in-progress streams. Default: 60 000.
   */
  idleTimeout?: number;

  // ── Discovery ─────────────────────────────────────────────────────────────
  /**
   * Peer discovery configuration.
   *
   * Controls how this node finds other peers on the network.  Both DNS and
   * mDNS are independently configurable.
   *
   * @example DNS with a custom server URL:
   * ```ts
   * discovery: { dns: { serverUrl: "https://dns.example.com" } }
   * ```
   *
   * @example LAN-only (disable DNS discovery):
   * ```ts
   * discovery: { dns: false }
   * ```
   */
  discovery?: {
    /**
     * DNS discovery configuration.
     * - `true` — enabled with n0 default servers (default).
     * - `false` — disabled; useful for LAN-only deployments.
     * - `{ serverUrl }` — enabled with a custom server URL.
     */
    dns?: boolean | { serverUrl?: string };
    /**
     * mDNS configuration.  Runtime control is via `node.browse()` /
     * `node.advertise()`; set this to pre-configure the default service name.
     * - `true` — enabled with service name `"iroh-http"`.
     * - `false` — disabled.
     * - `{ serviceName }` — enabled with a custom service name.
     */
    mdns?: boolean | { serviceName?: string };
  };

  // ── Power-user options ────────────────────────────────────────────────────
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

  // ── Advanced internal knobs ────────────────────────────────────────────────
  /**
   * Advanced internal configuration.
   *
   * Leave unset unless you have a specific reason. Incorrect values can
   * silently break connectivity or degrade performance.
   */
  advanced?: {
    /**
     * Controls backpressure between the Rust pump and your JS handler.
     * If your handler reads the request body slowly, the channel fills up and
     * the Rust sender pauses. Raise this to reduce stalls under slow consumers;
     * lower it to tighten memory use under high concurrency. Default: 32.
     */
    channelCapacity?: number;
    /**
     * Maximum byte length of a single chunk. Larger payloads are split into
     * multiple channel messages. Default: 65536 (64 KB).
     */
    maxChunkSizeBytes?: number;
    /**
     * Milliseconds to wait for a slow body reader to consume a chunk before
     * the connection is dropped. Default: 30 000.
     */
    drainTimeout?: number;
    /**
     * TTL in milliseconds for internal handle-table entries. Set to 0 to
     * disable periodic sweeping. Incorrect values can cause premature handle
     * invalidation or unbounded memory growth. Default: 300 000.
     */
    handleTtl?: number;
    /**
     * Number of consecutive accept errors before the serve loop gives up.
     * Increase if you see spurious shutdowns under adversarial load. Default: 5.
     */
    maxConsecutiveErrors?: number;
  };
  /**
   * Maximum number of idle QUIC connections to keep in the pool.
   * `undefined` means unlimited.
   */
  maxPooledConnections?: number;
  /**
   * Milliseconds a pooled connection may remain idle before being evicted and
   * a fresh handshake is forced on next use.  `undefined` (default) keeps
   * connections indefinitely (until the pool fills or they close).
   */
  poolIdleTimeoutMs?: number;

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

  /**
   * Maximum total QUIC connections the server will accept.
   * `undefined` means unlimited (default).
   */
  maxTotalConnections?: number;

  // ── Reconnect ──────────────────────────────────────────────────────────────
  /**
   * Automatic reconnect configuration.
   *
   * If enabled, the node will attempt to reconnect to the network after
   * losing connectivity. On mobile platforms this also handles
   * app-backgrounding and suspend cycles.
   */
  reconnect?: {
    /**
     * Automatically reconnect if the QUIC endpoint becomes unreachable.
     * On mobile, this also handles app-backgrounding/suspend cycles.
     * Default: false.
     */
    auto?: boolean;
    /**
     * Maximum reconnect attempts before marking the node as permanently dead.
     * Default: 3.
     */
    maxRetries?: number;
  };

  // ── Testing / CI ──────────────────────────────────────────────────────────
  /**
   * Peer identity verification for incoming `serve()` requests.
   *
   * By default, iroh-http rejects all incoming peers until this is explicitly
   * configured.
   *
   * - `true` — trust all peer node IDs (no verification).
   * - `(nodeId) => boolean | Promise<boolean>` — allow only verified node IDs.
   *
   * @default undefined (reject all incoming peers)
   */
  verifyNodeId?: true | ((nodeId: string) => boolean | Promise<boolean>);
  /**
   * Bind to local addresses only; no relay, no DNS discovery.
   * Use for tests and offline development.
   * @default false
   */
  disableNetworking?: boolean;
}

/**
 * An incoming `Request` delivered to a `serve` handler, augmented with
 * iroh-http-specific properties.
 *
 * Use this type instead of the plain `Request` when you need to access trailer
 * headers or other iroh-http extensions:
 *
 * ```ts
 * import type { IrohRequest } from "@momics/iroh-http-shared";
 *
 * node.serve({}, async (req: IrohRequest) => {
 *   const peer = req.headers.get('Peer-Id');
 *   const trailers = await req.trailers;        // null if none were sent
 *   const checksum = trailers?.get('x-checksum');
 *   return Response.json({ peer, checksum });
 * });
 * ```
 */
export interface IrohRequest extends Request {
  /**
   * A promise that resolves to the request trailer headers once the request
   * body has been fully consumed. Resolves to an empty `Headers` if no
   * trailers were sent (the runtime never returns `null` — issue-48 fix).
   */
  trailers: Promise<Headers>;
}

/**
 * The `Response`-like object returned by `IrohNode.fetch()`.
 *
 * Extends the standard `Response` with a `trailers` promise so that callers
 * can access response trailer headers without casting. The trailers are
 * populated after the response body is fully consumed.
 *
 * ```ts
 * import type { IrohResponse } from "@momics/iroh-http-shared";
 *
 * const res: IrohResponse = await node.fetch(peer, '/api/data');
 * const body = await res.text();
 * const checksum = (await res.trailers).get('x-checksum');
 * ```
 */
export interface IrohResponse extends Response {
  /**
   * Resolves to the response trailer headers after the response body is
   * fully consumed. Always resolves to a `Headers` object (never `null`).
   */
  trailers: Promise<Headers>;
}

/**
 * A `Response`-like object that a serve handler may return to include
 * response trailer headers.
 *
 * ```ts
 * node.serve(async (req) => {
 *   const res = new Response("body");
 *   (res as IrohServeResponse).trailers = () => new Headers({ 'x-checksum': '...' });
 *   return res;
 * });
 * ```
 *
 * The simpler way is to simply attach the `trailers` function to any `Response`:
 *
 * ```ts
 * return Object.assign(new Response("body"), {
 *   trailers: () => new Headers({ 'x-checksum': hash }),
 * });
 * ```
 */
export interface IrohServeResponse extends Response {
  /**
   * Called after the response body has been fully sent.
   * Return the trailer headers to send to the client.
   * Only invoked when the response includes a `Trailer:` header.
   */
  trailers?: () => Headers | Promise<Headers>;
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
  /**
   * Request trailer headers to send after the request body is complete.
   *
   * Only valid when a request body is also provided. Trailers are transmitted
   * as HTTP/1.1 chunked-encoding trailers — the server handler can read them
   * via `await req.trailers` after consuming the request body.
   *
   * @example
   * ```ts
   * const hash = computeHash(data);
   * const res = await node.fetch(peer, '/upload', {
   *   method: 'POST',
   *   body: data,
   *   trailers: { 'content-md5': hash },
   * });
   * ```
   */
  trailers?: HeadersInit;
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
 *   const peer = req.headers.get('Peer-Id');
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
  /**
   * Send an HTTP request to a remote node.
   *
   * Two call forms are supported:
   * - **Web-standard:** `node.fetch("httpi://<peerId>/path", init?)` — peer ID embedded in URL
   * - **Legacy:** `node.fetch(peer, "/path", init?)` — peer and path supplied separately
   *
   * @param input - `httpi://` URL string or URL object (web-standard form).
   * @param init - Standard `RequestInit` options plus iroh-specific `directAddrs`.
   * @returns An `IrohResponse` — a standard `Response` with an additional `trailers` promise.
   * @throws {IrohConnectError} If the peer is unreachable.
   * @throws {IrohAbortError} If `init.signal` is aborted.
   */
  fetch(input: string | URL, init?: IrohFetchInit): Promise<IrohResponse>;
  /**
   * Send an HTTP request to a remote node (legacy form).
   *
   * @param peer - Remote node's public key or base32 node ID string.
   * @param input - Request URL path, e.g. `"/api/data"` or full `"httpi://nodeId/path"`.
   * @param init - Standard `RequestInit` options plus iroh-specific `directAddrs`.
   * @returns An `IrohResponse` — a standard `Response` with an additional `trailers` promise.
   * @throws {IrohConnectError} If the peer is unreachable.
   * @throws {IrohAbortError} If `init.signal` is aborted.
   */
  fetch(
    peer: PublicKey | string,
    input: string | URL,
    init?: IrohFetchInit,
  ): Promise<IrohResponse>;
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
  connect(
    peer: PublicKey | string,
    init?: { directAddrs?: string[] },
  ): Promise<IrohSession>;
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
  browse(
    options?: MdnsOptions,
    signal?: AbortSignal,
  ): AsyncIterable<PeerDiscoveryEvent>;
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
  /**
   * Endpoint-level observability snapshot.
   *
   * Returns point-in-time counts of active handles, connections, requests,
   * and pool entries.  Useful for monitoring and debugging.
   */
  stats(): Promise<EndpointStats>;
  /**
   * Async iterable that yields a `PathInfo` each time the active network
   * path to `peer` changes (e.g. from relay to direct, or between addresses).
   *
   * The stream polls `peerStats` at `pollIntervalMs` intervals (default 500 ms)
   * and only yields when the selected path differs from the previous one.
   * Break the `for await` loop to stop watching.
   *
   * ```ts
   * for await (const path of node.pathChanges(peerId)) {
   *   console.log(path.relay ? `via relay ${path.addr}` : `direct ${path.addr}`);
   * }
   * ```
   */
  pathChanges(
    peer: PublicKey | string,
    pollIntervalMs?: number,
  ): AsyncIterable<PathInfo>;
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
 * Endpoint-level observability snapshot.
 *
 * All counts are point-in-time reads and may change between calls.
 */
export interface EndpointStats {
  /** Number of currently open body reader handles. */
  activeReaders: number;
  /** Number of currently open body writer handles. */
  activeWriters: number;
  /** Number of live QUIC session handles. */
  activeSessions: number;
  /** Total allocated handle count (readers + writers + sessions + trailers + …). */
  totalHandles: number;
  /** Number of QUIC connections currently cached in the connection pool. */
  poolSize: number;
  /** Number of live QUIC connections accepted by the serve loop. */
  activeConnections: number;
  /** Number of HTTP requests currently being processed. */
  activeRequests: number;
}

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
  /**
   * Round-trip time in milliseconds (from the QUIC connection).
   * `null` if no active QUIC connection is pooled (e.g. after a cold start).
   */
  rttMs: number | null;
  /**
   * Total UDP bytes sent to this peer since the connection was established.
   * `null` if no active QUIC connection is pooled.
   */
  bytesSent: number | null;
  /**
   * Total UDP bytes received from this peer since the connection was established.
   * `null` if no active QUIC connection is pooled.
   */
  bytesReceived: number | null;
  /**
   * Total QUIC packets lost on the active path.
   * `null` if no active QUIC connection is pooled, or not exposed by transport.
   */
  lostPackets: number | null;
  /**
   * Total QUIC packets sent on the active path.
   * `null` if no active QUIC connection is pooled, or not exposed by transport.
   */
  sentPackets: number | null;
  /**
   * Current QUIC congestion window in bytes.
   * `null` if no active QUIC connection is pooled, or not exposed by transport.
   */
  congestionWindow: number | null;
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
 * Connection lifecycle event fired when a QUIC peer connects or disconnects.
 *
 * - `connected: true` — fired on the 0→1 transition (first connection from this peer).
 * - `connected: false` — fired on the 1→0 transition (last connection from this peer closed).
 */
export interface PeerConnectionEvent {
  /** Base32-encoded public key of the peer. */
  peerId: string;
  /** `true` when connecting, `false` when disconnecting. */
  connected: boolean;
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
  options: {
    /** Called on each QUIC peer connect (0→1) and disconnect (1→0) transition. */
    onConnectionEvent?: (event: PeerConnectionEvent) => void;
  },
  callback: (payload: RequestPayload) => Promise<FfiResponseHead>,
) => Promise<void>;

/** Raw fetch function provided by each platform bridge. */
export type RawFetchFn = (
  endpointHandle: number,
  nodeId: string,
  url: string,
  method: string,
  headers: [string, string][],
  reqBodyHandle: bigint | null,
  reqTrailersHandle: bigint | null,
  fetchToken: bigint,
  directAddrs: string[] | null,
) => Promise<FfiResponse>;

/** Allocate a body writer handle (may be sync or async). */
export type AllocBodyWriterFn = () => bigint | Promise<bigint>;

// ── §2 Bidirectional streaming types ─────────────────────────────────────────

/** Raw duplex stream handles returned by `rawConnect`. */
export interface FfiDuplexStream {
  /** Handle for reading data sent by the server. */
  readHandle: bigint;
  /** Handle for writing data to the server. */
  writeHandle: bigint;
}

/** Full-duplex stream returned by `session.createBidirectionalStream()`. Mirrors `WebTransportBidirectionalStream`. */
export interface BidirectionalStream {
  /** Receive data from the server. */
  readable: ReadableStream<Uint8Array>;
  /** Send data to the server. */
  writable: WritableStream<Uint8Array>;
}

/** Raw connect function provided by each platform bridge. */
export type RawConnectFn = (
  endpointHandle: number,
  nodeId: string,
  path: string,
  headers: [string, string][],
) => Promise<FfiDuplexStream>;
