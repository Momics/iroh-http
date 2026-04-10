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
import {
  buildNode,
  type Bridge,
  type FfiResponse,
  type FfiResponseHead,
  type FfiDuplexStream,
  type NodeOptions,
  type IrohNode,
  type RawFetchFn,
  type RawServeFn,
  type RawConnectFn,
  type AllocBodyWriterFn,
  type RequestPayload,
  classifyBindError,
  type SecretKey,
} from "iroh-http-shared";

const PLUGIN = "plugin:iroh-http";

// ── Bridge implementation ─────────────────────────────────────────────────────

const bridge: Bridge = {
  nextChunk(handle: number): Promise<Uint8Array | null> {
    return invoke<number[] | null>(`${PLUGIN}|next_chunk`, { handle }).then(
      (bytes) => (bytes ? new Uint8Array(bytes) : null)
    );
  },

  sendChunk(handle: number, chunk: Uint8Array): Promise<void> {
    return invoke(`${PLUGIN}|send_chunk`, {
      handle,
      chunk: Array.from(chunk),
    });
  },

  finishBody(handle: number): Promise<void> {
    return invoke(`${PLUGIN}|finish_body`, { handle });
  },

  cancelRequest(handle: number): Promise<void> {
    return invoke(`${PLUGIN}|cancel_request`, { handle });
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
  reqBodyHandle
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
      // Send 500 so Rust can close the stream.
      await invoke(`${PLUGIN}|respond_to_request`, {
        args: { reqHandle: raw.reqHandle, status: 500, headers: [] },
      }).catch(() => {/* ignore */});
    }
  };

  invoke(`${PLUGIN}|serve`, { endpointHandle, channel }).catch((err: unknown) =>
    console.error("[iroh-http-tauri] serve error:", err)
  );
};

/** Allocate a body writer channel handle via the Tauri command. */
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

// ── Public API ────────────────────────────────────────────────────────────────

/**
 * Create an Iroh node for peer-to-peer HTTP inside a Tauri application.
 */
export async function createNode(options?: NodeOptions): Promise<IrohNode> {
  const keyBytes: number[] | null = options?.key
    ? Array.from(options.key instanceof Uint8Array ? options.key : (options.key as SecretKey).toBytes())
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
          relays: options.relays ?? null,
          dnsDiscovery: options.dnsDiscovery ?? null,
        }
      : null,
  }).catch((e: unknown) => { throw classifyBindError(e); });

  return buildNode(
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
    (handle) => invoke(`${PLUGIN}|close_endpoint`, { endpointHandle: handle })
  );
}

export type { NodeOptions, IrohNode };
