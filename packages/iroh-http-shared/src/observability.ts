/** Payload for a transport-level event dispatched by IrohNode when observability.transportEvents is true. */
export interface TransportEventPayload {
  type: 'pool:hit' | 'pool:miss' | 'pool:evict' | 'path:change' | 'handle:sweep';
  peerId?: string;
  timestamp: number;
  data: Record<string, number | string | boolean>;
}

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
