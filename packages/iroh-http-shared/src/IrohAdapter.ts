import type { PeerStats, EndpointStats } from './observability.js';
import type { PeerDiscoveryEvent } from './discovery.js';
import type { TransportEventPayload } from './observability.js';
import type { RawSessionFns } from './session.js';

export interface FfiRequest {
  method: string;
  url: string;
  headers: [string, string][];
  remoteNodeId: string;
}

export interface FfiResponseHead {
  status: number;
  headers: [string, string][];
}

export interface FfiResponse extends FfiResponseHead {
  bodyHandle: bigint;
  url: string;
}

export interface RequestPayload extends FfiRequest {
  reqHandle: bigint;
  reqBodyHandle: bigint;
  resBodyHandle: bigint;
  isBidi: boolean;
}

export interface FfiDuplexStream {
  readHandle: bigint;
  writeHandle: bigint;
}

export interface BidirectionalStream {
  readable: ReadableStream<Uint8Array>;
  writable: WritableStream<Uint8Array>;
}

export interface PeerConnectionEvent {
  peerId: string;
  connected: boolean;
}

export interface EndpointInfo {
  endpointHandle: number;
  nodeId: string;
  keypair: Uint8Array;
}

export interface NodeAddrInfo {
  id: string;
  addrs: string[];
}

export interface IrohFetchInit extends RequestInit {
  directAddrs?: string[];
}

export interface CloseOptions {
  force?: boolean;
}

export type RawServeFn = (
  endpointHandle: number,
  options: { onConnectionEvent?: (event: PeerConnectionEvent) => void },
  callback: (payload: RequestPayload) => Promise<FfiResponseHead>,
) => Promise<void>;

export type RawFetchFn = (
  endpointHandle: number,
  nodeId: string,
  url: string,
  method: string,
  headers: [string, string][],
  reqBodyHandle: bigint | null,
  fetchToken: bigint,
  directAddrs: string[] | null,
) => Promise<FfiResponse>;

export type AllocBodyWriterFn = () => bigint | Promise<bigint>;

export type RawConnectFn = (
  endpointHandle: number,
  nodeId: string,
  path: string,
  headers: [string, string][],
) => Promise<FfiDuplexStream>;

export abstract class IrohAdapter {
  // ── Required: body streaming ────────────────────────────────────────────────
  abstract nextChunk(handle: bigint): Promise<Uint8Array | null>;
  abstract sendChunk(handle: bigint, chunk: Uint8Array): Promise<void>;
  abstract finishBody(handle: bigint): Promise<void>;
  abstract cancelRequest(handle: bigint): Promise<void>;
  abstract allocFetchToken(endpointHandle: number): Promise<bigint>;
  abstract cancelFetch(token: bigint): void;
  abstract allocBodyWriter(endpointHandle: number): bigint | Promise<bigint>;

  // ── Required: raw transport ─────────────────────────────────────────────────
  abstract rawFetch(
    endpointHandle: number,
    nodeId: string,
    url: string,
    method: string,
    headers: [string, string][],
    reqBodyHandle: bigint | null,
    fetchToken: bigint,
    directAddrs: string[] | null,
  ): Promise<FfiResponse>;

  abstract rawServe(
    endpointHandle: number,
    options: { onConnectionEvent?: (event: PeerConnectionEvent) => void },
    callback: (payload: RequestPayload) => Promise<FfiResponseHead>,
  ): Promise<void>;

  // ── Required: endpoint lifecycle ────────────────────────────────────────────
  abstract closeEndpoint(handle: number, force?: boolean): Promise<void>;
  abstract stopServe(handle: number): void;
  abstract waitEndpointClosed(handle: number): Promise<void>;

  // ── Required: address / stats ───────────────────────────────────────────────
  abstract nodeAddr(endpointHandle: number): Promise<NodeAddrInfo>;
  abstract nodeTicket(endpointHandle: number): Promise<string>;
  abstract homeRelay(endpointHandle: number): Promise<string | null>;
  abstract peerInfo(endpointHandle: number, nodeId: string): Promise<NodeAddrInfo | null>;
  abstract peerStats(endpointHandle: number, nodeId: string): Promise<PeerStats | null>;
  abstract stats(endpointHandle: number): Promise<EndpointStats>;

  // ── Optional: raw connect ────────────────────────────────────────────────────
  rawConnect(
    _endpointHandle: number,
    _nodeId: string,
    _path: string,
    _headers: [string, string][],
  ): Promise<FfiDuplexStream> {
    return Promise.reject(new Error(`rawConnect() not supported by this adapter`));
  }

  // ── Optional: sessions ──────────────────────────────────────────────────────
  get sessionFns(): RawSessionFns | null { return null; }

  // ── Optional: mDNS discovery ────────────────────────────────────────────────
  mdnsBrowse(_endpointHandle: number, _serviceName: string): Promise<number> {
    return Promise.reject(new Error(`mdnsBrowse() not supported by this adapter`));
  }
  mdnsNextEvent(_browseHandle: number): Promise<PeerDiscoveryEvent | null> {
    return Promise.reject(new Error(`mdnsNextEvent() not supported by this adapter`));
  }
  mdnsBrowseClose(_browseHandle: number): void { /* no-op */ }
  mdnsAdvertise(_endpointHandle: number, _serviceName: string): Promise<number> {
    return Promise.reject(new Error(`mdnsAdvertise() not supported by this adapter`));
  }
  mdnsAdvertiseClose(_advertiseHandle: number): void { /* no-op */ }

  // ── Optional: transport events ──────────────────────────────────────────────
  pollTransportEvent(_endpointHandle: number): Promise<TransportEventPayload | null> {
    return Promise.resolve(null);
  }
}
