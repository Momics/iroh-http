/**
 * iroh-http Deno benchmark suite.
 *
 * Scenarios:
 *   1. Cold connect          — first fetch to a new peer (no pooled connection)
 *   2. Warm request          — fetch on a warm connection, ~100B body
 *   3. Throughput 1KB        — fetch + drain 1 KB body
 *   4. Throughput 64KB       — fetch + drain 64 KB body
 *   5. Throughput 1MB        — fetch + drain 1 MB body
 *   6. Throughput 10MB       — fetch + drain 10 MB body (reduced iterations)
 *   7. Multiplex ×8          — 8 concurrent fetches, ~100B body
 *   8. Multiplex ×32         — 32 concurrent fetches, ~100B body
 *   9. Serve req/s           — client saturates a serve handler, ~100B body
 *
 * Each scenario runs against both iroh and native Deno.serve/fetch for
 * overhead comparison.
 *
 * Run: deno bench --allow-net --allow-ffi --allow-env --allow-read --allow-write --allow-sys bench/deno/bench.ts
 */

import { createNode } from "../../packages/iroh-http-deno/mod.ts";

// ── Payloads ──────────────────────────────────────────────────────────────────

const SMALL = new TextEncoder().encode('{"ok":true}');

function payload(size: number): Uint8Array {
  return new Uint8Array(size).fill(0x61);
}

const PAYLOAD_1K = payload(1_024);
const PAYLOAD_64K = payload(64 * 1_024);
const PAYLOAD_1M = payload(1_024 * 1_024);
const PAYLOAD_10M = payload(10 * 1_024 * 1_024);

// ── Native TCP server ─────────────────────────────────────────────────────────

const tcpServer = Deno.serve({ hostname: "127.0.0.1", port: 0 }, (req) => {
  const url = new URL(req.url);
  const size = url.searchParams.get("size");
  let body: Uint8Array;
  switch (size) {
    case "1024":
      body = PAYLOAD_1K;
      break;
    case "65536":
      body = PAYLOAD_64K;
      break;
    case "1048576":
      body = PAYLOAD_1M;
      break;
    case "10485760":
      body = PAYLOAD_10M;
      break;
    default:
      body = SMALL;
  }
  return new Response(body as Uint8Array<ArrayBuffer>, {
    headers: { "content-type": "application/octet-stream" },
  });
});
const tcpAddr = tcpServer.addr as Deno.NetAddr;
const tcpBase = `http://${tcpAddr.hostname}:${tcpAddr.port}`;

// ── Iroh nodes ────────────────────────────────────────────────────────────────

const server = await createNode({
  disableNetworking: true,
  bindAddr: "127.0.0.1:0",
});
const client = await createNode({
  disableNetworking: true,
  bindAddr: "127.0.0.1:0",
});
const { id: serverId, addrs: serverAddrs } = await server.addr();
const serveAbort = new AbortController();
const serveHandle = server.serve({ signal: serveAbort.signal }, (req) => {
  const url = new URL(req.url);
  const size = url.searchParams.get("size");
  let body: Uint8Array;
  switch (size) {
    case "1024":
      body = PAYLOAD_1K;
      break;
    case "65536":
      body = PAYLOAD_64K;
      break;
    case "1048576":
      body = PAYLOAD_1M;
      break;
    case "10485760":
      body = PAYLOAD_10M;
      break;
    default:
      body = SMALL;
  }
  return new Response(body as Uint8Array<ArrayBuffer>);
});

// Warm the connection so scenarios 2–9 don't pay cold-connect cost.
await client.fetch(`httpi://${serverId}/warmup`, {
  directAddrs: serverAddrs,
});

// ── 1. Cold connect ───────────────────────────────────────────────────────────
// NOTE: close() is deferred — QUIC connection drain (~80ms) is not part of
// connect latency.  Nodes are collected and cleaned up at process exit.
const coldClients: Array<typeof server> = [];

Deno.bench("cold-connect/iroh", { group: "cold-connect", n: 5 }, async () => {
  const freshClient = await createNode({
    disableNetworking: true,
    bindAddr: "127.0.0.1:0",
  });
  const res = await freshClient.fetch(`httpi://${serverId}/cold`,
    { directAddrs: serverAddrs },
  );
  await res.arrayBuffer();
  coldClients.push(freshClient);
});

