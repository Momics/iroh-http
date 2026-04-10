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
  type NodeOptions,
  type IrohNode,
  type RawFetchFn,
  type RawServeFn,
  type AllocBodyWriterFn,
  type RequestPayload,
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
  } satisfies FfiResponse;
};

/** Tauri-specific request payload shape (camelCase, serialised from Rust). */
interface TauriRequestPayload {
  reqHandle: number;
  reqBodyHandle: number;
  resBodyHandle: number;
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

// ── Public API ────────────────────────────────────────────────────────────────

/**
 * Create an Iroh node for peer-to-peer HTTP inside a Tauri application.
 */
export async function createNode(options?: NodeOptions): Promise<IrohNode> {
  const info = await invoke<{
    endpointHandle: number;
    nodeId: string;
    keypair: number[];
  }>(`${PLUGIN}|create_endpoint`, {
    args: options
      ? {
          key: options.key ? Array.from(options.key) : null,
          idleTimeout: options.idleTimeout ?? null,
          relays: options.relays ?? null,
          dnsDiscovery: options.dnsDiscovery ?? null,
        }
      : null,
  });

  // Wrap the async alloc_body_writer in a way makeFetch can use.
  return buildNode(
    bridge,
    {
      endpointHandle: info.endpointHandle,
      nodeId: info.nodeId,
      keypair: new Uint8Array(info.keypair),
    },
    rawFetch,
    rawServe,
    allocBodyWriter,
    (handle) => invoke(`${PLUGIN}|close_endpoint`, { endpointHandle: handle })
  );
}

export type { NodeOptions, IrohNode };
