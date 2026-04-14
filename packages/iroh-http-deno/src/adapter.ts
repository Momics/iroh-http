/**
 * iroh-http-deno — DenoAdapter.
 *
 * Implements the Bridge interface using Deno.dlopen FFI and exposes the
 * raw platform functions needed by iroh-http-shared's buildNode.
 */

import { resolve, dirname, fromFileUrl } from "@std/path";
import type {
  EndpointInfo,
  NodeOptions,
  NodeAddrInfo,
  PeerDiscoveryEvent,
  PeerStats,
} from "@momics/iroh-http-shared";
import type {
  Bridge,
  FfiResponse,
  FfiResponseHead,
  FfiDuplexStream,
  RawFetchFn,
  RawServeFn,
  RawConnectFn,
  AllocBodyWriterFn,
  RequestPayload,
  RawSessionFns,
} from "@momics/iroh-http-shared/adapter";
import { classifyError, classifyBindError, encodeBase64, decodeBase64, normaliseRelayMode } from "@momics/iroh-http-shared";
import type {
  AddrFunctions,
  DiscoveryFunctions,
} from "@momics/iroh-http-shared";

// ── Platform library resolution ───────────────────────────────────────────────

function libExtension(): string {
  switch (Deno.build.os) {
    case "darwin":
      return "dylib";
    case "windows":
      return "dll";
    default:
      return "so";
  }
}

function libName(): string {
  return `libiroh_http_deno.${Deno.build.os}-${Deno.build.arch}.${libExtension()}`;
}

const LIB_DIR = resolve(dirname(fromFileUrl(import.meta.url)), "..", "lib");
const LIB_PATH = resolve(LIB_DIR, libName());

// ── FFI symbols ───────────────────────────────────────────────────────────────

const lib = Deno.dlopen(LIB_PATH, {
  iroh_http_call: {
    parameters: ["buffer", "usize", "buffer", "usize", "buffer", "usize"],
    result: "i32",
    nonblocking: true,
  },
  iroh_http_next_chunk: {
    parameters: ["u64", "buffer", "usize"],
    result: "i32",
    nonblocking: true,
  },
} as const);

// ── JSON dispatch helper ──────────────────────────────────────────────────────

const enc = new TextEncoder();
const dec = new TextDecoder();

/**
 * Capacity hint for per-call output buffers.  Each call allocates its own
 * buffer of this size so concurrent `nonblocking` FFI calls never alias the
 * same memory.  Grows when any call needs more space; never shrinks.
 */
let outBufHint = 128 * 1024;

/** Pre-encoded method name buffers (UTF-8). */
const METHOD_BUFS: Record<string, Uint8Array> = Object.fromEntries(
  [
    "nextChunk",
    "sendChunk",
    "finishBody",
    "cancelRequest",
    "nextTrailer",
    "sendTrailers",
    "rawFetch",
    "rawConnect",
    "serveStart",
    "nextRequest",
    "respond",
    "allocBodyWriter",
    "createEndpoint",
    "closeEndpoint",
    "allocFetchToken",
    "cancelInFlight",
    "nodeAddr",
    "homeRelay",
    "peerInfo",
  ].map((m) => [m, enc.encode(m)]),
);

