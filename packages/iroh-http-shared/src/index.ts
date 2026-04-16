/**
 * iroh-http-shared — public exports.
 *
 * Platform adapters (iroh-http-node, iroh-http-tauri) import from here
 * to wire their bridge implementations into the shared layer.
 */

// ── Public types ────────────────────────────────────────────────────────────
export type {
  BidirectionalStream,
  CloseOptions,
  EndpointInfo,
  IrohFetchInit,
  IrohNode,
  IrohRequest,
  IrohResponse,
  IrohServeResponse,
  NodeAddrInfo,
  NodeOptions,
  PathInfo,
  PeerDiscoveryEvent,
  PeerStats,
  RelayMode,
} from "./bridge.js";
export type {
  IrohSession,
  WebTransportBidirectionalStream,
  WebTransportCloseInfo,
  WebTransportDatagramDuplexStream,
} from "./session.js";
export type { ServeHandle, ServeHandler, ServeOptions } from "./serve.js";

// ── Internal types (used by platform adapters, not end users) ───────────────
// Adapter packages import these from "@momics/iroh-http-shared/adapter" instead.
// Bridge is kept here for use by buildNode() below.
export { buildSession } from "./session.js";
export { bodyInitToStream, makeReadable, pipeToWriter } from "./streams.js";
export { makeConnect, makeFetch } from "./fetch.js";
export { makeServe } from "./serve.js";
export { PublicKey, resolveNodeId, SecretKey } from "./keys.js";
export {
  classifyBindError,
  classifyError,
  IrohAbortError,
  IrohArgumentError,
  IrohBindError,
  IrohConnectError,
  IrohError,
  IrohHandleError,
  IrohProtocolError,
  IrohStreamError,
} from "./errors.js";
export {
  decodeBase64,
  encodeBase64,
  normaliseRelayMode,
} from "./utils.js";
export type { NormalisedRelay } from "./utils.js";

/**
 * Extract the node ID from a ticket string without network I/O.
 *
 * Accepts a ticket string (JSON-encoded address info) or a bare node ID
 * string (returned unchanged).
 */
export function ticketNodeId(ticket: string): string {
  try {
    const info = JSON.parse(ticket) as { id?: string };
    if (info && typeof info.id === "string") return info.id;
  } catch {
    // Not JSON — treat as bare node ID
  }
  return ticket;
}

import type {
  AllocBodyWriterFn,
  Bridge,
  CloseOptions,
  EndpointInfo,
  IrohNode,
  MdnsOptions,
  NodeAddrInfo,
  NodeOptions,
  PathInfo,
  PeerDiscoveryEvent,
  PeerStats,
  RawConnectFn,
  RawFetchFn,
  RawServeFn,
} from "./bridge.js";
import type { RawSessionFns, WebTransportCloseInfo } from "./session.js";
import { buildSession } from "./session.js";
import { makeConnect, makeFetch } from "./fetch.js";
import { makeServe } from "./serve.js";
import { PublicKey, resolveNodeId, SecretKey } from "./keys.js";

/** Platform-specific address introspection functions. */
export interface AddrFunctions {
  /** Full node address: node ID + relay URL(s) + direct socket addresses. */
  nodeAddr(endpointHandle: number): Promise<NodeAddrInfo>;
  /** Generate a ticket string. */
  nodeTicket(endpointHandle: number): Promise<string>;
  /** Home relay URL, or null if not connected to a relay. */
  homeRelay(endpointHandle: number): Promise<string | null>;
  /** Known addresses for a remote peer, or null if unknown. */
  peerInfo(
    endpointHandle: number,
    nodeId: string,
  ): Promise<NodeAddrInfo | null>;
  /** Per-peer connection statistics with path information. */
  peerStats(endpointHandle: number, nodeId: string): Promise<PeerStats | null>;
}

/** Platform-specific mDNS discovery functions. */
export interface DiscoveryFunctions {
  /** Start a browse session. Returns a browse handle. */
  mdnsBrowse(endpointHandle: number, serviceName: string): Promise<number>;
  /** Poll the next discovery event. Returns null when the session is closed. */
  mdnsNextEvent(browseHandle: number): Promise<PeerDiscoveryEvent | null>;
  /** Close a browse session. */
  mdnsBrowseClose(browseHandle: number): void;
  /** Start advertising. Returns an advertise handle. */
  mdnsAdvertise(endpointHandle: number, serviceName: string): Promise<number>;
  /** Stop advertising. */
  mdnsAdvertiseClose(advertiseHandle: number): void;
}

