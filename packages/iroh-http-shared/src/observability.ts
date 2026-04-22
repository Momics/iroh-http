/** Transport-level event dispatched by IrohNode when observability.transportEvents is true. */
export type TransportEventPayload =
  | { type: "pool:hit"; peerId: string; timestamp: number }
  | { type: "pool:miss"; peerId: string; timestamp: number }
  | { type: "pool:evict"; peerId: string; timestamp: number }
  | {
    type: "path:change";
    peerId: string;
    addr: string;
    relay: boolean;
    timestamp: number;
  }
  | { type: "handle:sweep"; evicted: number; timestamp: number };

export interface EndpointStats {
  activeReaders: number;
  activeWriters: number;
  activeSessions: number;
  totalHandles: number;
  poolSize: number;
  activeConnections: number;
  activeRequests: number;
}

export interface PeerStats {
  relay: boolean;
  relayUrl: string | null;
  paths: PathInfo[];
  rttMs: number | null;
  bytesSent: number | null;
  bytesReceived: number | null;
  lostPackets: number | null;
  sentPackets: number | null;
  congestionWindow: number | null;
}

export interface PathInfo {
  relay: boolean;
  addr: string;
  active: boolean;
}