/** Reusable buffer hint for estimating output size of `call()` responses. */async function call<T>(method: string, payload: unknown): Promise<T> {
  const methodBuf = METHOD_BUFS[method] ?? enc.encode(method);
  // ISS-032: bigint handles are slotmap u64 keys (32-bit slot + 32-bit version);
  // practical values are well within Number.MAX_SAFE_INTEGER. Throw early if a
  // handle ever exceeds the safe range rather than silently corrupting identity.
  const payloadBuf = enc.encode(
    JSON.stringify(payload, (_k, v) => {
      if (typeof v === "bigint") {
        if (v > BigInt(Number.MAX_SAFE_INTEGER)) {
          throw new RangeError(
            `[iroh-http] handle value ${v} exceeds safe integer range and cannot be safely serialised`,
          );
        }
        return Number(v);
      }
      return v;
    }),
  );
  // Deno's FFI types require Uint8Array<ArrayBuffer>; TextEncoder returns
  // Uint8Array<ArrayBufferLike>. The backing store is always a plain ArrayBuffer
  // in practice — cast to satisfy the stricter type.
  const mb = methodBuf as Uint8Array<ArrayBuffer>;
  const pb = payloadBuf as Uint8Array<ArrayBuffer>;

  // Per-call buffer: concurrent nonblocking FFI calls must not share memory.
  // Use the global hint as the initial capacity so most calls allocate once.
  let buf = new Uint8Array(outBufHint) as Uint8Array<ArrayBuffer>;

  let n = (await lib.symbols.iroh_http_call(
    mb,
    BigInt(mb.byteLength),
    pb,
    BigInt(pb.byteLength),
    buf,
    BigInt(buf.byteLength),
  )) as number;

  if (n < 0) {
    // Output buffer too small.  The Rust side cached the response and wrote
    // an 8-byte retrieval token into the first bytes of `buf`.  Read it and
    // retry with method "__cached" to avoid re-dispatching the original
    // handler (DENO-007).
    const tokenBuf = new Uint8Array(buf.buffer, buf.byteOffset, 8) as Uint8Array<ArrayBuffer>;
    const cachedMethod = enc.encode("__cached") as Uint8Array<ArrayBuffer>;
    buf = new Uint8Array(-n) as Uint8Array<ArrayBuffer>;
    n = (await lib.symbols.iroh_http_call(
      cachedMethod,
      BigInt(cachedMethod.byteLength),
      tokenBuf,
      BigInt(tokenBuf.byteLength),
      buf,
      BigInt(buf.byteLength),
    )) as number;
    // Raise the hint so future calls start with a large enough buffer.
    if (buf.byteLength > outBufHint) outBufHint = buf.byteLength;
  }

  const result = JSON.parse(dec.decode(buf.subarray(0, n))) as
    | { ok: T }
    | { err: string };

  if ("err" in result) {
    throw classifyError(result.err);
  }
  return result.ok;
}

/** Module-global hint: set to the size of the last successfully-read chunk
 * so subsequent calls begin with a right-sized allocation. */
let chunkBufHint = 65536;

// ── Bridge implementation ─────────────────────────────────────────────────────

