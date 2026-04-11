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
  NodeOptions,
  NodeAddrInfo,
} from "@momics/iroh-http-shared";
import { classifyError, classifyBindError } from "@momics/iroh-http-shared";
import type { AddrFunctions } from "@momics/iroh-http-shared";

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
  iroh_http_next_chunk: {
    parameters: ["u32", "buffer", "usize"],
    result: "i32",
    nonblocking: true,
  },
} as const);

// ── JSON dispatch helper ──────────────────────────────────────────────────────

const enc = new TextEncoder();
const dec = new TextDecoder();
// ── Base64 helpers ─────────────────────────────────────────────────────

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
/** Output buffer shared across calls; grows permanently but never shrinks. */
let outBuf = new Uint8Array(128 * 1024);

/** Pre-encoded method name buffers (UTF-8). */
const METHOD_BUFS: Record<string, Uint8Array> = Object.fromEntries(
  [
    "nextChunk", "sendChunk", "finishBody", "cancelRequest",
    "nextTrailer", "sendTrailers", "rawFetch", "rawConnect",
    "serveStart", "nextRequest", "respond", "allocBodyWriter",
    "createEndpoint", "closeEndpoint", "allocFetchToken", "cancelInFlight",
    "nodeAddr", "homeRelay", "peerInfo",
  ].map(m => [m, enc.encode(m)])
);

/** Reusable buffer for raw chunk reads via iroh_http_next_chunk. */
const chunkBuf = new Uint8Array(65536);

async function call<T>(method: string, payload: unknown): Promise<T> {
  const methodBuf  = METHOD_BUFS[method] ?? enc.encode(method);
  const payloadBuf = enc.encode(JSON.stringify(payload));

  let n = await lib.symbols.iroh_http_call(
    methodBuf,  BigInt(methodBuf.byteLength),
    payloadBuf, BigInt(payloadBuf.byteLength),
    outBuf,     BigInt(outBuf.byteLength),
  ) as number;

  if (n < 0) {
    // Output buffer too small; grow permanently and retry once.
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
    throw classifyError(result.err);
  }
  return result.ok;
}

// ── Bridge implementation ─────────────────────────────────────────────────────

