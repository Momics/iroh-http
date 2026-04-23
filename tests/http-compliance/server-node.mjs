/**
 * Standalone compliance server — Node.js
 *
 * Used for cross-runtime testing: this starts a Node server, prints
 * READY <public-key> on stdout, and waits for connections.
 *
 * Usage:
 *   node tests/server-node.mjs
 */

import { createNode } from "../../packages/iroh-http-node/lib.js";
import { handleRequest } from "./handler.mjs";

const node = await createNode();

node.serve({}, handleRequest);

// Signal readiness using the same protocol as upstream tests
console.log(`READY ${node.publicKey.toString()}`);

// Keep the process alive
process.on("SIGINT", () => {
  try { node.shutdown?.(); } catch {}
  process.exit(0);
});
process.on("SIGTERM", () => {
  try { node.shutdown?.(); } catch {}
  process.exit(0);
});