export const bridge: Bridge = {
  async nextChunk(handle: bigint): Promise<Uint8Array | null> {
    // DENO-001: allocate a per-call buffer so concurrent reads on different
    // handles do not share memory and corrupt each other's data.
    let buf = new Uint8Array(chunkBufHint) as Uint8Array<ArrayBuffer>;
    let n = (await lib.symbols.iroh_http_next_chunk(
      handle,
      buf,
      BigInt(buf.byteLength),
    )) as number;
    if (n < 0) {
      // Chunk too large for current hint; grow and retry once.
      buf = new Uint8Array(-n) as Uint8Array<ArrayBuffer>;
      n = (await lib.symbols.iroh_http_next_chunk(
        handle,
        buf,
        BigInt(buf.byteLength),
      )) as number;
    }
    if (n <= 0) return null;
    // Update hint so future calls start with a better-sized buffer.
    chunkBufHint = Math.max(chunkBufHint, n);
    return buf.slice(0, n);
  },
  async sendChunk(handle: bigint, chunk: Uint8Array): Promise<void> {
    await call<Record<never, never>>("sendChunk", {
      handle,
      chunk: encodeBase64(chunk),
    });
  },
  async finishBody(handle: bigint): Promise<void> {
    await call<Record<never, never>>("finishBody", { handle });
  },
  async cancelRequest(handle: bigint): Promise<void> {
    await call<Record<never, never>>("cancelRequest", { handle });
  },
  async allocFetchToken(endpointHandle: number): Promise<bigint> {
    const res = await call<{ token: number }>("allocFetchToken", { endpointHandle });
    return BigInt(res.token);
  },
  cancelFetch(token: bigint): void {
    // Fire-and-forget — do not await.
    void call<Record<never, never>>("cancelInFlight", { token });
  },
  async nextTrailer(handle: bigint): Promise<[string, string][] | null> {
    const res = await call<{ trailers: [string, string][] | null }>(
      "nextTrailer",
      { handle },
    );
    return res.trailers;
  },
  async sendTrailers(
    handle: bigint,
    trailers: [string, string][],
  ): Promise<void> {
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
  reqBodyHandle: bigint | null,
  fetchToken: bigint,
  directAddrs: string[] | null,
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
    directAddrs: directAddrs ?? null,
  });
  return {
    status: res.status,
    headers: res.headers,
    bodyHandle: BigInt(res.bodyHandle),
    url: res.url,
    trailersHandle: BigInt(res.trailersHandle),
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
    readHandle: BigInt(res.readHandle),
    writeHandle: BigInt(res.writeHandle),
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
): Promise<void> => {
  return call<Record<never, never>>("serveStart", { endpointHandle })
    .then(() => {
      return (async () => {
        while (true) {
          const raw = await call<{
            reqHandle: number;
            reqBodyHandle: number;
            resBodyHandle: number;
            reqTrailersHandle: number;
            resTrailersHandle: number;
            method: string;
            url: string;
            headers: [string, string][];
            remoteNodeId: string;
            isBidi: boolean;
          } | null>("nextRequest", { endpointHandle });
          if (raw === null) break;
          const payload: RequestPayload = {
            reqHandle: BigInt(raw.reqHandle),
            reqBodyHandle: BigInt(raw.reqBodyHandle),
            resBodyHandle: BigInt(raw.resBodyHandle),
            reqTrailersHandle: BigInt(raw.reqTrailersHandle),
            resTrailersHandle: BigInt(raw.resTrailersHandle),
            method: raw.method,
            url: raw.url,
            headers: raw.headers,
            remoteNodeId: raw.remoteNodeId,
            isBidi: raw.isBidi,
          };

          // Handle in the background — do not await.
          (async () => {
            try {
              const head = await callback(payload);
              await call<Record<never, never>>("respond", {
                reqHandle: payload.reqHandle,
                status: head.status,
                headers: head.headers,
              });
            } catch (err) {
              console.error("[iroh-http-deno] handler error:", err);
              try {
                await call<Record<never, never>>("respond", {
                  reqHandle: payload.reqHandle,
                  status: 500,
                  headers: [],
                });
              } catch {
                /* respond itself failed — nothing more to do */
              }
            }
          })();
        }
      })();
    });
};

export const allocBodyWriter: AllocBodyWriterFn = () =>
  call<{ handle: number }>("allocBodyWriter", {}).then((r) => BigInt(r.handle));

// ── Endpoint lifecycle ────────────────────────────────────────────────────────

