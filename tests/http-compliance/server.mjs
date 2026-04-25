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
import { handleRequest } from "./handler.mjs";

// ── Start ─────────────────────────────────────────────────────────────────────

const node = await createNode();
const { id: nodeId, addrs } = await node.addr();

const ac = new AbortController();
node.serve({ signal: ac.signal }, handleRequest);

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
