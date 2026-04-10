/**
 * iroh-http-deno — DenoAdapter.
 *
 * Implements the Bridge interface using Deno.dlopen FFI and exposes the
 * raw platform functions needed by iroh-http-shared's buildNode.
 */

import { resolve, dirname, fromFileUrl } from "@std/path";
import type {
  Bridge,
  EndpointInfo,
  FfiResponse,
  FfiResponseHead,
  FfiDuplexStream,
  RawFetchFn,
  RawServeFn,
  RawConnectFn,
  AllocBodyWriterFn,
  RequestPayload,
} from "iroh-http-shared";

// ── Platform library resolution ───────────────────────────────────────────────

function libExtension(): string {
  switch (Deno.build.os) {
    case "darwin":  return "dylib";
    case "windows": return "dll";
    default:        return "so";
  }
}

function libName(): string {
  return `libiroh_http_deno.${Deno.build.os}-${Deno.build.arch}.${libExtension()}`;
}

const LIB_DIR  = resolve(dirname(fromFileUrl(import.meta.url)), "..", "lib");
const LIB_PATH = resolve(LIB_DIR, libName());

// ── FFI symbols ───────────────────────────────────────────────────────────────

const lib = Deno.dlopen(LIB_PATH, {
  iroh_http_call: {
    parameters: ["buffer", "usize", "buffer", "usize", "buffer", "usize"],
    result: "i32",
    nonblocking: true,
  },
} as const);

// ── JSON dispatch helper ──────────────────────────────────────────────────────

const enc = new TextEncoder();
const dec = new TextDecoder();

/** Initial output buffer size.  Grown automatically on overflow. */
const INITIAL_BUF = 4096;

async function call<T>(method: string, payload: unknown): Promise<T> {
  const methodBuf  = enc.encode(method);
  const payloadBuf = enc.encode(JSON.stringify(payload));
  let   outBuf     = new Uint8Array(INITIAL_BUF);

  let n = await lib.symbols.iroh_http_call(
    methodBuf,  BigInt(methodBuf.byteLength),
    payloadBuf, BigInt(payloadBuf.byteLength),
    outBuf,     BigInt(outBuf.byteLength),
  ) as number;

  if (n < 0) {
    // Output buffer was too small; allocate the required size and retry once.
    outBuf = new Uint8Array(-n);
    n = await lib.symbols.iroh_http_call(
      methodBuf,  BigInt(methodBuf.byteLength),
      payloadBuf, BigInt(payloadBuf.byteLength),
      outBuf,     BigInt(outBuf.byteLength),
    ) as number;
  }

  const result = JSON.parse(dec.decode(outBuf.subarray(0, n))) as
    | { ok: T }
    | { err: string };

  if ("err" in result) {
    throw new Error(`[iroh-http-deno] ${method}: ${result.err}`);
  }
  return result.ok;
}

// ── Bridge implementation ─────────────────────────────────────────────────────

export const bridge: Bridge = {
  async nextChunk(handle: number): Promise<Uint8Array | null> {
    const res = await call<{ chunk: number[] | null }>("nextChunk", { handle });
    return res.chunk ? new Uint8Array(res.chunk) : null;
  },
  async sendChunk(handle: number, chunk: Uint8Array): Promise<void> {
    await call<Record<never, never>>("sendChunk", { handle, chunk: Array.from(chunk) });
  },
  async finishBody(handle: number): Promise<void> {
    await call<Record<never, never>>("finishBody", { handle });
  },
  async cancelRequest(handle: number): Promise<void> {
    await call<Record<never, never>>("cancelRequest", { handle });
  },
  async nextTrailer(handle: number): Promise<[string, string][] | null> {
    const res = await call<{ trailers: [string, string][] | null }>("nextTrailer", { handle });
    return res.trailers;
  },
  async sendTrailers(handle: number, trailers: [string, string][]): Promise<void> {
    await call<Record<never, never>>("sendTrailers", { handle, trailers });
  },
};