/** Normalise the `discovery` option into flat fields for the Rust adapter. */
function normaliseDiscovery(
  disc?: import("@momics/iroh-http-shared").NodeOptions["discovery"],
): {
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

export async function createEndpointInfo(
  options?: NodeOptions,
): Promise<EndpointInfo> {
  const keyBytes: string | null = options?.key
    ? encodeBase64(
        options.key instanceof Uint8Array ? options.key : options.key.toBytes(),
      )
    : null;

  const { relayMode, relays, disableNetworking } = normaliseRelayMode(
    options?.relayMode,
  );
  const discovery = normaliseDiscovery(options?.discovery);
  const bindAddrs = options?.bindAddr
    ? Array.isArray(options.bindAddr)
      ? options.bindAddr
      : [options.bindAddr]
    : null;

  const res = await call<{
    endpointHandle: number;
    nodeId: string;
    keypair: number[];
  }>("createEndpoint", {
    key: keyBytes,
    idleTimeout: options?.idleTimeout ?? null,
    relayMode: relayMode ?? null,
    relays: relays ?? null,
    bindAddrs,
    dnsDiscovery: discovery.dnsServerUrl ?? null,
    dnsDiscoveryEnabled: discovery.dnsEnabled,
    channelCapacity: options?.advanced?.channelCapacity ?? null,
    maxChunkSizeBytes: options?.advanced?.maxChunkSizeBytes ?? null,
    maxConsecutiveErrors: options?.advanced?.maxConsecutiveErrors ?? null,
    drainTimeout: options?.advanced?.drainTimeout ?? null,
    handleTtl: options?.advanced?.handleTtl ?? null,
    maxPooledConnections: options?.maxPooledConnections ?? null,
    poolIdleTimeoutMs: options?.poolIdleTimeoutMs ?? null,
    disableNetworking,
    proxyUrl: options?.proxyUrl ?? null,
    proxyFromEnv: options?.proxyFromEnv ?? null,
    keylog: options?.keylog ?? null,
    compressionLevel:
      typeof options?.compression === "object"
        ? (options.compression.level ?? null)
        : options?.compression
          ? 3
          : null,
    compressionMinBodyBytes:
      typeof options?.compression === "object"
        ? (options.compression.minBodyBytes ?? null)
        : null,
    maxConcurrency: options?.maxConcurrency ?? null,
    maxConnectionsPerPeer: options?.maxConnectionsPerPeer ?? null,
    requestTimeout: options?.requestTimeout ?? null,
    maxRequestBodyBytes: options?.maxRequestBodyBytes ?? null,
    maxHeaderBytes: options?.maxHeaderBytes ?? null,
  }).catch((e: unknown) => {
    throw classifyBindError(e);
  });
  return {
    endpointHandle: res.endpointHandle,
    nodeId: res.nodeId,
    keypair: new Uint8Array(res.keypair),
  };
}

export async function closeEndpoint(
  handle: number,
  force?: boolean,
): Promise<void> {
  await call<Record<never, never>>("closeEndpoint", {
    endpointHandle: handle,
    force: force ?? null,
  });
}

export function stopServe(handle: number): void {
  call<Record<never, never>>("stopServe", { endpointHandle: handle }).catch(
    () => {},
  );
}

// ── Address introspection ──────────────────────────────────────────────────────

export const denoAddrFns: AddrFunctions = {
  nodeAddr: async (handle) => {
    const res = await call<NodeAddrInfo>("nodeAddr", {
      endpointHandle: handle,
    });
    return res;
  },
  nodeTicket: async (handle) => {
    return call<string>("nodeTicket", { endpointHandle: handle });
  },
  homeRelay: async (handle) => {
    const res = await call<string | null>("homeRelay", {
      endpointHandle: handle,
    });
    return res;
  },
  peerInfo: async (handle, nodeId) => {
    const res = await call<NodeAddrInfo | null>("peerInfo", {
      endpointHandle: handle,
      nodeId,
    });
    return res;
  },
  peerStats: async (handle, nodeId) => {
    return call<PeerStats | null>("peerStats", {
      endpointHandle: handle,
      nodeId,
    });
  },
};

/** Discovery functions backed by Deno FFI calls. */
export const denoDiscoveryFns: DiscoveryFunctions = {
  mdnsBrowse: async (handle, serviceName) => {
    return call<number>("mdnsBrowse", { endpointHandle: handle, serviceName });
  },
  mdnsNextEvent: async (browseHandle) => {
    return call<PeerDiscoveryEvent | null>("mdnsNextEvent", { browseHandle });
  },
  mdnsBrowseClose: (browseHandle) => {
    call<Record<never, never>>("mdnsBrowseClose", { browseHandle }).catch(
      () => {},
    );
  },
  mdnsAdvertise: async (handle, serviceName) => {
    return call<number>("mdnsAdvertise", {
      endpointHandle: handle,
      serviceName,
    });
  },
  mdnsAdvertiseClose: (advertiseHandle) => {
    call<Record<never, never>>("mdnsAdvertiseClose", { advertiseHandle }).catch(
      () => {},
    );
  },
};

// ── Session functions ─────────────────────────────────────────────────────────

export const denoSessionFns: RawSessionFns = {
  connect: async (endpointHandle, nodeId, directAddrs) => {
    const res = await call<{ sessionHandle: number }>("sessionConnect", {
      endpointHandle,
      nodeId,
      directAddrs: directAddrs ?? null,
    });
    return BigInt(res.sessionHandle as unknown as number);
  },
  createBidiStream: async (sessionHandle) => {
    const res = await call<{ readHandle: number; writeHandle: number }>(
      "sessionCreateBidiStream",
      { sessionHandle },
    );
    return {
      readHandle: BigInt(res.readHandle),
      writeHandle: BigInt(res.writeHandle),
    } satisfies FfiDuplexStream;
  },
  nextBidiStream: async (sessionHandle) => {
    const res = await call<{ readHandle: number; writeHandle: number } | null>(
      "sessionNextBidiStream",
      { sessionHandle },
    );
    return res
      ? ({
          readHandle: BigInt(res.readHandle),
          writeHandle: BigInt(res.writeHandle),
        } satisfies FfiDuplexStream)
      : null;
  },
  createUniStream: async (sessionHandle) => {
    const res = await call<{ writeHandle: number }>("sessionCreateUniStream", {
      sessionHandle,
    });
    return BigInt(res.writeHandle);
  },
  nextUniStream: async (sessionHandle) => {
    const res = await call<{ readHandle: number } | null>(
      "sessionNextUniStream",
      { sessionHandle },
    );
    return res ? BigInt(res.readHandle) : null;
  },
  sendDatagram: async (sessionHandle, data) => {
    await call<Record<never, never>>("sessionSendDatagram", {
      sessionHandle,
      data: encodeBase64(data),
    });
  },
  recvDatagram: async (sessionHandle) => {
    const res = await call<{ data: string } | null>("sessionRecvDatagram", {
      sessionHandle,
    });
    return res ? decodeBase64(res.data) : null;
  },
  maxDatagramSize: async (sessionHandle) => {
    const res = await call<{ maxDatagramSize: number | null }>(
      "sessionMaxDatagramSize",
      { sessionHandle },
    );
    return res.maxDatagramSize;
  },
  closed: async (sessionHandle) => {
    return call<{ closeCode: number; reason: string }>("sessionClosed", {
      sessionHandle,
    });
  },
  close: async (sessionHandle, closeCode?, reason?) => {
    await call<Record<never, never>>("sessionClose", {
      sessionHandle,
      closeCode,
      reason,
    });
  },
};

// ── Cryptography ───────────────────────────────────────────────────────────────

/**
 * Sign `data` with a 32-byte Ed25519 secret key.
 * Returns a 64-byte signature.
 */
export async function secretKeySign(
  secretKey: Uint8Array,
  data: Uint8Array,
): Promise<Uint8Array> {
  const b64 = await call<string>("secretKeySign", {
    secretKey: encodeBase64(secretKey),
    data: encodeBase64(data),
  });
  return decodeBase64(b64);
}

/**
 * Verify an Ed25519 signature.
 * @param publicKey  32-byte Ed25519 public key.
 * @param data       Original message bytes.
 * @param signature  64-byte signature to verify.
 * @returns `true` if the signature is valid.
 */
export async function publicKeyVerify(
  publicKey: Uint8Array,
  data: Uint8Array,
  signature: Uint8Array,
): Promise<boolean> {
  return call<boolean>("publicKeyVerify", {
    publicKey: encodeBase64(publicKey),
    data: encodeBase64(data),
    signature: encodeBase64(signature),
  });
}

/**
 * Generate a fresh random 32-byte Ed25519 secret key.
 */
export async function generateSecretKey(): Promise<Uint8Array> {
  const b64 = await call<string>("generateSecretKey", {});
  return decodeBase64(b64);
}
