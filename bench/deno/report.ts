import { createNode } from "../../packages/iroh-http-deno/mod.ts";

const mode = Deno.args[0] ?? "all";
const SIZES = [1, 1024, 64 * 1024, 1024 * 1024];
const ITERATIONS = 25;
const CONCURRENCY = 32;

const toUs = (ms: number) => ms * 1000;
const toMbPerSec = (bytes: number, ms: number) => (bytes / (1024 * 1024)) / (ms / 1000);

function payload(size: number): Uint8Array {
  return new Uint8Array(size).fill(0x61);
}

function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0;
  const idx = Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length));
  return sorted[idx];
}

async function timeRequest(fn: () => Promise<void>): Promise<number> {
  const t0 = performance.now();
  await fn();
  return performance.now() - t0;
}

const tcpServer = Deno.serve({ hostname: "127.0.0.1", port: 0 }, (req) => {
  const size = Number(new URL(req.url).searchParams.get("size") ?? 1024);
  return new Response(payload(size));
});
const tcpAddr = tcpServer.addr as Deno.NetAddr;
const tcpBase = `http://${tcpAddr.hostname}:${tcpAddr.port}`;

const server = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const client = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
const { id: serverId, addrs: serverAddrs } = await server.addr();
const abort = new AbortController();
const serveHandle = server.serve({ signal: abort.signal }, (req) => {
  const size = Number(new URL(req.url).searchParams.get("size") ?? 1024);
  return new Response(payload(size));
});

try {
  await client.fetch(serverId, "httpi://bench.local/data?size=1024", { directAddrs: serverAddrs });

  const throughput: Array<{ name: string; unit: string; value: number }> = [];
  const latency: Array<{ name: string; unit: string; value: number }> = [];

  for (const size of SIZES) {
    const tcpDurations: number[] = [];
    const irohDurations: number[] = [];

    for (let i = 0; i < ITERATIONS; i++) {
      tcpDurations.push(await timeRequest(async () => {
        const res = await fetch(`${tcpBase}/data?size=${size}`);
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
      { name: `deno/tcp/${size}B`, unit: "MB/s", value: toMbPerSec(size, tcpAvg) },
      { name: `deno/iroh/${size}B`, unit: "MB/s", value: toMbPerSec(size, irohAvg) },
    );

    const sorted = [...irohDurations].sort((a, b) => a - b);
    latency.push(
      { name: `deno/iroh/${size}B/p50`, unit: "us", value: toUs(percentile(sorted, 50)) },
      { name: `deno/iroh/${size}B/p95`, unit: "us", value: toUs(percentile(sorted, 95)) },
      { name: `deno/iroh/${size}B/p99`, unit: "us", value: toUs(percentile(sorted, 99)) },
      { name: `deno/iroh/${size}B/p999`, unit: "us", value: toUs(percentile(sorted, 99.9)) },
    );
  }

  const tcpConnect = await timeRequest(async () => {
    const res = await fetch(`${tcpBase}/data?size=1`);
    await res.arrayBuffer();
  });

  const irohCold = await timeRequest(async () => {
    const fresh = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
    try {
      const res = await fresh.fetch(serverId, "httpi://bench.local/data?size=1", {
        directAddrs: serverAddrs,
      });
      await res.arrayBuffer();
    } finally {
      await fresh.close();
    }
  });

  latency.push(
    { name: "deno/connection/tcp", unit: "us", value: toUs(tcpConnect) },
    { name: "deno/connection/iroh", unit: "us", value: toUs(irohCold) },
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

  latency.push({ name: `deno/multiplex/${CONCURRENCY}`, unit: "us", value: toUs(mux / CONCURRENCY) });

  if (mode === "throughput") {
    console.log(JSON.stringify(throughput, null, 2));
  } else if (mode === "latency") {
    console.log(JSON.stringify(latency, null, 2));
  } else {
    console.log(JSON.stringify({ throughput, latency }, null, 2));
  }
} finally {
  abort.abort();
  await serveHandle.finished.catch(() => {});
  await server.close();
  await client.close();
  await tcpServer.shutdown();
}