/** Configuration for `buildNode()`. Groups platform primitives into a single object. */
export interface BuildNodeConfig {
  bridge: Bridge;
  info: EndpointInfo;
  rawFetch: RawFetchFn;
  rawServe: RawServeFn;
  rawConnect: RawConnectFn;
  allocBodyWriter: AllocBodyWriterFn;
  closeEndpoint: (handle: number, force?: boolean) => Promise<void>;
  stopServe: (handle: number) => void;
  addrFns?: AddrFunctions;
  discoveryFns?: DiscoveryFunctions;
  sessionFns?: RawSessionFns;
  /**
   * A promise that resolves when the native endpoint shuts down — either via
   * explicit `closeEndpoint()` or because the QUIC stack closed on its own.
   * When provided, `node.closed` is also resolved by this signal (in addition
   * to the explicit `node.close()` call).
   */
  nativeClosed?: Promise<void>;
}

/**
 * Factory that constructs an `IrohNode` from platform primitives.
 *
 * Each platform adapter calls this after binding an endpoint.
 *
 * @returns A fully wired `IrohNode` ready for `fetch`, `serve`, and `close`.
 *
 * @example
 * ```ts
 * // Platform adapter wiring (typically internal):
 * const node = buildNode({ bridge, info, rawFetch, rawServe, rawConnect, allocBodyWriter, closeEndpoint, stopServe });
 * const res = await node.fetch(peerId, '/hello');
 * ```
 */