// ── Platform functions ────────────────────────────────────────────────────────

export const rawFetch: RawFetchFn = async (
  endpointHandle: number,
  nodeId: string,
  url: string,
  method: string,
  headers: [string, string][],
  reqBodyHandle: number | null,
) => {
  const res = await call<{
    status: number;
    headers: [string, string][];
    bodyHandle: number;
    url: string;
    trailersHandle: number;
  }>("rawFetch", {
    endpointHandle,
    nodeId,
    url,
    method,
    headers,
    reqBodyHandle: reqBodyHandle ?? null,
  });
  return {
    status:         res.status,
    headers:        res.headers,
    bodyHandle:     res.bodyHandle,
    url:            res.url,
    trailersHandle: res.trailersHandle,
  } satisfies FfiResponse;
};

export const rawConnect: RawConnectFn = async (
  endpointHandle: number,
  nodeId: string,
  path: string,
  headers: [string, string][],
) => {
  const res = await call<{ readHandle: number; writeHandle: number }>(
    "rawConnect",
    { endpointHandle, nodeId, path, headers },
  );
  return {
    readHandle:  res.readHandle,
    writeHandle: res.writeHandle,
  } satisfies FfiDuplexStream;
};

/**
 * Polling serve loop.
 *
 * 1. `serveStart` tells Rust to begin accepting connections and feeding them
 *    into the per-endpoint mpsc queue.
 * 2. `nextRequest` blocks (nonblocking: true → Promise) until the next item
 *    is queued.  Returns `null` when the endpoint closes.
 * 3. Each request is dispatched to the user callback in the background.
 */
export const rawServe: RawServeFn = (
  endpointHandle: number,
  _options: Record<string, unknown>,
  callback: (payload: RequestPayload) => Promise<FfiResponseHead>,
) => {
  call<Record<never, never>>("serveStart", { endpointHandle })
    .then(() => {
      (async () => {
        while (true) {
          const payload = await call<RequestPayload | null>(
            "nextRequest",
            { endpointHandle },
          );
          if (payload === null) break;

          // Handle in the background — do not await.
          (async () => {
            try {
              const head = await callback(payload);
              await call<Record<never, never>>("respond", {
                reqHandle: payload.reqHandle,
                status:    head.status,
                headers:   head.headers,
              });
            } catch (err) {
              console.error("[iroh-http-deno] handler error:", err);
              await call<Record<never, never>>("respond", {
                reqHandle: payload.reqHandle,
                status:    500,
                headers:   [],
              }).catch(() => { /* ignore */ });
            }
          })();
        }
      })().catch((err) =>
        console.error("[iroh-http-deno] serve loop error:", err)
      );
    })
    .catch((err) => console.error("[iroh-http-deno] serveStart error:", err));
};

export const allocBodyWriter: AllocBodyWriterFn = () =>
  call<{ handle: number }>("allocBodyWriter", {}).then((r) => r.handle);

// ── Endpoint lifecycle ────────────────────────────────────────────────────────

export async function createEndpointInfo(options?: {
  key?: Uint8Array;
  idleTimeout?: number;
  relays?: string[];
  dnsDiscovery?: string;
}): Promise<EndpointInfo> {
  const res = await call<{ endpointHandle: number; nodeId: string; keypair: number[] }>(
    "createEndpoint",
    {
      key:          options?.key ? Array.from(options.key) : null,
      idleTimeout:  options?.idleTimeout ?? null,
      relays:       options?.relays ?? null,
      dnsDiscovery: options?.dnsDiscovery ?? null,
    },
  );
  return {
    endpointHandle: res.endpointHandle,
    nodeId:         res.nodeId,
    keypair:        new Uint8Array(res.keypair),
  };
}

export async function closeEndpoint(handle: number): Promise<void> {
  await call<Record<never, never>>("closeEndpoint", { endpointHandle: handle });
}
