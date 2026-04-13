/**
 * Cross-runtime compliance server — Node.js.
 *
 * Starts an iroh-http node, prints its nodeId and direct addresses as JSON to
 * stdout (one line, prefixed "READY:"), then serves compliance echo requests
 * until SIGINT/SIGTERM.
 *
 * The orchestrator (run.sh) reads the READY line, then spawns client processes
 * in other runtimes that connect to this server.
 *
 * Run standalone:
 *   node tests/http-compliance/server.mjs
 */

import { createNode } from "../../packages/iroh-http-node/lib.js";

// ── Compliance echo handler (identical logic to compliance.mjs) ───────────────

function handle(req) {
  const url = new URL(req.url);
  const parts = url.pathname.split("/").filter(Boolean);

  if (parts[0] === "status" && parts[1]) {
    const code = parseInt(parts[1], 10);
    return new Response(null, { status: isNaN(code) ? 400 : code });
  }
  if (parts[0] === "echo" && parts.length === 1) {
    return new Response(req.body, { status: 200 });
  }
  if (parts[0] === "echo-path") {
    return new Response(url.pathname, { status: 200 });
  }
  if (parts[0] === "echo-method") {
    return new Response(req.method, { status: 200 });
  }
  if (parts[0] === "echo-length") {
    return req.arrayBuffer().then((buf) => new Response(String(buf.byteLength), { status: 200 }));
  }
  if (parts[0] === "header" && parts[1]) {
    return new Response(req.headers.get(parts[1]) ?? "", { status: 200 });
  }
  if (parts[0] === "set-header" && parts[1] && parts[2]) {
    return new Response(null, { status: 200, headers: { [parts[1]]: parts[2] } });
  }
  if (parts[0] === "stream" && parts[1]) {
    const n = parseInt(parts[1], 10);
    if (!isNaN(n) && n >= 0) return new Response(new Uint8Array(n), { status: 200 });
  }
  return new Response("not found", { status: 404 });
}

// ── Start ─────────────────────────────────────────────────────────────────────

const node = await createNode();
const { id: nodeId, addrs } = await node.addr();

const ac = new AbortController();
node.serve({ signal: ac.signal }, handle);

// Signal readiness — orchestrator waits for this line.
process.stdout.write(
  "READY:" + JSON.stringify({ nodeId, addrs }) + "\n",
);
process.stderr.write(`[server.mjs] serving as ${nodeId}\n`);

// Graceful shutdown on signal.
process.once("SIGINT", async () => {
  process.stderr.write("[server.mjs] shutting down\n");
  ac.abort();
  await node.close();
  process.exit(0);
});
process.once("SIGTERM", async () => {
  ac.abort();
  await node.close();
  process.exit(0);
});

// Keep the process alive.
await new Promise(() => {});