export function buildNode(config: BuildNodeConfig): IrohNode {
  const {
    bridge,
    info,
    rawFetch,
    rawServe,
    rawConnect,
    allocBodyWriter,
    closeEndpoint,
    stopServe,
    addrFns,
    discoveryFns,
    sessionFns,
    nativeClosed,
  } = config;
  let resolveClosed!: (info: WebTransportCloseInfo) => void;
  const closedPromise = new Promise<WebTransportCloseInfo>((resolve) => {
    resolveClosed = resolve;
  });
  // #60: also resolve node.closed when the native endpoint signals shutdown,
  // so that callers awaiting node.closed are not left hanging on fatal exits.
  if (nativeClosed) {
    nativeClosed.then(() =>
      resolveClosed({ closeCode: 0, reason: "native shutdown" })
    );
  }

  const publicKey = PublicKey.fromString(info.nodeId);
  const secretKey = SecretKey._fromBytesWithPublicKey(info.keypair, publicKey);

  const node: IrohNode = {
    publicKey,
    secretKey,
    fetch: makeFetch(bridge, info.endpointHandle, rawFetch, allocBodyWriter),
    serve: makeServe(
      bridge,
      info.endpointHandle,
      rawServe,
      info.nodeId,
      closedPromise.then(() => {}),
      () => stopServe(info.endpointHandle),
    ),
    async connect(peer, init?) {
      if (!sessionFns) {
        throw new Error("connect() not supported by this platform adapter");
      }
      const nodeId = resolveNodeId(peer);
      const directAddrs = init?.directAddrs ?? null;
      const sessionHandle = await sessionFns.connect(
        info.endpointHandle,
        nodeId,
        directAddrs,
      );
      const remotePk = PublicKey.fromString(nodeId);
      return buildSession(bridge, sessionHandle, remotePk, sessionFns);
    },
    browse(
      options?: MdnsOptions,
      signal?: AbortSignal,
    ): AsyncIterable<PeerDiscoveryEvent> {
      if (!discoveryFns) {
        throw new Error("browse() not supported by this platform adapter");
      }
      const fns = discoveryFns;
      const handle = info.endpointHandle;
      const svcName = options?.serviceName ?? "iroh-http";
      return {
        [Symbol.asyncIterator]() {
          let browseHandle: number | null = null;
          return {
            async next() {
              if (browseHandle === null) {
                browseHandle = await fns.mdnsBrowse(handle, svcName);
              }
              if (signal?.aborted) {
                fns.mdnsBrowseClose(browseHandle);
                browseHandle = null;
                return { done: true as const, value: undefined };
              }

              // Issue-62: race mdnsNextEvent against AbortSignal so that
              // aborting on a quiet network unblocks iteration immediately
              // without waiting for the next discovery event to arrive.
              let event: PeerDiscoveryEvent | null;
              if (signal) {
                const abortPromise = new Promise<null>((resolve) => {
                  if (signal.aborted) {
                    resolve(null);
                    return;
                  }
                  signal.addEventListener("abort", () => resolve(null), {
                    once: true,
                  });
                });
                event = await Promise.race([
                  fns.mdnsNextEvent(browseHandle),
                  abortPromise,
                ]);
                // Close the native handle immediately on abort so the Rust
                // side can clean up even while the other branch is pending.
                if (signal.aborted && browseHandle !== null) {
                  fns.mdnsBrowseClose(browseHandle);
                  browseHandle = null;
                  return { done: true as const, value: undefined };
                }
              } else {
                event = await fns.mdnsNextEvent(browseHandle);
              }

              if (event === null) {
                return { done: true as const, value: undefined };
              }
              return { done: false as const, value: event };
            },
            return() {
              if (browseHandle !== null) {
                fns.mdnsBrowseClose(browseHandle);
                browseHandle = null;
              }
              return Promise.resolve({ done: true as const, value: undefined });
            },
          };
        },
      };
    },
    async advertise(
      options?: MdnsOptions,
      signal?: AbortSignal,
    ): Promise<void> {
      if (!discoveryFns) {
        throw new Error("advertise() not supported by this platform adapter");
      }
      const svcName = options?.serviceName ?? "iroh-http";
      const advHandle = await discoveryFns.mdnsAdvertise(
        info.endpointHandle,
        svcName,
      );
      if (signal) {
        return new Promise<void>((resolve) => {
          signal.addEventListener("abort", () => {
            discoveryFns!.mdnsAdvertiseClose(advHandle);
            resolve();
          }, { once: true });
          if (signal.aborted) {
            discoveryFns!.mdnsAdvertiseClose(advHandle);
            resolve();
          }
        });
      }
      // No signal — advertise until the node closes.
    },
    addr: async () => {
      if (!addrFns) {
        throw new Error("addr() not supported by this platform adapter");
      }
      return addrFns.nodeAddr(info.endpointHandle);
    },
    ticket: async () => {
      if (!addrFns) {
        throw new Error("ticket() not supported by this platform adapter");
      }
      return addrFns.nodeTicket(info.endpointHandle);
    },
    homeRelay: async () => {
      if (!addrFns) return null;
      return addrFns.homeRelay(info.endpointHandle);
    },
    peerInfo: async (peer: PublicKey | string) => {
      if (!addrFns) return null;
      const nodeId = resolveNodeId(peer);
      return addrFns.peerInfo(info.endpointHandle, nodeId);
    },
    peerStats: async (peer: PublicKey | string) => {
      if (!addrFns) return null;
      const nodeId = resolveNodeId(peer);
      return addrFns.peerStats(info.endpointHandle, nodeId);
    },
    pathChanges(
      peer: PublicKey | string,
      pollIntervalMs = 500,
    ): AsyncIterable<PathInfo> {
      const nodeId = resolveNodeId(peer);
      const endpointHandle = info.endpointHandle;
      return {
        [Symbol.asyncIterator]() {
          let stopped = false;
          let lastPath: string | null = null;
          let timeoutId: ReturnType<typeof setTimeout> | null = null;
          let wakeResolve: (() => void) | null = null;

          // Schedule a wake-up after pollIntervalMs.
          function scheduleWake() {
            timeoutId = setTimeout(() => {
              timeoutId = null;
              const r = wakeResolve;
              wakeResolve = null;
              r?.();
            }, pollIntervalMs);
          }

          function cancelWake() {
            if (timeoutId !== null) {
              clearTimeout(timeoutId);
              timeoutId = null;
            }
            const r = wakeResolve;
            wakeResolve = null;
            r?.();
          }

          return {
            async next(): Promise<IteratorResult<PathInfo>> {
              while (!stopped) {
                const stats = addrFns
                  ? await addrFns.peerStats(endpointHandle, nodeId)
                  : null;

                if (stats) {
                  const selected = stats.paths.find((p) => p.active);
                  if (selected) {
                    const key = `${selected.relay}:${selected.addr}`;
                    if (key !== lastPath) {
                      lastPath = key;
                      scheduleWake();
                      return { done: false as const, value: selected };
                    }
                  }
                }

                // Wait for the next poll interval.
                await new Promise<void>((resolve) => {
                  wakeResolve = resolve;
                  scheduleWake();
                });
              }
              return { done: true as const, value: undefined };
            },
            return(): Promise<IteratorResult<PathInfo>> {
              stopped = true;
              cancelWake();
              return Promise.resolve({ done: true as const, value: undefined });
            },
          };
        },
      };
    },
    closed: closedPromise,
    close: async (options?) => {
      await closeEndpoint(info.endpointHandle, options?.force);
      resolveClosed({ closeCode: 0, reason: "" });
    },
    [Symbol.asyncDispose]() {
      return node.close();
    },
  };
  return node;
}
