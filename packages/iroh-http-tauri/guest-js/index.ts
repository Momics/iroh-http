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

import { Channel, invoke } from "@tauri-apps/api/core";
import {
  type AddrFunctions,
  buildNode,
  classifyBindError,
  decodeBase64,
  type DiscoveryFunctions,
  encodeBase64,
  type EndpointStats,
  type IrohNode,
  type NodeAddrInfo,
  type NodeOptions,
  normaliseRelayMode,
  type PeerConnectionEvent,
  type PeerDiscoveryEvent,
  type PeerStats,
  type RelayMode,
  type SecretKey,
} from "@momics/iroh-http-shared";
import type {
  AllocBodyWriterFn,
  Bridge,
  FfiDuplexStream,
  FfiResponse,
  FfiResponseHead,
  RawConnectFn,
  RawFetchFn,
  RawServeFn,
  RawSessionFns,
  RequestPayload,
} from "@momics/iroh-http-shared/adapter";

const PLUGIN = "plugin:iroh-http";

// ── Bridge implementation ─────────────────────────────────────────────────────

const bridge: Bridge = {
  nextChunk(handle: bigint): Promise<Uint8Array | null> {
    return invoke<string | null>(`${PLUGIN}|next_chunk`, {
      handle: Number(handle),
    }).then(
      (b64) => (b64 ? decodeBase64(b64) : null),
    );
  },

  sendChunk(handle: bigint, chunk: Uint8Array): Promise<void> {
    return invoke(`${PLUGIN}|send_chunk`, {
      handle: Number(handle),
      chunk: encodeBase64(chunk),
    });
  },

  finishBody(handle: bigint): Promise<void> {
    return invoke(`${PLUGIN}|finish_body`, { handle: Number(handle) });
  },

  cancelRequest(handle: bigint): Promise<void> {
    return invoke(`${PLUGIN}|cancel_request`, { handle: Number(handle) });
  },

  allocFetchToken(endpointHandle: number): Promise<bigint> {
    return invoke<number>(`${PLUGIN}|alloc_fetch_token`, { endpointHandle }).then(BigInt);
  },

  cancelFetch(token: bigint): void {
    void invoke(`${PLUGIN}|cancel_in_flight`, { token: Number(token) });
  },

  async nextTrailer(handle: bigint): Promise<[string, string][] | null> {
    const rows = await invoke<string[][] | null>(`${PLUGIN}|next_trailer`, {
      handle: Number(handle),
    });
    return rows ? (rows as [string, string][]) : null;
  },

  sendTrailers(handle: bigint, trailers: [string, string][]): Promise<void> {
    return invoke(`${PLUGIN}|send_trailers`, {
      handle: Number(handle),
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
  directAddrs,
) => {
  const res = await invoke<{
    status: number;
    headers: string[][];
    bodyHandle: number;
    url: string;
    trailersHandle: number;
  }>(`${PLUGIN}|raw_fetch`, {
    args: {
      endpointHandle: Number(endpointHandle),
      nodeId,
      url,
      method,
      headers,
      reqBodyHandle: reqBodyHandle != null ? Number(reqBodyHandle) : null,
      fetchToken: fetchToken != null ? Number(fetchToken) : null,
      directAddrs: directAddrs ?? null,
    },
  });
  return {
    status: res.status,
    headers: res.headers as [string, string][],
    bodyHandle: BigInt(res.bodyHandle),
    url: res.url,
    trailersHandle: BigInt(res.trailersHandle),
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
  options,
  callback: (payload: RequestPayload) => Promise<FfiResponseHead>,
): Promise<void> => {
  const channel = new Channel<TauriRequestPayload>();

  // Wire optional connection event channel.
  let connChannel: Channel<PeerConnectionEvent> | undefined;
  if (options.onConnectionEvent) {
    const cb = options.onConnectionEvent;
    connChannel = new Channel<PeerConnectionEvent>();
    connChannel.onmessage = (ev: PeerConnectionEvent) => cb(ev);
  }

  channel.onmessage = async (raw: TauriRequestPayload) => {
    const payload: RequestPayload = {
      reqHandle: BigInt(raw.reqHandle),
      reqBodyHandle: BigInt(raw.reqBodyHandle),
      resBodyHandle: BigInt(raw.resBodyHandle),
      reqTrailersHandle: BigInt(raw.reqTrailersHandle),
      resTrailersHandle: BigInt(raw.resTrailersHandle),
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
          reqHandle: Number(payload.reqHandle),
          status: head.status,
          headers: head.headers,
        },
      });
    } catch (err) {
      console.error("[iroh-http-tauri] handler error:", err);
      await invoke(`${PLUGIN}|respond_to_request`, {
        args: { reqHandle: Number(raw.reqHandle), status: 500, headers: [] },
      }).catch(() => {/* ignore */});
    }
  };

  invoke(`${PLUGIN}|serve`, {
    endpointHandle: Number(endpointHandle),
    channel,
    connChannel: connChannel ?? null,
  })
    .catch((err: unknown) =>
      console.error("[iroh-http-tauri] serve error:", err)
    );

  // Return a promise that resolves when the native serve loop has fully exited.
  // The `wait_serve_stop` command blocks until stop_serve() is called and all
  // in-flight requests have been drained on the Rust side.
  return invoke<void>(`${PLUGIN}|wait_serve_stop`, {
    endpointHandle: Number(endpointHandle),
  });
};

const allocBodyWriter: AllocBodyWriterFn = (): Promise<bigint> => {
  return invoke<number>(`${PLUGIN}|alloc_body_writer`).then(BigInt);
};

const rawConnect: RawConnectFn = async (
  endpointHandle,
  nodeId,
  path,
  headers,
) => {
  const res = await invoke<{ readHandle: number; writeHandle: number }>(
    `${PLUGIN}|raw_connect`,
    {
      args: { endpointHandle: Number(endpointHandle), nodeId, path, headers },
    },
  );
  return {
    readHandle: BigInt(res.readHandle),
    writeHandle: BigInt(res.writeHandle),
  } satisfies FfiDuplexStream;
};

// ── Session functions ─────────────────────────────────────────────────────────

const tauriSessionFns: RawSessionFns = {
  connect: async (endpointHandle, nodeId, directAddrs) => {
    return invoke<number>(`${PLUGIN}|session_connect`, {
      args: {
        endpointHandle: Number(endpointHandle),
        nodeId,
        directAddrs: directAddrs ?? null,
      },
    }).then(BigInt);
  },
  createBidiStream: async (sessionHandle) => {
    const res = await invoke<{ readHandle: number; writeHandle: number }>(
      `${PLUGIN}|session_create_bidi_stream`,
      { sessionHandle: Number(sessionHandle) },
    );
    return {
      readHandle: BigInt(res.readHandle),
      writeHandle: BigInt(res.writeHandle),
    } satisfies FfiDuplexStream;
  },
  nextBidiStream: async (sessionHandle) => {
    const res = await invoke<
      { readHandle: number; writeHandle: number } | null
    >(
      `${PLUGIN}|session_next_bidi_stream`,
      { sessionHandle: Number(sessionHandle) },
    );
    return res
      ? {
        readHandle: BigInt(res.readHandle),
        writeHandle: BigInt(res.writeHandle),
      } satisfies FfiDuplexStream
      : null;
  },
  createUniStream: async (sessionHandle) => {
    return invoke<number>(`${PLUGIN}|session_create_uni_stream`, {
      sessionHandle: Number(sessionHandle),
    }).then(BigInt);
  },
  nextUniStream: async (sessionHandle) => {
    const h = await invoke<number | null>(`${PLUGIN}|session_next_uni_stream`, {
      sessionHandle: Number(sessionHandle),
    });
    return h != null ? BigInt(h) : null;
  },
  sendDatagram: async (sessionHandle, data) => {
    const b64 = btoa(String.fromCharCode(...data));
    await invoke<void>(`${PLUGIN}|session_send_datagram`, {
      sessionHandle: Number(sessionHandle),
      data: b64,
    });
  },
  recvDatagram: async (sessionHandle) => {
    const res = await invoke<string | null>(`${PLUGIN}|session_recv_datagram`, {
      sessionHandle: Number(sessionHandle),
    });
    if (res === null) return null;
    const bin = atob(res);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  },
  maxDatagramSize: async (sessionHandle) => {
    return invoke<number | null>(`${PLUGIN}|session_max_datagram_size`, {
      sessionHandle: Number(sessionHandle),
    });
  },
  closed: async (sessionHandle) => {
    return invoke<{ closeCode: number; reason: string }>(
      `${PLUGIN}|session_closed`,
      { sessionHandle: Number(sessionHandle) },
    );
  },
  close: async (sessionHandle, closeCode?, reason?) => {
    await invoke<void>(`${PLUGIN}|session_close`, {
      sessionHandle: Number(sessionHandle),
      closeCode,
      reason,
    });
  },
};

// ── Mobile lifecycle listener ─────────────────────────────────────────────────

function installLifecycleListener(
  endpointHandle: number,
  options: { auto?: boolean; maxRetries?: number },
  onDead: () => void,
): (() => void) | undefined {
  if (typeof document === "undefined") return;
  const isMobile = /android|iphone|ipad/i.test(navigator.userAgent);
  if (!isMobile && !options.auto) return;

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
          await new Promise<void>((r) => setTimeout(r, 100 * 2 ** retries));
        }
      }
    }
    onDead();
  };
  document.addEventListener("visibilitychange", handler);
  return () => document.removeEventListener("visibilitychange", handler);
}

