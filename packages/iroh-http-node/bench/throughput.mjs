/**
 * Throughput and latency benchmarks for iroh-http-node.
 *
 * Requires the native addon to be compiled.
 * Run: node bench/throughput.mjs
 *
 * Each benchmark is repeated `REPS` times; wall-clock time is reported as
 * ops/s (latency benches) or MB/s (throughput benches).
 */

import { createNode } from "../lib.js";

const REPS = 50;

// ── helpers ───────────────────────────────────────────────────────────────────

function msSince(hrstart) {
  const [s, ns] = process.hrtime(hrstart);
  return s * 1e3 + ns / 1e6;
}

function report(name, ms, bytes = 0) {
  const ops = (REPS / (ms / 1000)).toFixed(1);
  if (bytes > 0) {
    const mbps = ((bytes * REPS) / (ms / 1000) / (1024 * 1024)).toFixed(2);
    console.log(`  ${name.padEnd(40)} ${ops.padStart(8)} ops/s   ${mbps.padStart(8)} MB/s`);
  } else {
    console.log(`  ${name.padEnd(40)} ${ops.padStart(8)} ops/s`);
  }
}

async function setup() {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();
  const ac = new AbortController();
  return { server, client, serverId, serverAddrs, ac };
}

// ── bench 1: single GET round-trip latency ────────────────────────────────────

async function benchGetLatency() {
  const { server, client, serverId, serverAddrs, ac } = await setup();
  server.serve({ signal: ac.signal }, () => new Response("ok", { status: 200 }));

  // warm-up
  await client.fetch(serverId, "httpi://example.com/", { directAddrs: serverAddrs });

  const t = process.hrtime();
  for (let i = 0; i < REPS; i++) {
    const r = await client.fetch(serverId, "httpi://example.com/", { directAddrs: serverAddrs });
    await r.text();
  }
  const elapsed = msSince(t);
  report("fetch_get_latency", elapsed);

  ac.abort();
  await server.close();
  await client.close();
}

// ── bench 2: POST body throughput (1 KB, 64 KB, 1 MB) ────────────────────────

async function benchPostBodyThroughput() {
  const { server, client, serverId, serverAddrs, ac } = await setup();
  server.serve({ signal: ac.signal }, async (req) => {
    await req.arrayBuffer(); // drain
    return new Response("", { status: 200 });
  });

  for (const size of [1_024, 64 * 1_024, 1_024 * 1_024]) {
    const body = new Uint8Array(size).fill(0x42);

    // warm-up
    await (await client.fetch(serverId, "httpi://example.com/up", {
      method: "POST", body, directAddrs: serverAddrs,
    })).text();

    const t = process.hrtime();
    for (let i = 0; i < REPS; i++) {
      const r = await client.fetch(serverId, "httpi://example.com/up", {
        method: "POST", body, directAddrs: serverAddrs,
      });
      await r.text();
    }
    const elapsed = msSince(t);
    report(`post_body_throughput/${size}`, elapsed, size);
  }

  ac.abort();
  await server.close();
  await client.close();
}

// ── bench 3: response body streaming (server → client) ───────────────────────

async function benchResponseBodyStreaming() {
  const { server, client, serverId, serverAddrs, ac } = await setup();
  server.serve({ signal: ac.signal }, (req) => {
    const n = parseInt(new URL(req.url).pathname.split("/").pop() ?? "0", 10);
    return new Response(new Uint8Array(n).fill(0x00), { status: 200 });
  });

  for (const size of [1_024, 64 * 1_024, 1_024 * 1_024]) {
    const url = `httpi://example.com/bench/${size}`;

    // warm-up
    await (await client.fetch(serverId, url, { directAddrs: serverAddrs })).arrayBuffer();

    const t = process.hrtime();
    for (let i = 0; i < REPS; i++) {
      const r = await client.fetch(serverId, url, { directAddrs: serverAddrs });
      await r.arrayBuffer();
    }
    const elapsed = msSince(t);
    report(`response_body_streaming/${size}`, elapsed, size);
  }

  ac.abort();
  await server.close();
  await client.close();
}

// ── bench 4: 10 concurrent GET requests ──────────────────────────────────────

async function benchConcurrentRequests() {
  const { server, client, serverId, serverAddrs, ac } = await setup();
  server.serve({ signal: ac.signal }, (req) => {
    const path = new URL(req.url).pathname;
    return new Response(`echo:${path}`, { status: 200 });
  });

  const N = 10;
  const paths = Array.from({ length: N }, (_, i) => `/r${i}`);

  // warm-up
  await Promise.all(paths.map(async (p) => {
    const r = await client.fetch(serverId, `httpi://example.com${p}`, { directAddrs: serverAddrs });
    return r.text();
  }));

  const t = process.hrtime();
  for (let i = 0; i < REPS; i++) {
    await Promise.all(paths.map(async (p) => {
      const r = await client.fetch(serverId, `httpi://example.com${p}`, { directAddrs: serverAddrs });
      return r.text();
    }));
  }
  const elapsed = msSince(t);
  // Each rep does N requests, so total requests = REPS * N
  const totalOps = REPS * N;
  const ops = (totalOps / (elapsed / 1000)).toFixed(1);
  console.log(`  ${"concurrent_requests_10x".padEnd(40)} ${ops.padStart(8)} req/s`);

  ac.abort();
  await server.close();
  await client.close();
}

// ── runner ────────────────────────────────────────────────────────────────────

console.log(`\niroh-http-node throughput (${REPS} reps each)\n`);

await benchGetLatency();
await benchPostBodyThroughput();
await benchResponseBodyStreaming();
await benchConcurrentRequests();

console.log("\nDone.");