export const bridge: Bridge = {
  async nextChunk(handle: number): Promise<Uint8Array | null> {
    let n = await lib.symbols.iroh_http_next_chunk(
      handle, chunkBuf, BigInt(chunkBuf.byteLength),
    ) as number;
    if (n < 0) {
      // Chunk too large for shared buffer; grow and retry once.
      const grown = new Uint8Array(-n);
      n = await lib.symbols.iroh_http_next_chunk(
        handle, grown, BigInt(grown.byteLength),
      ) as number;
      return n > 0 ? grown.subarray(0, n) : null;
    }
    return n > 0 ? chunkBuf.slice(0, n) : null;
  },
  async sendChunk(handle: number, chunk: Uint8Array): Promise<void> {
    await call<Record<never, never>>("sendChunk", { handle, chunk: encodeBase64(chunk) });
  },
  async finishBody(handle: number): Promise<void> {
    await call<Record<never, never>>("finishBody", { handle });
  },
  async cancelRequest(handle: number): Promise<void> {
    await call<Record<never, never>>("cancelRequest", { handle });
  },
  async allocFetchToken(): Promise<number> {
    const res = await call<{ token: number }>("allocFetchToken", {});
    return res.token;
  },
  cancelFetch(token: number): void {
    // Fire-and-forget — do not await.
    void call<Record<never, never>>("cancelInFlight", { token });
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
  fetchToken: number,
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
    fetchToken,
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

/** Normalise `relayMode` into flat fields for the Rust adapter. */
function normaliseRelayMode(mode?: import("@momics/iroh-http-shared").RelayMode): {
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
function normaliseDiscovery(disc?: import("@momics/iroh-http-shared").DiscoveryOptions): {
  mdns: boolean;
  serviceName?: string;
  advertise: boolean;
  dnsEnabled: boolean;
} {
  if (!disc) return { mdns: false, advertise: true, dnsEnabled: true };
  const dnsEnabled = disc.dns !== false;
  if (disc.mdns === true) return { mdns: true, advertise: true, dnsEnabled };
  if (disc.mdns && typeof disc.mdns === "object") {
    return {
      mdns: true,
      advertise: disc.mdns.advertise ?? true,
      serviceName: disc.mdns.serviceName,
      dnsEnabled,
    };
  }
  return { mdns: false, advertise: true, dnsEnabled };
}

export async function createEndpointInfo(options?: NodeOptions): Promise<EndpointInfo> {
  const keyBytes: string | null = options?.key
    ? encodeBase64(options.key instanceof Uint8Array ? options.key : options.key.toBytes())
    : null;

  const { relayMode, relays, disableNetworking } = normaliseRelayMode(options?.relayMode);
  const discovery = normaliseDiscovery(options?.discovery);
  const bindAddrs = options?.bindAddr
    ? (Array.isArray(options.bindAddr) ? options.bindAddr : [options.bindAddr])
    : null;

  const res = await call<{ endpointHandle: number; nodeId: string; keypair: number[] }>(
    "createEndpoint",
    {
      key:                  keyBytes,
      idleTimeout:          options?.idleTimeout ?? null,
      relayMode:            relayMode ?? null,
      relays:               relays ?? null,
      bindAddrs,
      dnsDiscovery:         options?.dnsDiscovery ?? null,
      dnsDiscoveryEnabled:  discovery.dnsEnabled,
      channelCapacity:      options?.channelCapacity ?? null,
      maxChunkSizeBytes:    options?.maxChunkSizeBytes ?? null,
      maxConsecutiveErrors: options?.maxConsecutiveErrors ?? null,
      discoveryMdns:        discovery.mdns,
      discoveryServiceName: discovery.serviceName ?? null,
      discoveryAdvertise:   discovery.advertise,
      drainTimeout:         options?.drainTimeout ?? null,
      handleTtl:            options?.handleTtl ?? null,
      disableNetworking,
      proxyUrl:             options?.proxyUrl ?? null,
      proxyFromEnv:         options?.proxyFromEnv ?? null,
      keylog:               options?.keylog ?? null,
      compressionLevel:     typeof options?.compression === "object"
        ? options.compression.level ?? null : options?.compression ? 3 : null,
      compressionMinBodyBytes: typeof options?.compression === "object"
        ? options.compression.minBodyBytes ?? null : null,
      maxConcurrency:       options?.maxConcurrency ?? null,
      maxConnectionsPerPeer: options?.maxConnectionsPerPeer ?? null,
      requestTimeout:       options?.requestTimeout ?? null,
      maxRequestBodyBytes:  options?.maxRequestBodyBytes ?? null,
    },
  ).catch((e: unknown) => { throw classifyBindError(e); });
  return {
    endpointHandle: res.endpointHandle,
    nodeId:         res.nodeId,
    keypair:        new Uint8Array(res.keypair),
  };
}

export async function closeEndpoint(handle: number): Promise<void> {
  await call<Record<never, never>>("closeEndpoint", { endpointHandle: handle });
}

export function stopServe(handle: number): void {
  call<Record<never, never>>("stopServe", { endpointHandle: handle }).catch(() => {});
}

// ── Address introspection ──────────────────────────────────────────────────────

export const denoAddrFns: AddrFunctions = {
  nodeAddr: async (handle) => {
    const res = await call<NodeAddrInfo>("nodeAddr", { endpointHandle: handle });
    return res;
  },
  nodeTicket: async (handle) => {
    return call<string>("nodeTicket", { endpointHandle: handle });
  },
  homeRelay: async (handle) => {
    const res = await call<string | null>("homeRelay", { endpointHandle: handle });
    return res;
  },
  peerInfo: async (handle, nodeId) => {
    const res = await call<NodeAddrInfo | null>("peerInfo", { endpointHandle: handle, nodeId });
    return res;
  },
};
