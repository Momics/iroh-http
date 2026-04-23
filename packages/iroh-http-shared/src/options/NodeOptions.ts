import type { SecretKey } from "../keys.js";

/**
 * How to use the global Iroh relay network for NAT traversal.
 *
 * - `"default"` — use Iroh's public relay servers (recommended).
 * - `"staging"` — use Iroh's staging relay servers (testing only).
 * - `"disabled"` — direct QUIC only; connections fail if hole-punching is not possible.
 * - `string` — a single custom relay server URL.
 * - `string[]` — multiple custom relay server URLs.
 */
export type RelayMode = "default" | "staging" | "disabled" | (string & {}) | string[];

export interface NodeOptions {
  /**
   * Ed25519 identity for this node. Accepts a `SecretKey` instance or raw 32-byte
   * key material as a `Uint8Array`. When omitted a fresh key is generated.
   */
  key?: SecretKey | Uint8Array;

  /**
   * Relay server configuration. Controls whether and how the node uses the Iroh
   * relay network for NAT traversal.
   * @default "default"
   */
  relayMode?: RelayMode;

  /**
   * Local socket address(es) the QUIC endpoint binds to.
   * Use `"0.0.0.0:0"` (or `"[::]:0"`) to let the OS pick a port.
   * @default "0.0.0.0:0"
   */
  bindAddr?: string | string[];

  /**
   * QUIC idle timeout in milliseconds. Connections with no activity for this
   * duration are closed. Set to `0` to disable.
   * @default 30_000
   */
  idleTimeout?: number;

  /** Peer discovery backends. */
  discovery?: {
    /**
     * DNS-SD peer discovery. Pass `true` to use the default Iroh DNS server,
     * or `{ serverUrl }` to point at a custom DNS-SD resolver.
     */
    dns?: boolean | { serverUrl?: string };
    /**
     * mDNS peer discovery on the local network. Pass `true` to use the default
     * service name (`"iroh-http"`), or `{ serviceName }` for a custom name.
     */
    mdns?: boolean | { serviceName?: string };
  };

  /**
   * HTTP proxy URL for outbound relay connections, e.g. `"http://proxy:8080"`.
   * Takes precedence over `proxyFromEnv`.
   */
  proxyUrl?: string;

  /**
   * When `true`, read proxy settings from the `HTTP_PROXY` / `HTTPS_PROXY`
   * environment variables. Ignored when `proxyUrl` is set.
   */
  proxyFromEnv?: boolean;

  /**
   * Write TLS session keys to a keylog file for Wireshark capture.
   * Reads the `SSLKEYLOGFILE` environment variable for the file path.
   * Never enable in production.
   */
  keylog?: boolean;

  /**
   * Low-level Rust runtime knobs. Only touch these if you understand the
   * internal request pipeline.
   */
  advanced?: {
    /**
     * Capacity of the internal mpsc channel that queues requests from the QUIC
     * accept loop to the JS handler. Increase if you see dropped requests under
     * burst load.
     * @default 256
     */
    channelCapacity?: number;

    /**
     * Maximum body chunk size in bytes for streaming request/response bodies
     * over the FFI boundary.
     * @default 65_536 (64 KB)
     */
    maxChunkSizeBytes?: number;

    /**
     * Milliseconds to wait for in-flight requests to complete during graceful
     * shutdown before forcibly closing connections.
     * @default 5_000
     */
    drainTimeout?: number;

    /**
     * Milliseconds before an idle body or request handle is evicted from the
     * slab. Prevents handle leaks when callers fail to consume or cancel a body.
     * @default 60_000
     */
    handleTtl?: number;

    /**
     * Maximum consecutive accept-loop errors before the serve loop gives up
     * and terminates with an error.
     * @default 10
     */
    maxConsecutiveErrors?: number;
  };

  /**
   * Maximum number of QUIC connections kept alive in the connection pool
   * across all peers. Older idle connections are evicted when the limit is hit.
   * @default 64
   */
  maxPooledConnections?: number;

  /**
   * Milliseconds before an idle pooled connection is dropped.
   * @default 90_000
   */
  poolIdleTimeoutMs?: number;

  /**
   * Enable zstd response compression. Pass `true` to use defaults, or an
   * object to tune the level and minimum body size.
   * @default false
   */
  compression?: boolean | { level?: number; minBodyBytes?: number };

  /**
   * Maximum number of requests processed concurrently across the endpoint.
   * Requests beyond this limit receive `503 Service Unavailable`.
   * @default unlimited
   */
  maxConcurrency?: number;

  /**
   * Maximum number of simultaneous QUIC connections from a single peer.
   * Additional connection attempts from the same peer are rejected.
   * @default unlimited
   */
  maxConnectionsPerPeer?: number;

  /**
   * Milliseconds before an outbound request is aborted with a timeout error.
   * @default 30_000
   */
  requestTimeout?: number;

  /**
   * Maximum request body size in bytes. Requests with larger bodies receive
   * `413 Content Too Large`.
   * @default 10_485_760 (10 MB)
   */
  maxRequestBodyBytes?: number;

  /**
   * Maximum total size of all request headers in bytes. Requests that exceed
   * this limit receive `431 Request Header Fields Too Large`.
   * @default 65_536 (64 KB)
   */
  maxHeaderBytes?: number;

  /**
   * Maximum total number of active QUIC connections across all peers.
   * New connections are rejected when the limit is reached.
   * @default unlimited
   */
  maxTotalConnections?: number;

  /** Automatic reconnect policy for outbound connections. */
  reconnect?: {
    /** Enable automatic reconnect on connection loss. @default false */
    auto?: boolean;
    /** Maximum reconnect attempts before giving up. @default 3 */
    maxRetries?: number;
  };

  /**
   * When `true`, the endpoint binds only to loopback. Useful for tests that
   * must not touch the network.
   * @default false
   */
  disableNetworking?: boolean;

  /** Observability and diagnostics options. */
  observability?: {
    /**
     * Start the background transport-event loop, which delivers `pool:hit`,
     * `pool:miss`, `pool:evict`, `path:change`, and `handle:sweep` events via
     * the `"transport"` CustomEvent on `IrohNode`.
     * @default false
     */
    transportEvents?: boolean;
  };
}
