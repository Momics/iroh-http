/**
 * iroh-http-shared — public exports.
 *
 * Platform adapters (iroh-http-node, iroh-http-tauri) import from here
 * to wire their bridge implementations into the shared layer.
 */
export type { BidirectionalStream, CloseOptions, EndpointInfo, EndpointStats, IrohFetchInit, IrohNode, NodeAddrInfo, NodeOptions, PathInfo, PeerConnectionEvent, PeerDiscoveryEvent, PeerStats, RelayMode, } from "./bridge.js";
export type { IrohSession, WebTransportBidirectionalStream, WebTransportCloseInfo, WebTransportDatagramDuplexStream, } from "./session.js";
export type { ServeHandle, ServeHandler, ServeOptions } from "./serve.js";
export { buildSession } from "./session.js";
export { bodyInitToStream, makeReadable, pipeToWriter } from "./streams.js";
export { makeConnect, makeFetch } from "./fetch.js";
export { makeServe } from "./serve.js";
export { PublicKey, resolveNodeId, SecretKey } from "./keys.js";
export { classifyBindError, classifyError, IrohAbortError, IrohArgumentError, IrohBindError, IrohConnectError, IrohError, IrohHandleError, IrohProtocolError, IrohStreamError, } from "./errors.js";
export { decodeBase64, encodeBase64, normaliseRelayMode, } from "./utils.js";
export type { NormalisedRelay } from "./utils.js";
/**
 * Extract the node ID from a ticket string without network I/O.
 *
 * Accepts a ticket string (JSON-encoded address info) or a bare node ID
 * string (returned unchanged).
 */
export declare function ticketNodeId(ticket: string): string;
import type { AllocBodyWriterFn, Bridge, EndpointInfo, EndpointStats, IrohNode, NodeAddrInfo, PeerDiscoveryEvent, PeerStats, RawConnectFn, RawFetchFn, RawServeFn } from "./bridge.js";
import type { RawSessionFns } from "./session.js";
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
    /** Endpoint-level observability snapshot. */
    stats?(endpointHandle: number): Promise<EndpointStats>;
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
export declare function buildNode(config: BuildNodeConfig): IrohNode;
//# sourceMappingURL=index.d.ts.map