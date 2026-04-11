/**
 * iroh-http-shared — public exports.
 *
 * Platform adapters (iroh-http-node, iroh-http-tauri) import from here
 * to wire their bridge implementations into the shared layer.
 */

export type { Bridge, FfiRequest, FfiResponseHead, FfiResponse, RequestPayload,
              NodeOptions, IrohNode, EndpointInfo, RawServeFn, RawFetchFn, AllocBodyWriterFn,
              FfiDuplexStream, BidirectionalStream, DuplexStream, RawConnectFn,
              RelayMode, IrohFetchInit, DiscoveryOptions, LifecycleOptions,
              NodeAddrInfo, PeerDiscoveryEvent, PeerStats, PathInfo } from "./bridge.js";
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

import type { Bridge, EndpointInfo, NodeOptions, IrohNode, NodeAddrInfo, PeerStats, RawServeFn, RawFetchFn, AllocBodyWriterFn, RawConnectFn } from "./bridge.js";
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
  closeEndpoint: (handle: number) => Promise<void>,
  stopServe: (handle: number) => void,
  addrFns?: AddrFunctions,
): IrohNode {
  let resolveClosed!: () => void;
  const closedPromise = new Promise<void>((resolve) => {
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
    serve: makeServe(bridge, info.endpointHandle, rawServe, info.nodeId, closedPromise, () => stopServe(info.endpointHandle)),
    createBidirectionalStream: makeConnect(bridge, info.endpointHandle, rawConnect),
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
    close: async () => {
      await closeEndpoint(info.endpointHandle);
      resolveClosed();
    },
    [Symbol.asyncDispose]() { return node.close(); },
  };
  return node;
}
