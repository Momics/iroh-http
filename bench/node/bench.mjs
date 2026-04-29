/**
 * iroh-http Node.js benchmark suite (mitata).
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
 * Run: node bench/node/bench.mjs
 */

import { bench, group, run } from "mitata";
import { createServer } from "node:http";
import { once } from "node:events";
import { createNode } from "../../packages/iroh-http-node/lib.js";

// ── Payloads ──────────────────────────────────────────────────────────────────

const SMALL = Buffer.from('{"ok":true}');

function payload(size) {
  return Buffer.alloc(size, 0x61);
}

const PAYLOAD_1K = payload(1_024);
const PAYLOAD_64K = payload(64 * 1_024);
const PAYLOAD_1M = payload(1_024 * 1_024);
const PAYLOAD_10M = payload(10 * 1_024 * 1_024);

function getPayload(size) {
  switch (size) {
    case 1024: return PAYLOAD_1K;
    case 65536: return PAYLOAD_64K;
    case 1048576: return PAYLOAD_1M;
    case 10485760: return PAYLOAD_10M;
    default: return SMALL;
  }
}

// ── Native TCP server ─────────────────────────────────────────────────────────

async function makeTcpServer() {
  const server = createServer((req, res) => {
    const size = Number(
      new URL(req.url ?? "/", "http://localhost").searchParams.get("size") ?? 0,
    );
    const body = getPayload(size);
    res.writeHead(200, {
      "content-type": "application/octet-stream",
      "content-length": body.length,
    });
    res.end(body);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const addr = server.address();
  const port = typeof addr === "object" && addr ? addr.port : 0;
  return {
    baseUrl: `http://127.0.0.1:${port}`,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

// ── Setup ─────────────────────────────────────────────────────────────────────

const tcp = await makeTcpServer();
const server = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const client = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const { id: serverId, addrs: serverAddrs } = await server.addr();
const serveAbort = new AbortController();
const serveHandle = server.serve({ signal: serveAbort.signal }, (req) => {
  const size = Number(new URL(req.url).searchParams.get("size") ?? 0);
  return new Response(getPayload(size));
});

try {
  // Warm the connection
  await client.fetch(`httpi://${serverId}/warmup`, { directAddrs: serverAddrs });

  // ── 1. Cold connect ─────────────────────────────────────────────────────────
  // NOTE: close() is deferred — QUIC connection drain (~80ms) is not part of
  // connect latency.  Nodes are collected and cleaned up after `run()`.
  const coldClients = [];

  group("cold-connect", () => {
    bench("cold-connect/iroh", async () => {
      const freshClient = await createNode({
        disableNetworking: true,
        bindAddr: "127.0.0.1:0",
      });
      const res = await freshClient.fetch(`httpi://${serverId}/cold`, {
        directAddrs: serverAddrs,
      });
      await res.arrayBuffer();
      coldClients.push(freshClient);
    });

    bench("cold-connect/native", async () => {
      const res = await fetch(`${tcp.baseUrl}/cold`);
      await res.arrayBuffer();
    });
  });

  // ── 2. Warm request ─────────────────────────────────────────────────────────

  group("warm-request", () => {
    bench("warm-request/iroh", async () => {
      const res = await client.fetch(`httpi://${serverId}/ping`, {
        directAddrs: serverAddrs,
      });
      await res.arrayBuffer();
    });

    bench("warm-request/native", async () => {
      const res = await fetch(`${tcp.baseUrl}/ping`);
      await res.arrayBuffer();
    });
  });

  // ── 3–5. Throughput (1KB, 64KB, 1MB) ────────────────────────────────────────

  for (const [label, size] of [
    ["1kb", 1_024],
    ["64kb", 64 * 1_024],
    ["1mb", 1_024 * 1_024],
  ]) {
    group(`throughput-${label}`, () => {
      bench(`throughput-${label}/iroh`, async () => {
        const res = await client.fetch(
          `httpi://${serverId}/data?size=${size}`,
          { directAddrs: serverAddrs },
        );
        await res.arrayBuffer();
      });

      bench(`throughput-${label}/native`, async () => {
        const res = await fetch(`${tcp.baseUrl}/data?size=${size}`);
        await res.arrayBuffer();
      });
    });
  }

  // ── 6. Throughput 10MB ──────────────────────────────────────────────────────

  group("throughput-10mb", () => {
    bench("throughput-10mb/iroh", async () => {
      const res = await client.fetch(`httpi://${serverId}/data?size=10485760`,
        { directAddrs: serverAddrs },
      );
      await res.arrayBuffer();
    });

    bench("throughput-10mb/native", async () => {
      const res = await fetch(`${tcp.baseUrl}/data?size=10485760`);
      await res.arrayBuffer();
    });
  });

  // ── 7–8. Multiplexing ──────────────────────────────────────────────────────

  for (const n of [8, 32]) {
    group(`multiplex-x${n}`, () => {
      bench(`multiplex-x${n}/iroh`, async () => {
        await Promise.all(
          Array.from({ length: n }, () =>
            client
              .fetch(`httpi://${serverId}/ping`, { directAddrs: serverAddrs })
              .then((res) => res.arrayBuffer()),
          ),
        );
      });

      bench(`multiplex-x${n}/native`, async () => {
        await Promise.all(
          Array.from({ length: n }, () =>
            fetch(`${tcp.baseUrl}/ping`).then((res) => res.arrayBuffer()),
          ),
        );
      });
    });
  }

  // ── 9. Serve req/s ─────────────────────────────────────────────────────────

  group("serve-rps", () => {
    bench("serve-rps/iroh", async () => {
      const res = await client.fetch(`httpi://${serverId}/ping`, {
        directAddrs: serverAddrs,
      });
      await res.arrayBuffer();
    });

    bench("serve-rps/native", async () => {
      const res = await fetch(`${tcp.baseUrl}/ping`);
      await res.arrayBuffer();
    });
  });

  await run();
  // Clean up cold-connect clients deferred from the benchmark loop.
  await Promise.all(coldClients.map((c) => c.close({ force: true })));
} finally {
  serveAbort.abort();
  await serveHandle.finished.catch(() => {});
  await server.close();
  await client.close();
  await tcp.close();
}
