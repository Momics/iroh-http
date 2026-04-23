export type {
  BidirectionalStream,
  CloseOptions,
  EndpointInfo,
  FfiDuplexStream,
  IrohFetchInit,
  NodeAddrInfo,
  PeerConnectionEvent,
} from "./IrohAdapter.js";
export { IrohAdapter } from "./IrohAdapter.js";
export { IrohNode } from "./IrohNode.js";
export type { NodeOptions, RelayMode } from "./options/NodeOptions.js";
export type {
  DiagnosticsEventDetail,
  EndpointStats,
  PathChangeEventDetail,
  PathInfo,
  PeerConnectEventDetail,
  PeerDisconnectEventDetail,
  PeerStats,
} from "./observability.js";
export type {
  AdvertiseOptions,
  BrowseOptions,
  DiscoveredPeer,
  PeerDiscoveryEvent,
} from "./discovery.js";
export type {
  IrohSession,
  RawSessionFns,
  WebTransportBidirectionalStream,
  WebTransportCloseInfo,
  WebTransportDatagramDuplexStream,
} from "./session.js";
export type {
  ServeFn,
  ServeHandle,
  ServeHandler,
  ServeOptions,
} from "./serve.js";
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
export { decodeBase64, encodeBase64, normaliseRelayMode } from "./utils.js";
export type { NormalisedRelay } from "./utils.js";

export function ticketNodeId(ticket: string): string {
  try {
    const info = JSON.parse(ticket) as { id?: string };
    if (info && typeof info.id === "string") return info.id;
  } catch {}
  return ticket;
}
