/**
 * iroh-http-shared — public exports.
 *
 * Platform adapters (iroh-http-node, iroh-http-tauri) import from here
 * to wire their bridge implementations into the shared layer.
 */

// ── Public types ────────────────────────────────────────────────────────────
export type { CloseOptions, NodeOptions, IrohNode, EndpointInfo,
              RelayMode, IrohFetchInit, DiscoveryOptions, MdnsOptions, LifecycleOptions,
              NodeAddrInfo, PeerDiscoveryEvent, PeerStats, PathInfo,
              BidirectionalStream, DuplexStream } from "./bridge.js";
export type { IrohSession, WebTransportBidirectionalStream, WebTransportCloseInfo, WebTransportDatagramDuplexStream } from "./session.js";
export type { ServeHandler, ServeOptions, ServeHandle } from "./serve.js";

// ── Internal types (used by platform adapters, not end users) ───────────────
/** @internal */ export type { Bridge, FfiRequest, FfiResponseHead, FfiResponse, RequestPayload,
              RawServeFn, RawFetchFn, AllocBodyWriterFn,
              FfiDuplexStream, RawConnectFn } from "./bridge.js";
/** @internal */ export type { RawSessionFns } from "./session.js";
export { buildSession } from "./session.js";
export type { ServeHandler, ServeOptions, ServeHandle } from "./serve.js";
export { makeReadable, pipeToWriter, bodyInitToStream } from "./streams.js";
export { makeFetch, makeConnect } from "./fetch.js";
export { makeServe } from "./serve.js";
export { PublicKey, SecretKey, resolveNodeId } from "./keys.js";
export {
  IrohError, IrohBindError, IrohConnectError, IrohStreamError, IrohProtocolError,
  IrohAbortError, IrohArgumentError, IrohHandleError,
  classifyError, classifyBindError,
} from "./errors.js";

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

import type { Bridge, CloseOptions, EndpointInfo, NodeOptions, IrohNode, MdnsOptions, NodeAddrInfo, PeerDiscoveryEvent, PeerStats, RawServeFn, RawFetchFn, AllocBodyWriterFn, RawConnectFn } from "./bridge.js";
import type { RawSessionFns, WebTransportCloseInfo } from "./session.js";
import { buildSession } from "./session.js";
import { makeFetch, makeConnect } from "./fetch.js";
import { makeServe } from "./serve.js";
import { PublicKey, SecretKey, resolveNodeId } from "./keys.js";

/** Platform-specific address introspection functions. */
export interface AddrFunctions {
  /** Full node address: node ID + relay URL(s) + direct socket addresses. */
  nodeAddr(endpointHandle: number): Promise<NodeAddrInfo>;
  /** Generate a ticket string. */
  nodeTicket(endpointHandle: number): Promise<string>;
  /** Home relay URL, or null if not connected to a relay. */
  homeRelay(endpointHandle: number): Promise<string | null>;
  /** Known addresses for a remote peer, or null if unknown. */
  peerInfo(endpointHandle: number, nodeId: string): Promise<NodeAddrInfo | null>;
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

/**
 * Factory that constructs an `IrohNode` from platform primitives.
 *
 * Each platform adapter calls this after binding an endpoint.
 *
 * @param bridge          Platform bridge implementation.
 * @param info            Endpoint info returned by the low-level bind.
 * @param rawFetch        Low-level fetch function (platform-specific).
 * @param rawServe        Low-level serve function (platform-specific).
 * @param rawConnect      Low-level duplex connect function (platform-specific).
 * @param allocBodyWriter Synchronously allocates a body writer handle.
 * @param closeEndpoint   Closes the bound endpoint.
 * @param stopServe       Stops the serve loop for graceful shutdown.
 * @param addrFns         Platform-specific address introspection functions.
 * @param discoveryFns    Platform-specific mDNS discovery functions.
 * @param sessionFns      Platform-specific session (connect/bidi stream) functions.
 * @returns A fully wired `IrohNode` ready for `fetch`, `serve`, and `close`.
 *
 * @example
 * ```ts
 * // Platform adapter wiring (typically internal):
 * const node = buildNode(bridge, info, rawFetch, rawServe, rawConnect, alloc, close, addrFns);
 * const res = await node.fetch(peerId, '/hello');
 * ```
 */
export function buildNode(
  bridge: Bridge,
  info: EndpointInfo,
  rawFetch: RawFetchFn,
  rawServe: RawServeFn,
  rawConnect: RawConnectFn,
  allocBodyWriter: AllocBodyWriterFn,
  closeEndpoint: (handle: number, force?: boolean) => Promise<void>,
  stopServe: (handle: number) => void,
  addrFns?: AddrFunctions,
  discoveryFns?: DiscoveryFunctions,
  sessionFns?: RawSessionFns,
): IrohNode {
  let resolveClosed!: (info: WebTransportCloseInfo) => void;
  const closedPromise = new Promise<WebTransportCloseInfo>((resolve) => {
    resolveClosed = resolve;
  });

  const publicKey = PublicKey.fromString(info.nodeId);
  const secretKey = SecretKey._fromBytesWithPublicKey(info.keypair, publicKey);

  const node: IrohNode = {
    publicKey,
    secretKey,
    nodeId: info.nodeId,
    keypair: info.keypair,
    fetch: makeFetch(bridge, info.endpointHandle, rawFetch, allocBodyWriter),
    serve: makeServe(bridge, info.endpointHandle, rawServe, info.nodeId, closedPromise.then(() => {}), () => stopServe(info.endpointHandle)),
    async connect(peer, init?) {
      if (!sessionFns) throw new Error("connect() not supported by this platform adapter");
      const nodeId = resolveNodeId(peer);
      const directAddrs = init?.directAddrs ?? null;
      const sessionHandle = await sessionFns.connect(info.endpointHandle, nodeId, directAddrs);
      const remotePk = PublicKey.fromString(nodeId);
      return buildSession(bridge, sessionHandle, remotePk, sessionFns);
    },
    browse(options?: MdnsOptions, signal?: AbortSignal): AsyncIterable<PeerDiscoveryEvent> {
      if (!discoveryFns) throw new Error("browse() not supported by this platform adapter");
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
              const event = await fns.mdnsNextEvent(browseHandle);
              if (event === null) return { done: true as const, value: undefined };
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
    async advertise(options?: MdnsOptions, signal?: AbortSignal): Promise<void> {
      if (!discoveryFns) throw new Error("advertise() not supported by this platform adapter");
      const svcName = options?.serviceName ?? "iroh-http";
      const advHandle = await discoveryFns.mdnsAdvertise(info.endpointHandle, svcName);
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
      if (!addrFns) throw new Error("addr() not supported by this platform adapter");
      return addrFns.nodeAddr(info.endpointHandle);
    },
    ticket: async () => {
      if (!addrFns) throw new Error("ticket() not supported by this platform adapter");
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
    closed: closedPromise,
    close: async (options?) => {
      await closeEndpoint(info.endpointHandle, options?.force);
      resolveClosed({ closeCode: 0, reason: "" });
    },
    [Symbol.asyncDispose]() { return node.close(); },
  };
  return node;
}
