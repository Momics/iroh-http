/**
 * iroh-http-deno — DenoAdapter.
 *
 * Implements the Bridge interface using Deno.dlopen FFI and exposes the
 * raw platform functions needed by iroh-http-shared's buildNode.
 */

import { resolve, dirname, fromFileUrl } from "@std/path";
import type {
  EndpointInfo,
  EndpointStats,
  NodeOptions,
  NodeAddrInfo,
  PeerConnectionEvent,
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

/** Version must match the tag used for GitHub releases (v0.1.0 → tag v0.1.0). */
const VERSION = "0.1.6";
const DOWNLOAD_BASE = `https://github.com/Momics/iroh-http/releases/download/v${VERSION}`;

function cacheDir(): string {
  // Local dev: import.meta.url is file://, use lib/ next to src/.
  if (import.meta.url.startsWith("file://")) {
    return resolve(dirname(fromFileUrl(import.meta.url)), "..", "lib");
  }
  // JSR / remote: use a platform-appropriate cache directory.
  const home = Deno.env.get("HOME") ?? Deno.env.get("USERPROFILE") ?? "/tmp";
  return resolve(home, ".cache", "iroh-http-deno", VERSION);
}

const LIB_DIR = cacheDir();

async function ensureLib(): Promise<string> {
  const name = libName();
  const libPath = resolve(LIB_DIR, name);

  // Fast path: lib already exists locally (dev build or cached download).
  try {
    await Deno.stat(libPath);
    return libPath;
  } catch {
    // Not found — download it.
  }

  const url = `${DOWNLOAD_BASE}/${name}`;
  console.error(`[iroh-http] Downloading native library from ${url} …`);

  const resp = await fetch(url);
  if (!resp.ok || !resp.body) {
    throw new Error(
      `[iroh-http] Failed to download native library: ${resp.status} ${resp.statusText}\n` +
      `  URL: ${url}\n` +
      `  Ensure a GitHub release exists for v${VERSION} with the binary attached.`,
    );
  }

  await Deno.mkdir(LIB_DIR, { recursive: true });
  const file = await Deno.open(libPath, { write: true, create: true });
  try {
    await resp.body.pipeTo(file.writable);
  } catch (e) {
    // Clean up partial download.
    try { await Deno.remove(libPath); } catch { /* ignore */ }
    throw e;
  }

  // Mark executable on Unix.
  if (Deno.build.os !== "windows") {
    await Deno.chmod(libPath, 0o755);
  }

  return libPath;
}

const LIB_PATH = await ensureLib();

// ── FFI symbols ───────────────────────────────────────────────────────────────

const lib = Deno.dlopen(LIB_PATH, {
  iroh_http_call: {
    parameters: ["buffer", "usize", "buffer", "usize", "buffer", "usize"],
    result: "i32",
    nonblocking: true,
  },
  iroh_http_next_chunk: {
    parameters: ["u32", "u64", "buffer", "usize"],
    result: "i32",
    nonblocking: true,
  },
  iroh_http_send_chunk: {
    parameters: ["u32", "u64", "buffer", "usize"],
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
 * same memory.  Grows when any call needs more space; capped at 4 MB to
 * prevent unbounded memory growth from a single large response.
 */
const MAX_OUT_BUF = 4 * 1024 * 1024; // 4 MB
let outBufHint = 128 * 1024;

/** Pre-encoded method name buffers (UTF-8). */
const METHOD_BUFS: Record<string, Uint8Array> = Object.fromEntries(
  [
    "finishBody",
    "cancelRequest",
    "rawFetch",
    "rawConnect",
    "serveStart",
    "nextRequest",
    "nextConnectionEvent",
    "respond",
    "allocBodyWriter",
    "createEndpoint",
    "closeEndpoint",
    "allocFetchToken",
    "cancelInFlight",
    "waitEndpointClosed",
    "endpointStats",
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
    // Raise the hint so future calls start with a large enough buffer (capped).
    if (buf.byteLength > outBufHint) outBufHint = Math.min(buf.byteLength, MAX_OUT_BUF);
  }

  const result = JSON.parse(dec.decode(buf.subarray(0, n))) as
    | { ok: T }
    | { err: string };

  if ("err" in result) {
    throw classifyError(result.err);
  }
  return result.ok;
}

/** Module-global hint for `nextChunk` receive buffers.  Grows up to
 * `MAX_CHUNK_BUF` (4 MB) to match the largest chunk seen; never shrinks. */
const MAX_CHUNK_BUF = 4 * 1024 * 1024; // 4 MB
let chunkBufHint = 65536;

// ── Bridge implementation ─────────────────────────────────────────────────────

export function makeBridge(endpointHandle: number): Bridge {
  return {
  async nextChunk(handle: bigint): Promise<Uint8Array | null> {
    // DENO-001: allocate a per-call buffer so concurrent reads on different
    // handles do not share memory and corrupt each other's data.
    let buf = new Uint8Array(chunkBufHint) as Uint8Array<ArrayBuffer>;
    let n = (await lib.symbols.iroh_http_next_chunk(
      endpointHandle,
      handle,
      buf,
      BigInt(buf.byteLength),
    )) as number;
    if (n < -1) {
      // Return value encodes the required size as a negative number.
      // Grow the buffer and retry exactly once.
      buf = new Uint8Array(-n) as Uint8Array<ArrayBuffer>;
      n = (await lib.symbols.iroh_http_next_chunk(
        endpointHandle,
        handle,
        buf,
        BigInt(buf.byteLength),
      )) as number;
    }
    // n === -1  → hard error (endpoint gone, handle invalid, stream reset).
    // n === 0   → clean EOF.
    // n > 0     → chunk of n bytes.
    if (n === -1) {
      throw new Error(`nextChunk: stream error on handle ${handle}`);
    }
    if (n === 0) return null;
    // Update hint so future calls start with a better-sized buffer (capped).
    chunkBufHint = Math.min(Math.max(chunkBufHint, n), MAX_CHUNK_BUF);
    return buf.slice(0, n);
  },
  async sendChunk(handle: bigint, chunk: Uint8Array): Promise<void> {
    // Direct binary FFI — avoids base64 encode / decode round-trip on hot path.
    const buf = chunk as Uint8Array<ArrayBuffer>;
    const result = await lib.symbols.iroh_http_send_chunk(
      endpointHandle,
      handle,
      buf,
      BigInt(buf.byteLength),
    ) as number;
    if (result < 0) {
      throw new Error(`sendChunk failed: handle ${handle}`);
    }
  },
  async finishBody(handle: bigint): Promise<void> {
    await call<Record<never, never>>("finishBody", { endpointHandle, handle });
  },
  async cancelRequest(handle: bigint): Promise<void> {
    await call<Record<never, never>>("cancelRequest", { endpointHandle, handle });
  },
  async allocFetchToken(_endpointHandle: number): Promise<bigint> {
    const res = await call<{ token: number }>("allocFetchToken", { endpointHandle });
    return BigInt(res.token);
  },
  cancelFetch(token: bigint): void {
    // Fire-and-forget — do not await.
    void call<Record<never, never>>("cancelInFlight", { endpointHandle, token });
  },
  };
}

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
 *
 * If `options.onConnectionEvent` is provided, a parallel polling loop reads
 * peer connect/disconnect events via `nextConnectionEvent`.
 */
export const rawServe: RawServeFn = (
  endpointHandle: number,
  options: { onConnectionEvent?: (event: PeerConnectionEvent) => void },
  callback: (payload: RequestPayload) => Promise<FfiResponseHead>,
): Promise<void> => {
  return call<Record<never, never>>("serveStart", { endpointHandle })
    .then(() => {
      // Start connection event polling loop if a callback was supplied.
      if (options.onConnectionEvent) {
        const onEv = options.onConnectionEvent;
        (async () => {
          while (true) {
            const ev = await call<PeerConnectionEvent | null>(
              "nextConnectionEvent",
              { endpointHandle },
            );
            if (ev === null) break;
            try {
              onEv(ev);
            } catch (err) {
              console.error("[iroh-http-deno] onConnectionEvent error:", err);
            }
          }
        })();
      }

      return (async () => {
        while (true) {
          const raw = await call<{
            reqHandle: number;
            reqBodyHandle: number;
            resBodyHandle: number;
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
                endpointHandle,
                reqHandle: payload.reqHandle,
                status: head.status,
                headers: head.headers,
              });
            } catch (err) {
              console.error("[iroh-http-deno] handler error:", err);
              try {
                await call<Record<never, never>>("respond", {
                  endpointHandle,
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

export function makeAllocBodyWriter(endpointHandle: number): AllocBodyWriterFn {
  return () =>
    call<{ handle: number }>("allocBodyWriter", { endpointHandle }).then((r) => BigInt(r.handle));
}

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

/** Resolves when the endpoint has been fully closed (explicit or native). */
export function waitEndpointClosed(handle: number): Promise<void> {
  return call<Record<never, never>>("waitEndpointClosed", {
    endpointHandle: handle,
  }).then(() => {});
}

/** Snapshot of current endpoint statistics (point-in-time). */
export function endpointStats(handle: number): Promise<EndpointStats> {
  return call<EndpointStats>("endpointStats", { endpointHandle: handle });
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
  stats: async (handle) => {
    return call<EndpointStats>("endpointStats", { endpointHandle: handle });
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
//
// Every session dispatch handler in dispatch.rs requires `endpointHandle` to
// look up the IrohEndpoint from the global registry.  The `RawSessionFns`
// interface passes `endpointHandle` as a parameter only for `connect`; all
// other methods only receive the session handle.  We therefore use a factory
// that closes over the endpoint handle so every call can include it.

export function makeDenoSessionFns(endpointHandle: number): RawSessionFns {
  return {
  connect: async (_endpointHandle, nodeId, directAddrs) => {
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
      { endpointHandle, sessionHandle },
    );
    return {
      readHandle: BigInt(res.readHandle),
      writeHandle: BigInt(res.writeHandle),
    } satisfies FfiDuplexStream;
  },
  nextBidiStream: async (sessionHandle) => {
    const res = await call<{ readHandle: number; writeHandle: number } | null>(
      "sessionNextBidiStream",
      { endpointHandle, sessionHandle },
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
      endpointHandle, sessionHandle,
    });
    return BigInt(res.writeHandle);
  },
  nextUniStream: async (sessionHandle) => {
    const res = await call<{ readHandle: number } | null>(
      "sessionNextUniStream",
      { endpointHandle, sessionHandle },
    );
    return res ? BigInt(res.readHandle) : null;
  },
  sendDatagram: async (sessionHandle, data) => {
    await call<Record<never, never>>("sessionSendDatagram", {
      endpointHandle, sessionHandle,
      data: encodeBase64(data),
    });
  },
  recvDatagram: async (sessionHandle) => {
    const res = await call<{ data: string } | null>("sessionRecvDatagram", {
      endpointHandle, sessionHandle,
    });
    return res ? decodeBase64(res.data) : null;
  },
  maxDatagramSize: async (sessionHandle) => {
    const res = await call<{ maxDatagramSize: number | null }>(
      "sessionMaxDatagramSize",
      { endpointHandle, sessionHandle },
    );
    return res.maxDatagramSize;
  },
  closed: async (sessionHandle) => {
    return call<{ closeCode: number; reason: string }>("sessionClosed", {
      endpointHandle, sessionHandle,
    });
  },
  close: async (sessionHandle, closeCode?, reason?) => {
    await call<Record<never, never>>("sessionClose", {
      endpointHandle, sessionHandle,
      closeCode,
      reason,
    });
  },
  };
}

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
