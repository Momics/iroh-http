/**
 * Smoke test — verifies the native addon loads and basic operations work.
 *
 * Run: node test/smoke.mjs
 *
 * Note: `lib.js` must be compiled from `lib.ts` first (e.g. via `tsc`).
 * Some methods (ticket, addr, etc.) may be missing if the JS is stale.
 */

import { createNode } from "../lib.js";
import { strict as assert } from "node:assert";

async function main() {
  console.log("1. createNode...");
  const node = await createNode({ disableNetworking: true });
  assert.ok(node.publicKey, "publicKey should exist");
  assert.ok(node.publicKey.toString().length > 10, "publicKey should be base32-encoded");
  console.log(`   publicKey = ${node.publicKey}`);

  console.log("2. secretKey...");
  const kp = node.secretKey.toBytes();
  assert.ok(kp instanceof Uint8Array, "secretKey bytes should be Uint8Array");
  assert.equal(kp.length, 32, "secretKey should be 32 bytes");

  console.log("3. publicKey...");
  assert.ok(node.publicKey, "publicKey should exist");

  console.log("4. secretKey...");
  assert.ok(node.secretKey, "secretKey should exist");
  assert.equal(node.secretKey.toBytes().length, 32, "secretKey should be 32 bytes");

  console.log("5. close...");
  await node.close();

  console.log("\n✅ All smoke tests passed.");
}

main().catch((err) => {
  console.error("❌ Smoke test failed:", err);
  process.exit(1);
});
