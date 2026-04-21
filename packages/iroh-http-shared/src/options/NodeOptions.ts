import type { SecretKey } from "../keys.js";

export type RelayMode = "default" | "staging" | "disabled" | string | string[];

export interface NodeOptions {
  key?: SecretKey | Uint8Array;
  relayMode?: RelayMode;
  bindAddr?: string | string[];
  idleTimeout?: number;
  discovery?: {
    dns?: boolean | { serverUrl?: string };
    mdns?: boolean | { serviceName?: string };
  };
  proxyUrl?: string;
  proxyFromEnv?: boolean;
  keylog?: boolean;
  advanced?: {
    channelCapacity?: number;
    maxChunkSizeBytes?: number;
    drainTimeout?: number;
    handleTtl?: number;
    maxConsecutiveErrors?: number;
  };
  maxPooledConnections?: number;
  poolIdleTimeoutMs?: number;
  compression?: boolean | { level?: number; minBodyBytes?: number };
  maxConcurrency?: number;
  maxConnectionsPerPeer?: number;
  requestTimeout?: number;
  maxRequestBodyBytes?: number;
  maxHeaderBytes?: number;
  maxTotalConnections?: number;
  reconnect?: { auto?: boolean; maxRetries?: number };
  disableNetworking?: boolean;
  observability?: {
    transportEvents?: boolean;
  };
}
