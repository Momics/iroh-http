/**
 * Smoke test — verifies the native addon loads and basic operations work.
 *
 * Run: deno run --allow-read --allow-ffi test/smoke.ts
 *
 * Note: the native library must be built first (`deno task build`).
 */

import { createNode } from "../mod.ts";
import { assertEquals, assertExists, assertGreater } from "jsr:@std/assert@^1";

Deno.test("smoke: createNode, nodeId, close", async () => {
  const node = await createNode({ relayMode: "disabled" });

  assertExists(node.nodeId, "nodeId should be a non-empty string");
  assertGreater(node.nodeId.length, 10, "nodeId should be base32-encoded");
  console.log(`   nodeId = ${node.nodeId}`);

  const kp = node.keypair;
  assertEquals(kp instanceof Uint8Array, true, "keypair should be Uint8Array");
  assertEquals(kp.length, 32, "keypair should be 32 bytes");

  assertExists(node.publicKey, "publicKey should exist");
  assertEquals(
    node.publicKey.toString(),
    node.nodeId,
    "publicKey.toString() should match nodeId",
  );

  assertExists(node.secretKey, "secretKey should exist");
  assertEquals(
    node.secretKey.toBytes().length,
    32,
    "secretKey should be 32 bytes",
  );

  await node.close();
});