// ── Public API ────────────────────────────────────────────────────────────────

/** Normalise the `discovery` option into flat fields for the Rust adapter. */
function normaliseDiscovery(disc?: NodeOptions["discovery"]): {
  dnsEnabled: boolean;
  dnsServerUrl?: string;
} {
  if (!disc) return { dnsEnabled: true };
  if (disc.dns === false) return { dnsEnabled: false };
  if (typeof disc.dns === "object" && disc.dns !== null) {
    return { dnsEnabled: true, dnsServerUrl: disc.dns.serverUrl };
  }
  return { dnsEnabled: true };
}

/** Address introspection functions backed by Tauri invoke calls. */
const tauriAddrFns: AddrFunctions = {
  nodeAddr: async (handle) => {
    return invoke<NodeAddrInfo>(`${PLUGIN}|node_addr`, {
      endpointHandle: Number(handle),
    });
  },
  nodeTicket: async (handle) => {
    return invoke<string>(`${PLUGIN}|node_ticket`, {
      endpointHandle: Number(handle),
    });
  },
  homeRelay: async (handle) => {
    return invoke<string | null>(`${PLUGIN}|home_relay`, {
      endpointHandle: Number(handle),
    });
  },
  peerInfo: async (handle, nodeId) => {
    return invoke<NodeAddrInfo | null>(`${PLUGIN}|peer_info`, {
      endpointHandle: Number(handle),
      nodeId,
    });
  },
  peerStats: async (handle, nodeId) => {
    return invoke<PeerStats | null>(`${PLUGIN}|peer_stats`, {
      endpointHandle: Number(handle),
      nodeId,
    });
  },
  stats: async (handle) => {
    return invoke<EndpointStats>(`${PLUGIN}|endpoint_stats`, {
      endpointHandle: Number(handle),
    });
  },
};