Deno.bench(
  "cold-connect/native",
  { group: "cold-connect", baseline: true, n: 5 },
  async () => {
    const res = await fetch(`${tcpBase}/cold`);
    await res.arrayBuffer();
  },
);

// ── 2. Warm request (small body) ──────────────────────────────────────────────

Deno.bench("warm-request/iroh", { group: "warm-request" }, async () => {
  const res = await client.fetch(`httpi://${serverId}/ping`, {
    directAddrs: serverAddrs,
  });
  await res.arrayBuffer();
});

Deno.bench(
  "warm-request/native",
  { group: "warm-request", baseline: true },
  async () => {
    const res = await fetch(`${tcpBase}/ping`);
    await res.arrayBuffer();
  },
);

// ── 3–5. Throughput (1KB, 64KB, 1MB) ──────────────────────────────────────────

for (const [label, size] of [
  ["1kb", 1_024],
  ["64kb", 64 * 1_024],
  ["1mb", 1_024 * 1_024],
] as const) {
  Deno.bench(
    `throughput-${label}/iroh`,
    { group: `throughput-${label}` },
    async () => {
      const res = await client.fetch(
        serverId,
        `httpi://bench.local/data?size=${size}`,
        { directAddrs: serverAddrs },
      );
      await res.arrayBuffer();
    },
  );

  Deno.bench(
    `throughput-${label}/native`,
    { group: `throughput-${label}`, baseline: true },
    async () => {
      const res = await fetch(`${tcpBase}/data?size=${size}`);
      await res.arrayBuffer();
    },
  );
}

// ── 6. Throughput 10MB (reduced iterations) ───────────────────────────────────

Deno.bench(
  "throughput-10mb/iroh",
  { group: "throughput-10mb", n: 10 },
  async () => {
    const res = await client.fetch(`httpi://${serverId}/data?size=10485760`,
      { directAddrs: serverAddrs },
    );
    await res.arrayBuffer();
  },
);

Deno.bench(
  "throughput-10mb/native",
  { group: "throughput-10mb", baseline: true, n: 10 },
  async () => {
    const res = await fetch(`${tcpBase}/data?size=10485760`);
    await res.arrayBuffer();
  },
);

// ── 7–8. Multiplexing ─────────────────────────────────────────────────────────

for (const n of [8, 32]) {
  Deno.bench(
    `multiplex-x${n}/iroh`,
    { group: `multiplex-x${n}` },
    async () => {
      await Promise.all(
        Array.from({ length: n }, () =>
          client
            .fetch(`httpi://${serverId}/ping`, {
              directAddrs: serverAddrs,
            })
            .then((res) => res.arrayBuffer()),
        ),
      );
    },
  );

  Deno.bench(
    `multiplex-x${n}/native`,
    { group: `multiplex-x${n}`, baseline: true },
    async () => {
      await Promise.all(
        Array.from({ length: n }, () =>
          fetch(`${tcpBase}/ping`).then((res) => res.arrayBuffer()),
        ),
      );
    },
  );
}

// ── 9. Serve req/s ────────────────────────────────────────────────────────────

Deno.bench("serve-rps/iroh", { group: "serve-rps" }, async () => {
  const res = await client.fetch(`httpi://${serverId}/ping`, {
    directAddrs: serverAddrs,
  });
  await res.arrayBuffer();
});

Deno.bench(
  "serve-rps/native",
  { group: "serve-rps", baseline: true },
  async () => {
    const res = await fetch(`${tcpBase}/ping`);
    await res.arrayBuffer();
  },
);

// ── Teardown ──────────────────────────────────────────────────────────────────

globalThis.addEventListener("unload", () => {
  serveAbort.abort();
  void serveHandle.finished.catch(() => {});
  for (const c of coldClients) void c.close({ force: true }).catch(() => {});
  void server.close().catch(() => {});
  void client.close().catch(() => {});
  void tcpServer.shutdown().catch(() => {});
});
