/**
 * iroh-http-tauri — guest-js entry point.
 *
 * Implements the `Bridge` interface using Tauri `invoke()` calls and wires
 * it into iroh-http-shared to export the standard `createNode` API.
 *
 * ```ts
 * import { createNode } from 'iroh-http-tauri';
 *
 * const node = await createNode({ key: savedKey });
 * node.serve({}, req => new Response('hello'));
 * const res = await node.fetch(peerId, '/api');
 * ```
 */

import { invoke, Channel } from "@tauri-apps/api/core";

// ── Base64 helpers ────────────────────────────────────────────────────────────

function encodeBase64(u8: Uint8Array): string {
  const CHUNK = 0x8000; // 32 KB — safe for String.fromCharCode spread
  const parts: string[] = [];
  for (let i = 0; i < u8.length; i += CHUNK)
    parts.push(String.fromCharCode(...u8.subarray(i, i + CHUNK)));
  return btoa(parts.join(""));
}

function decodeBase64(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
import {
  buildNode,
  type Bridge,
  type FfiResponse,
  type FfiResponseHead,
  type FfiDuplexStream,
  type NodeOptions,
  type LifecycleOptions,
  type IrohNode,
  type RawFetchFn,
  type RawServeFn,
  type RawConnectFn,
  type AllocBodyWriterFn,
  type RequestPayload,
  type AddrFunctions,
  type DiscoveryFunctions,
  type NodeAddrInfo,
  type PeerStats,
  type PeerDiscoveryEvent,
  type RelayMode,
  type DiscoveryOptions,
  classifyBindError,
  type SecretKey,
} from "iroh-http-shared";

const PLUGIN = "plugin:iroh-http";

// ── Bridge implementation ─────────────────────────────────────────────────────

const bridge: Bridge = {
  nextChunk(handle: number): Promise<Uint8Array | null> {
    return invoke<string | null>(`${PLUGIN}|next_chunk`, { handle }).then(
      (b64) => (b64 ? decodeBase64(b64) : null)
    );
  },

  sendChunk(handle: number, chunk: Uint8Array): Promise<void> {
    return invoke(`${PLUGIN}|send_chunk`, {
      handle,
      chunk: encodeBase64(chunk),
    });
  },

  finishBody(handle: number): Promise<void> {
    return invoke(`${PLUGIN}|finish_body`, { handle });
  },

  cancelRequest(handle: number): Promise<void> {
    return invoke(`${PLUGIN}|cancel_request`, { handle });
  },

  allocFetchToken(): Promise<number> {
    return invoke<number>(`${PLUGIN}|alloc_fetch_token`);
  },

  cancelFetch(token: number): void {
    void invoke(`${PLUGIN}|cancel_in_flight`, { token });
  },

  async nextTrailer(handle: number): Promise<[string, string][] | null> {
    const rows = await invoke<string[][] | null>(`${PLUGIN}|next_trailer`, { handle });
    return rows ? (rows as [string, string][]) : null;
  },

  sendTrailers(handle: number, trailers: [string, string][]): Promise<void> {
    return invoke(`${PLUGIN}|send_trailers`, {
      handle,
      trailers,
    });
  },
};

// ── Platform functions ────────────────────────────────────────────────────────

const rawFetch: RawFetchFn = async (
  endpointHandle,
  nodeId,
  url,
  method,
  headers,
  reqBodyHandle,
  fetchToken,
  directAddrs
) => {
  const res = await invoke<{
    status: number;
    headers: string[][];
    bodyHandle: number;
    url: string;
    trailersHandle: number;
  }>(`${PLUGIN}|raw_fetch`, {
    args: {
      endpointHandle,
      nodeId,
      url,
      method,
      headers,
      reqBodyHandle: reqBodyHandle ?? null,
      fetchToken,
      directAddrs: directAddrs ?? null,
    },
  });
  return {
    status: res.status,
    headers: res.headers as [string, string][],
    bodyHandle: res.bodyHandle,
    url: res.url,
    trailersHandle: res.trailersHandle,
  } satisfies FfiResponse;
};

/** Tauri-specific request payload shape (camelCase, serialised from Rust). */
interface TauriRequestPayload {
  reqHandle: number;
  reqBodyHandle: number;
  resBodyHandle: number;
  reqTrailersHandle: number;
  resTrailersHandle: number;
  isBidi: boolean;
  method: string;
  url: string;
  headers: string[][];
  remoteNodeId: string;
}

const rawServe: RawServeFn = (
  endpointHandle,
  _options,
  callback: (payload: RequestPayload) => Promise<FfiResponseHead>
) => {
  const channel = new Channel<TauriRequestPayload>();

  channel.onmessage = async (raw: TauriRequestPayload) => {
    const payload: RequestPayload = {
      reqHandle: raw.reqHandle,
      reqBodyHandle: raw.reqBodyHandle,
      resBodyHandle: raw.resBodyHandle,
      reqTrailersHandle: raw.reqTrailersHandle,
      resTrailersHandle: raw.resTrailersHandle,
      isBidi: raw.isBidi,
      method: raw.method,
      url: raw.url,
      headers: raw.headers as [string, string][],
      remoteNodeId: raw.remoteNodeId,
    };

    try {
      const head = await callback(payload);
      await invoke(`${PLUGIN}|respond_to_request`, {
        args: {
          reqHandle: payload.reqHandle,
          status: head.status,
          headers: head.headers,
        },
      });
    } catch (err) {
      console.error("[iroh-http-tauri] handler error:", err);
      await invoke(`${PLUGIN}|respond_to_request`, {
        args: { reqHandle: raw.reqHandle, status: 500, headers: [] },
      }).catch(() => {/* ignore */});
    }
  };

  invoke(`${PLUGIN}|serve`, { endpointHandle, channel }).catch((err: unknown) =>
    console.error("[iroh-http-tauri] serve error:", err)
  );
};

const allocBodyWriter: AllocBodyWriterFn = (): Promise<number> => {
  return invoke<number>(`${PLUGIN}|alloc_body_writer`);
};

const rawConnect: RawConnectFn = async (
  endpointHandle,
  nodeId,
  path,
  headers
) => {
  const res = await invoke<{ readHandle: number; writeHandle: number }>(
    `${PLUGIN}|raw_connect`,
    {
      args: { endpointHandle, nodeId, path, headers },
    }
  );
  return {
    readHandle: res.readHandle,
    writeHandle: res.writeHandle,
  } satisfies FfiDuplexStream;
};

// ── Mobile lifecycle listener ─────────────────────────────────────────────────

function installLifecycleListener(
  endpointHandle: number,
  options: LifecycleOptions,
  onDead: () => void,
): (() => void) | undefined {
  if (typeof document === "undefined") return;
  const isMobile = /android|iphone|ipad/i.test(navigator.userAgent);
  if (!isMobile && !options.autoReconnect) return;

  let retries = 0;
  const maxRetries = options.maxRetries ?? 3;
  const handler = async () => {
    if (document.visibilityState !== "visible") return;
    retries = 0;
    while (retries < maxRetries) {
      try {
        await invoke(`${PLUGIN}|ping`, { endpointHandle });
        return;
      } catch {
        retries++;
        if (retries < maxRetries) {
          await new Promise<void>(r => setTimeout(r, 100 * 2 ** retries));
        }
      }
    }
    onDead();
  };
  document.addEventListener("visibilitychange", handler);
  return () => document.removeEventListener("visibilitychange", handler);
}

// ── Public API ────────────────────────────────────────────────────────────────

/** Normalise `relayMode` into flat fields for the Rust adapter. */
function normaliseRelayMode(mode?: RelayMode): {
  relayMode: string | undefined;
  relays: string[] | null;
  disableNetworking: boolean;
} {
  if (mode === "disabled") return { relayMode: "disabled", relays: [], disableNetworking: true };
  if (mode === "default" || mode === undefined) return { relayMode: undefined, relays: null, disableNetworking: false };
  if (mode === "staging") return { relayMode: "staging", relays: null, disableNetworking: false };
  if (Array.isArray(mode)) return { relayMode: "custom", relays: mode, disableNetworking: false };
  return { relayMode: "custom", relays: [mode], disableNetworking: false };
}

/** Normalise DiscoveryOptions into flat fields for the Rust adapter. */
function normaliseDiscovery(disc?: DiscoveryOptions): {
  dnsEnabled: boolean;
} {
  if (!disc) return { dnsEnabled: true };
  const dnsEnabled = disc.dns !== false;
  return { dnsEnabled };
}

/** Address introspection functions backed by Tauri invoke calls. */
const tauriAddrFns: AddrFunctions = {
  nodeAddr: async (handle) => {
    return invoke<NodeAddrInfo>(`${PLUGIN}|node_addr`, { endpointHandle: handle });
  },
  nodeTicket: async (handle) => {
    return invoke<string>(`${PLUGIN}|node_ticket`, { endpointHandle: handle });
  },
  homeRelay: async (handle) => {
    return invoke<string | null>(`${PLUGIN}|home_relay`, { endpointHandle: handle });
  },
  peerInfo: async (handle, nodeId) => {
    return invoke<NodeAddrInfo | null>(`${PLUGIN}|peer_info`, { endpointHandle: handle, nodeId });
  },
  peerStats: async (handle, nodeId) => {
    return invoke<PeerStats | null>(`${PLUGIN}|peer_stats`, { endpointHandle: handle, nodeId });
  },
};

/** Discovery functions backed by Tauri invoke calls. */
const tauriDiscoveryFns: DiscoveryFunctions = {
  mdnsBrowse: async (handle, serviceName) => {
    return invoke<number>(`${PLUGIN}|mdns_browse`, { endpointHandle: handle, serviceName });
  },
  mdnsNextEvent: async (browseHandle) => {
    return invoke<PeerDiscoveryEvent | null>(`${PLUGIN}|mdns_next_event`, { browseHandle });
  },
  mdnsBrowseClose: (browseHandle) => {
    void invoke(`${PLUGIN}|mdns_browse_close`, { browseHandle });
  },
  mdnsAdvertise: async (handle, serviceName) => {
    return invoke<number>(`${PLUGIN}|mdns_advertise`, { endpointHandle: handle, serviceName });
  },
  mdnsAdvertiseClose: (advertiseHandle) => {
    void invoke(`${PLUGIN}|mdns_advertise_close`, { advertiseHandle });
  },
};

/**
 * Create an Iroh node for peer-to-peer HTTP inside a Tauri application.
 */
export async function createNode(options?: NodeOptions): Promise<IrohNode> {
  const keyBytes: string | null = options?.key
    ? encodeBase64(options.key instanceof Uint8Array ? options.key : (options.key as SecretKey).toBytes())
    : null;

  const { relayMode, relays, disableNetworking } = normaliseRelayMode(options?.relayMode);
  const discovery = normaliseDiscovery(options?.discovery);
  const bindAddrs = options?.bindAddr
    ? (Array.isArray(options.bindAddr) ? options.bindAddr : [options.bindAddr])
    : null;

  const info = await invoke<{
    endpointHandle: number;
    nodeId: string;
    keypair: number[];
  }>(`${PLUGIN}|create_endpoint`, {
    args: options
      ? {
          key: keyBytes,
          idleTimeout: options.idleTimeout ?? null,
          relayMode: relayMode ?? null,
          relays,
          bindAddrs,
          dnsDiscovery: options.dnsDiscovery ?? null,
          dnsDiscoveryEnabled: discovery.dnsEnabled,
          channelCapacity: options.channelCapacity ?? null,
          maxChunkSizeBytes: options.maxChunkSizeBytes ?? null,
          maxConsecutiveErrors: options.maxConsecutiveErrors ?? null,
          drainTimeout: options.drainTimeout ?? null,
          handleTtl: options.handleTtl ?? null,
          disableNetworking,
          proxyUrl: options.proxyUrl ?? null,
          proxyFromEnv: options.proxyFromEnv ?? null,
          keylog: options.keylog ?? null,
          compressionLevel: typeof options.compression === "object"
            ? options.compression.level ?? null : options.compression ? 3 : null,
          compressionMinBodyBytes: typeof options.compression === "object"
            ? options.compression.minBodyBytes ?? null : null,
          maxConcurrency: options.maxConcurrency ?? null,
          maxConnectionsPerPeer: options.maxConnectionsPerPeer ?? null,
          requestTimeout: options.requestTimeout ?? null,
          maxRequestBodyBytes: options.maxRequestBodyBytes ?? null,
        }
      : null,
  }).catch((e: unknown) => { throw classifyBindError(e); });

  const node = buildNode(
    bridge,
    {
      endpointHandle: info.endpointHandle,
      nodeId: info.nodeId,
      keypair: new Uint8Array(info.keypair),
    },
    rawFetch,
    rawServe,
    rawConnect,
    allocBodyWriter,
    (handle) => invoke(`${PLUGIN}|close_endpoint`, { endpointHandle: handle }),
    (handle) => { invoke(`${PLUGIN}|stop_serve`, { endpointHandle: handle }).catch(() => {}); },
    tauriAddrFns,
    tauriDiscoveryFns,
  );

  // Install lifecycle listener for mobile/reconnect support.
  if (options?.lifecycle) {
    installLifecycleListener(
      info.endpointHandle,
      options.lifecycle,
      () => {
        // Resolve the closed promise to signal the node is dead.
        node.close().catch(() => {/* already closed */});
      },
    );
  }

  return node;
}

export type { NodeOptions, IrohNode };
