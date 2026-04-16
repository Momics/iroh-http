/**
 * Throughput and latency benchmarks for iroh-http-deno.
 *
 * Run (after `deno task build`):
 *   deno bench --allow-read --allow-ffi --allow-env --allow-net bench/throughput.ts
 *
 * Each Deno.bench() call represents one request (or batch) so Deno's built-in
 * harness handles repetitions and reports ns/iter automatically.
 */

import { createNode } from "../mod.ts";
import type { IrohNode } from "@momics/iroh-http-shared";

// ── shared fixtures ───────────────────────────────────────────────────────────

interface Fixture {
  server: IrohNode;
  client: IrohNode;
  serverId: string;
  serverAddrs: string[];
  ac: AbortController;
}

async function makeFixture(
  handler: (req: Request) => Response | Promise<Response>,
): Promise<Fixture> {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await (server as any).addr();
  const ac = new AbortController();
  (server as any).serve({ signal: ac.signal }, handler);
  return { server, client, serverId, serverAddrs, ac };
}

async function teardown({ server, client, ac }: Fixture) {
  ac.abort();
  await server.close();
  await client.close();
}

// ── bench 1: single GET round-trip latency ────────────────────────────────────

{
  const fix = await makeFixture(() => new Response("ok", { status: 200 }));

  Deno.bench(
    {
      name: "fetch_get_latency",
      baseline: true,
    },
    async () => {
      const r = await (fix.client as any).fetch(
        fix.serverId,
        "httpi://example.com/",
        { directAddrs: fix.serverAddrs },
      );
      await r.text();
    },
  );

  // Use an unload event to clean up the fixture after all benches finish.
  addEventListener("unload", () => teardown(fix));
}

// ── bench 2: POST 1 KB body ───────────────────────────────────────────────────

{
  const fix = await makeFixture(async (req) => {
    await req.arrayBuffer();
    return new Response("", { status: 200 });
  });
  const body1k = new Uint8Array(1_024).fill(0x42);

  Deno.bench("post_body_1kb", async () => {
    const r = await (fix.client as any).fetch(
      fix.serverId,
      "httpi://example.com/up",
      { method: "POST", body: body1k, directAddrs: fix.serverAddrs },
    );
    await r.text();
  });

  addEventListener("unload", () => teardown(fix));
}

// ── bench 3: POST 1 MB body ───────────────────────────────────────────────────

{
  const fix = await makeFixture(async (req) => {
    await req.arrayBuffer();
    return new Response("", { status: 200 });
  });
  const body1m = new Uint8Array(1_024 * 1_024).fill(0x42);

  Deno.bench("post_body_1mb", async () => {
    const r = await (fix.client as any).fetch(
      fix.serverId,
      "httpi://example.com/up",
      { method: "POST", body: body1m, directAddrs: fix.serverAddrs },
    );
    await r.text();
  });

  addEventListener("unload", () => teardown(fix));
}

// ── bench 4: response body streaming 1 MB ────────────────────────────────────

{
  const fix = await makeFixture((req) => {
    const n = parseInt(new URL(req.url).pathname.split("/").pop() ?? "0", 10);
    return new Response(new Uint8Array(n).fill(0x00), { status: 200 });
  });

  Deno.bench("response_body_streaming_1mb", async () => {
    const r = await (fix.client as any).fetch(
      fix.serverId,
      `httpi://example.com/bench/${1_024 * 1_024}`,
      { directAddrs: fix.serverAddrs },
    );
    await r.arrayBuffer();
  });

  addEventListener("unload", () => teardown(fix));
}

// ── bench 5: 10 concurrent GETs ───────────────────────────────────────────────

{
  const fix = await makeFixture((req) => {
    const path = new URL(req.url).pathname;
    return new Response(`echo:${path}`, { status: 200 });
  });
  const paths = Array.from({ length: 10 }, (_, i) => `/r${i}`);

  Deno.bench("concurrent_requests_10x", async () => {
    await Promise.all(
      paths.map(async (p) => {
        const r = await (fix.client as any).fetch(
          fix.serverId,
          `httpi://example.com${p}`,
          { directAddrs: fix.serverAddrs },
        );
        return r.text();
      }),
    );
  });

  addEventListener("unload", () => teardown(fix));
}
