/**
 * iroh-http Deno example.
 *
 * deno task server
 * deno task client -- <node-id>
 */

import { createNode } from "@momics/iroh-http-deno";

const [mode, peerId] = Deno.args;

const node = await createNode();
console.log("Node ID:", node.publicKey.toString());

if (mode === "server") {
  node.serve({}, (req) => {
    const path = new URL(req.url).pathname;
    console.log("Incoming:", req.method, path);
    return new Response(`Hello from Deno iroh-http! Path: ${path}`);
  });
  console.log("Serving. Share your node ID with the client.");
} else if (mode === "client" && peerId) {
  const res = await node.fetch(peerId, "/hello");
  console.log("Status:", res.status);
  console.log("Body:", await res.text());
  await node.close();
} else {
  console.error("Usage: deno task server | deno task client <peer-id>");
  Deno.exit(1);
}
