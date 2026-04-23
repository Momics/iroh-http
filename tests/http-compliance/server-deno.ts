/**
 * Standalone compliance server — Deno
 *
 * Used for cross-runtime testing: this starts a Deno server, prints
 * READY <public-key> on stdout, and waits for connections.
 *
 * Usage:
 *   deno run -A tests/server-deno.ts
 */

import { createNode } from "../../packages/iroh-http-deno/mod.ts";
import { handleRequest } from "./handler.mjs";

const node = await createNode();

node.serve({}, handleRequest);

// Signal readiness using the same protocol as upstream tests
console.log(`READY ${node.publicKey.toString()}`);

// Keep the process alive
Deno.addSignalListener("SIGINT", () => {
  try { (node as any).shutdown?.(); } catch {}
  Deno.exit(0);
});
Deno.addSignalListener("SIGTERM", () => {
  try { (node as any).shutdown?.(); } catch {}
  Deno.exit(0);
});

// Prevent exit
await new Promise(() => {});
