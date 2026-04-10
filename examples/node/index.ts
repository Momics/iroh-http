/**
 * iroh-http Node.js example.
 *
 * Run two instances of this script:
 *   node 1: npm start -- server
 *   node 2: npm start -- client <node1-id>
 */

import { createNode } from "@momics/iroh-http-node";

const [mode, peerId] = process.argv.slice(2);

const node = await createNode();
console.log("Node ID:", node.publicKey.toString());

if (mode === "server") {
  node.serve({}, async (req) => {
    const path = new URL(req.url).pathname;
    console.log("Incoming request:", req.method, path);
    return new Response(`Hello from iroh-http! Path: ${path}`, {
      headers: { "content-type": "text/plain" },
    });
  });
  console.log("Serving. Share your node ID with the client.");
} else if (mode === "client" && peerId) {
  const res = await node.fetch(peerId, "/hello");
  console.log("Response status:", res.status);
  console.log("Body:", await res.text());
  await node.close();
} else {
  console.error("Usage: tsx index.ts server | tsx index.ts client <peer-id>");
  process.exit(1);
}
