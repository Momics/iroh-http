/**
 * Cross-runtime compliance server — Deno.
 *
 * Mirror of server.mjs but using the Deno FFI adapter.
 * Prints "READY:<json>" to stdout when ready, then serves until SIGINT.
 */

import { createNode } from "../../packages/iroh-http-deno/mod.ts";

function handle(req: Request): Response | Promise<Response> {
  const url = new URL(req.url);
  const parts = url.pathname.split("/").filter(Boolean);

  if (parts[0] === "status" && parts[1]) {
    const code = parseInt(parts[1], 10);
    return new Response(null, { status: isNaN(code) ? 400 : code });
  }
  if (parts[0] === "echo" && parts.length === 1) return new Response(req.body, { status: 200 });
  if (parts[0] === "echo-path") return new Response(url.pathname, { status: 200 });
  if (parts[0] === "echo-method") return new Response(req.method, { status: 200 });
  if (parts[0] === "echo-length") {
    return req
      .arrayBuffer()
      .then((buf) => new Response(String(buf.byteLength), { status: 200 }));
  }
  if (parts[0] === "echo-query") {
    return new Response(url.search, { status: 200 });
  }
  if (parts[0] === "echo-header-count") {
    const count = [...req.headers].length;
    return new Response(String(count), { status: 200 });
  }
  if (parts[0] === "header" && parts[1]) {
    return new Response(req.headers.get(parts[1]) ?? "", { status: 200 });
  }
  if (parts[0] === "set-header" && parts[1] && parts[2]) {
    return new Response(null, { status: 200, headers: { [parts[1]]: parts[2] } });
  }
  if (parts[0] === "set-headers" && parts[1]) {
    const n = parseInt(parts[1], 10);
    if (!isNaN(n) && n >= 0) {
      const hdrs: Record<string, string> = {};
      for (let i = 0; i < n; i++) hdrs[`x-h-${i}`] = `v${i}`;
      return new Response(null, { status: 200, headers: hdrs });
    }
  }
  if (parts[0] === "stream" && parts[1]) {
    const n = parseInt(parts[1], 10);
    if (!isNaN(n) && n >= 0) return new Response(new Uint8Array(n), { status: 200 });
  }
  return new Response("not found", { status: 404 });
}

const node = await createNode();
const { id: nodeId, addrs } = await node.addr();

const ac = new AbortController();
node.serve({ signal: ac.signal }, handle);

// Signal readiness.
Deno.stdout.writeSync(
  new TextEncoder().encode("READY:" + JSON.stringify({ nodeId, addrs }) + "\n"),
);
Deno.stderr.writeSync(new TextEncoder().encode(`[server.deno.ts] serving as ${nodeId}\n`));

// Wait for signal.
Deno.addSignalListener("SIGINT", async () => {
  ac.abort();
  await node.close();
  Deno.exit(0);
});
Deno.addSignalListener("SIGTERM", async () => {
  ac.abort();
  await node.close();
  Deno.exit(0);
});

await new Promise(() => {});
