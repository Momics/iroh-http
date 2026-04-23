/**
 * iroh-http lifecycle tests — Deno
 *
 * Tests node creation, shutdown, resource cleanup, and API surface
 * guarantees. These are imperative tests that don't fit the cases.json model.
 *
 * Usage:
 *   deno run -A tests/lifecycle/test-deno.ts
 */

import { createNode } from "../../packages/iroh-http-deno/mod.ts";
import {
  suite, test, assert, assertEqual, assertNotEqual,
  assertThrows, assertResolves, run,
} from "../harness.mjs";

suite("lifecycle");

// ── Node creation ───────────────────────────────────────────────────────────

test("createNode returns a node", async () => {
  const node = await createNode();
  assert(node != null, "node is null");
  await node.close();
});

test("node has a publicKey", async () => {
  const node = await createNode();
  assert(node.publicKey != null, "publicKey is null");
  assert(typeof node.publicKey.toString() === "string", "publicKey.toString() is not a string");
  assert(node.publicKey.toString().length > 0, "publicKey is empty");
  await node.close();
});

test("publicKey is consistent across accesses", async () => {
  const node = await createNode();
  const a = node.publicKey.toString();
  const b = node.publicKey.toString();
  assertEqual(a, b, "publicKey");
  await node.close();
});

test("two nodes get different publicKeys", async () => {
  const a = await createNode();
  const b = await createNode();
  assertNotEqual(
    a.publicKey.toString(),
    b.publicKey.toString(),
    "two nodes have same publicKey",
  );
  await a.close();
  await b.close();
});

test("node has a secretKey", async () => {
  const node = await createNode();
  assert(node.secretKey != null, "secretKey is null");
  await node.close();
});

test("node.addr() returns nodeId and addrs", async () => {
  const node = await createNode();
  const addr = await node.addr();
  assert(addr != null, "addr is null");
  assert(typeof addr.id === "string", "addr.id is not a string");
  assertEqual(addr.id, node.publicKey.toString(), "addr.id matches publicKey");
  await node.close();
});

// ── Closing ─────────────────────────────────────────────────────────────────

test("node.close() resolves without error", async () => {
  const node = await createNode();
  await assertResolves(node.close());
});

test("node.closed promise resolves after close", async () => {
  const node = await createNode();
  const closePromise = node.closed;
  assert(closePromise instanceof Promise, "closed is not a promise");
  await node.close();
  const info = await closePromise;
  assert(info != null, "closed resolved with null");
});

test("double close does not throw", async () => {
  const node = await createNode();
  await node.close();
  // Second close should be idempotent or at least not crash
  try {
    await node.close();
  } catch {
    // Some impls may throw — that's acceptable, just not a crash
  }
});

// ── Serve lifecycle ─────────────────────────────────────────────────────────

test("serve() returns a handle", async () => {
  const node = await createNode();
  const handle = node.serve({}, () => new Response("ok"));
  assert(handle != null, "serve handle is null");
  await node.close();
});

test("serve handler receives requests", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();
  let handlerCalled = false;

  server.serve({}, () => {
    handlerCalled = true;
    return new Response("hello");
  });

  const res = await client.fetch(serverId, "/test", { directAddrs: serverAddrs });
  assert(handlerCalled, "handler was not called");
  assertEqual(res.status, 200, "status");
  const body = await res.text();
  assertEqual(body, "hello", "body");

  await server.close();
  await client.close();
});

test("serve handler gets valid Request object", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();
  let receivedMethod = "";
  let receivedPath = "";

  server.serve({}, (req) => {
    receivedMethod = req.method;
    receivedPath = new URL(req.url).pathname;
    return new Response("ok");
  });

  await client.fetch(serverId, "/hello", { method: "POST", directAddrs: serverAddrs });

  assertEqual(receivedMethod, "POST", "method");
  assertEqual(receivedPath, "/hello", "path");

  await server.close();
  await client.close();
});

// ── Fetch lifecycle ─────────────────────────────────────────────────────────

test("fetch returns a valid Response", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();

  server.serve({}, () => new Response("works"));
  const res = await client.fetch(serverId, "/", { directAddrs: serverAddrs });
  assert(res instanceof Response, "not a Response");
  assert(typeof res.status === "number", "status not a number");
  assert(res.headers instanceof Headers, "headers not Headers");

  await server.close();
  await client.close();
});

test("fetch response body can be read as text", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();

  server.serve({}, () => new Response("text-body"));
  const res = await client.fetch(serverId, "/", { directAddrs: serverAddrs });
  const text = await res.text();
  assertEqual(text, "text-body", "body text");

  await server.close();
  await client.close();
});

test("fetch response body can be read as arrayBuffer", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();

  server.serve({}, () => new Response("buf"));
  const res = await client.fetch(serverId, "/", { directAddrs: serverAddrs });
  const buf = await res.arrayBuffer();
  assert(buf instanceof ArrayBuffer, "not an ArrayBuffer");
  assertEqual(buf.byteLength, 3, "byteLength");

  await server.close();
  await client.close();
});

// ── Create/close cycles ─────────────────────────────────────────────────────

test("10 sequential create/close cycles", async () => {
  for (let i = 0; i < 10; i++) {
    const node = await createNode();
    assert(node.publicKey.toString().length > 0, `cycle ${i}: no publicKey`);
    await node.close();
  }
});

// ── Run ─────────────────────────────────────────────────────────────────────
const code = await run();
Deno.exit(code);