/** Discovery functions backed by Tauri invoke calls. */
const tauriDiscoveryFns: DiscoveryFunctions = {
  mdnsBrowse: async (handle, serviceName) => {
    return invoke<number>(`${PLUGIN}|mdns_browse`, {
      endpointHandle: Number(handle),
      serviceName,
    });
  },
  mdnsNextEvent: async (browseHandle) => {
    return invoke<PeerDiscoveryEvent | null>(`${PLUGIN}|mdns_next_event`, {
      browseHandle: Number(browseHandle),
    });
  },
  mdnsBrowseClose: (browseHandle) => {
    void invoke(`${PLUGIN}|mdns_browse_close`, {
      browseHandle: Number(browseHandle),
    });
  },
  mdnsAdvertise: async (handle, serviceName) => {
    return invoke<number>(`${PLUGIN}|mdns_advertise`, {
      endpointHandle: Number(handle),
      serviceName,
    });
  },
  mdnsAdvertiseClose: (advertiseHandle) => {
    void invoke(`${PLUGIN}|mdns_advertise_close`, {
      advertiseHandle: Number(advertiseHandle),
    });
  },
};

/**
 * Create an Iroh node for peer-to-peer HTTP inside a Tauri application.
 */
export async function createNode(options?: NodeOptions): Promise<IrohNode> {
  const keyBytes: string | null = options?.key
    ? encodeBase64(
      options.key instanceof Uint8Array
        ? options.key
        : (options.key as SecretKey).toBytes(),
    )
    : null;

  const { relayMode, relays, disableNetworking } = normaliseRelayMode(
    options?.relayMode,
  );
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
        dnsDiscovery: discovery.dnsServerUrl ?? null,
        dnsDiscoveryEnabled: discovery.dnsEnabled,
        channelCapacity: options.advanced?.channelCapacity ?? null,
        maxChunkSizeBytes: options.advanced?.maxChunkSizeBytes ?? null,
        maxConsecutiveErrors: options.advanced?.maxConsecutiveErrors ?? null,
        drainTimeout: options.advanced?.drainTimeout ?? null,
        handleTtl: options.advanced?.handleTtl ?? null,
        maxPooledConnections: options.maxPooledConnections ?? null,
        poolIdleTimeoutMs: options.poolIdleTimeoutMs ?? null,
        disableNetworking,
        proxyUrl: options.proxyUrl ?? null,
        proxyFromEnv: options.proxyFromEnv ?? null,
        keylog: options.keylog ?? null,
        compressionLevel: typeof options.compression === "object"
          ? options.compression.level ?? null
          : options.compression
          ? 3
          : null,
        compressionMinBodyBytes: typeof options.compression === "object"
          ? options.compression.minBodyBytes ?? null
          : null,
        maxConcurrency: options.maxConcurrency ?? null,
        maxConnectionsPerPeer: options.maxConnectionsPerPeer ?? null,
        requestTimeout: options.requestTimeout ?? null,
        maxRequestBodyBytes: options.maxRequestBodyBytes ?? null,
        maxHeaderBytes: options.maxHeaderBytes ?? null,
      }
      : null,
  }).catch((e: unknown) => {
    throw classifyBindError(e);
  });

  const node = buildNode({
    bridge,
    info: {
      endpointHandle: Number(info.endpointHandle),
      nodeId: info.nodeId,
      keypair: new Uint8Array(info.keypair),
    },
    rawFetch,
    rawServe,
    rawConnect,
    allocBodyWriter,
    closeEndpoint: (handle, force?) =>
      invoke(`${PLUGIN}|close_endpoint`, {
        endpointHandle: Number(handle),
        force: force ?? null,
      }),
    stopServe: (handle) => {
      invoke(`${PLUGIN}|stop_serve`, { endpointHandle: Number(handle) }).catch(
        () => {},
      );
    },
    nativeClosed: invoke<void>(`${PLUGIN}|wait_endpoint_closed`, {
      endpointHandle: Number(info.endpointHandle),
    }).then(() => {}),
    addrFns: tauriAddrFns,
    discoveryFns: tauriDiscoveryFns,
    sessionFns: tauriSessionFns,
  });

  // TAURI-005: install lifecycle listener and store the unsubscribe function
  // so it can be called when the node closes, preventing stale callbacks.
  const reconnect = options?.reconnect;
  if (reconnect) {
    const unsubscribe = installLifecycleListener(
      Number(info.endpointHandle),
      reconnect,
      () => {
        // Resolve the closed promise to signal the node is dead.
        node.close().catch(() => {/* already closed */});
      },
    );
    if (unsubscribe) {
      const originalClose = node.close.bind(node);
      (node as { close: () => Promise<void> }).close = async () => {
        unsubscribe();
        return originalClose();
      };
    }
  }

  return node;
}

export type { IrohNode, NodeOptions };
export { PublicKey, SecretKey } from "@momics/iroh-http-shared";
