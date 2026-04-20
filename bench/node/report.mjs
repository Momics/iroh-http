import { createServer } from "node:http";
import { once } from "node:events";
import { createNode } from "../../packages/iroh-http-node/lib.js";

const mode = process.argv[2] ?? "all";
const SIZES = [1, 1024, 64 * 1024, 1024 * 1024];
const ITERATIONS = 25;
const CONCURRENCY = 32;

const toUs = (ms) => ms * 1000;
const toMbPerSec = (bytes, ms) => (bytes / (1024 * 1024)) / (ms / 1000);

function payload(size) {
  return Buffer.alloc(size, 0x61);
}

function percentile(sorted, p) {
  if (sorted.length === 0) return 0;
  const idx = Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length));
  return sorted[idx];
}

async function makeTcpServer() {
  const server = createServer((req, res) => {
    const size = Number(new URL(req.url ?? "/", "http://localhost").searchParams.get("size") ?? 1024);
    const body = payload(size);
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

async function timeRequest(run) {
  const t0 = performance.now();
  await run();
  return performance.now() - t0;
}

const tcp = await makeTcpServer();
const server = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const client = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const { id: serverId, addrs: serverAddrs } = await server.addr();
const serveAbort = new AbortController();
const serveHandle = server.serve({ signal: serveAbort.signal }, (req) => {
  const size = Number(new URL(req.url).searchParams.get("size") ?? 1024);
  return new Response(payload(size));
});

try {
  await client.fetch(serverId, "httpi://bench.local/data?size=1024", { directAddrs: serverAddrs });

  const throughput = [];
  const latency = [];

  for (const size of SIZES) {
    const tcpDurations = [];
    const irohDurations = [];

    for (let i = 0; i < ITERATIONS; i++) {
      tcpDurations.push(await timeRequest(async () => {
        const res = await fetch(`${tcp.baseUrl}/data?size=${size}`);
        await res.arrayBuffer();
      }));
      irohDurations.push(await timeRequest(async () => {
        const res = await client.fetch(serverId, `httpi://bench.local/data?size=${size}`, {
          directAddrs: serverAddrs,
        });
        await res.arrayBuffer();
      }));
    }

    const tcpAvg = tcpDurations.reduce((a, b) => a + b, 0) / tcpDurations.length;
    const irohAvg = irohDurations.reduce((a, b) => a + b, 0) / irohDurations.length;
    throughput.push(
      { name: `node/tcp/${size}B`, unit: "MB/s", value: toMbPerSec(size, tcpAvg) },
      { name: `node/iroh/${size}B`, unit: "MB/s", value: toMbPerSec(size, irohAvg) },
    );

    const sorted = [...irohDurations].sort((a, b) => a - b);
    latency.push(
      { name: `node/iroh/${size}B/p50`, unit: "us", value: toUs(percentile(sorted, 50)) },
      { name: `node/iroh/${size}B/p95`, unit: "us", value: toUs(percentile(sorted, 95)) },
      { name: `node/iroh/${size}B/p99`, unit: "us", value: toUs(percentile(sorted, 99)) },
      { name: `node/iroh/${size}B/p999`, unit: "us", value: toUs(percentile(sorted, 99.9)) },
    );
  }

  const tcpConnect = await timeRequest(async () => {
    const res = await fetch(`${tcp.baseUrl}/data?size=1`);
    await res.arrayBuffer();
  });

  const irohCold = await timeRequest(async () => {
    const freshClient = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
    try {
      const res = await freshClient.fetch(serverId, "httpi://bench.local/data?size=1", {
        directAddrs: serverAddrs,
      });
      await res.arrayBuffer();
    } finally {
      await freshClient.close();
    }
  });

  latency.push(
    { name: "node/connection/tcp", unit: "us", value: toUs(tcpConnect) },
    { name: "node/connection/iroh", unit: "us", value: toUs(irohCold) },
  );

  const mux = await timeRequest(async () => {
    await Promise.all(
      Array.from({ length: CONCURRENCY }, () =>
        client
          .fetch(serverId, "httpi://bench.local/data?size=1024", { directAddrs: serverAddrs })
          .then((res) => res.arrayBuffer()),
      ),
    );
  });

  latency.push({
    name: `node/multiplex_avg_per_stream/${CONCURRENCY}`,
    unit: "us",
    value: toUs(mux / CONCURRENCY),
  });

  if (mode === "throughput") {
    console.log(JSON.stringify(throughput, null, 2));
  } else if (mode === "latency") {
    console.log(JSON.stringify(latency, null, 2));
  } else {
    console.log(JSON.stringify({ throughput, latency }, null, 2));
  }
} finally {
  serveAbort.abort();
  await serveHandle.finished.catch(() => {});
  await server.close();
  await client.close();
  await tcp.close();
}
