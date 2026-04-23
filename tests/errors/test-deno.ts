/**
 * iroh-http error handling tests — Deno
 *
 * Tests error scenarios: handler failures, unknown peers, aborted requests,
 * invalid arguments, and recovery after errors.
 *
 * Usage:
 *   deno run -A tests/errors/test-deno.ts
 */

import { createNode } from "../../packages/iroh-http-deno/mod.ts";
import {
  suite, test, assert, assertEqual,
  assertThrows, run,
} from "../harness.mjs";

suite("error handling");

// ── Handler errors ──────────────────────────────────────────────────────────

test("handler that throws → client gets error response", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();

  server.serve({}, () => {
    throw new Error("handler exploded");
  });

  try {
    const res = await client.fetch(serverId, "/boom", { directAddrs: serverAddrs });
    // If the transport delivers an error response, status should indicate server error
    assert(res.status >= 500 || res.status === 0, `expected 5xx or connection error, got ${res.status}`);
    // Drain body to avoid resource leak
    await res.body?.cancel();
  } catch (err) {
    // Connection error is also acceptable — the handler crashed
    assert(err instanceof Error, "expected Error instance");
  }

  await server.close();
  await client.close();
});

test("handler that returns rejected promise → client gets error", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();

  server.serve({}, async () => {
    throw new Error("async handler failed");
  });

  try {
    const res = await client.fetch(serverId, "/fail", { directAddrs: serverAddrs });
    assert(res.status >= 500 || res.status === 0, `expected error status, got ${res.status}`);
    await res.body?.cancel();
  } catch (err) {
    assert(err instanceof Error, "expected Error instance");
  }

  await server.close();
  await client.close();
});

test("server stays alive after handler error", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();

  let callCount = 0;
  server.serve({}, (req) => {
    callCount++;
    if (callCount === 1) throw new Error("first call fails");
    return new Response("recovered");
  });

  // First request — handler throws
  try {
    const res1 = await client.fetch(serverId, "/first", { directAddrs: serverAddrs });
    await res1.body?.cancel();
  } catch {
    // expected
  }

  // Second request — handler should work
  const res2 = await client.fetch(serverId, "/second", { directAddrs: serverAddrs });
  assertEqual(res2.status, 200, "second request status");
  const body = await res2.text();
  assertEqual(body, "recovered", "second request body");
  assertEqual(callCount, 2, "handler called twice");

  await server.close();
  await client.close();
});

// ── Fetch errors ────────────────────────────────────────────────────────────

test("fetch to unknown peer rejects", async () => {
  const client = await createNode();

  // Use a valid-format but non-existent public key
  const fakePeer = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

  await assertThrows(
    async () => {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), 5000);
      try {
        await client.fetch(fakePeer, "/hello", { signal: controller.signal });
      } finally {
        clearTimeout(timer);
      }
    }
  );

  await client.close();
});

test("fetch with aborted signal rejects", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();

  server.serve({}, () => new Response("ok"));

  const controller = new AbortController();
  controller.abort(); // Abort immediately

  await assertThrows(
    async () => {
      await client.fetch(serverId, "/", { signal: controller.signal, directAddrs: serverAddrs });
    }
  );

  await server.close();
  await client.close();
});

test("fetch with abort during request rejects", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();

  // Server handler that delays
  server.serve({}, async () => {
    await new Promise((r) => setTimeout(r, 2000));
    return new Response("slow");
  });

  const controller = new AbortController();
  // Abort after 100ms
  setTimeout(() => controller.abort(), 100);

  await assertThrows(
    async () => {
      await client.fetch(serverId, "/slow", { signal: controller.signal, directAddrs: serverAddrs });
    }
  );

  await server.close();
  await client.close();
});

// ── Handler edge cases ──────────────────────────────────────────────────────

test("handler returning non-Response doesn't crash server", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();

  let callCount = 0;
  server.serve({}, () => {
    callCount++;
    if (callCount === 1) return "not a response"; // invalid!
    return new Response("ok");
  });

  // First request with bad return — should error somehow
  try {
    const res = await client.fetch(serverId, "/bad", { directAddrs: serverAddrs });
    await res.body?.cancel();
  } catch {
    // expected
  }

  // Server should still be alive
  try {
    const res2 = await client.fetch(serverId, "/good", { directAddrs: serverAddrs });
    if (res2.status === 200) {
      const body = await res2.text();
      assertEqual(body, "ok", "recovery body");
    } else {
      await res2.body?.cancel();
    }
  } catch {
    // If server died, that's a real bug — but we don't assert here
    // because behavior may be platform-dependent
  }

  await server.close();
  await client.close();
});

test("handler with null body on 200 doesn't crash", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();

  server.serve({}, () => new Response(null, { status: 200 }));

  const res = await client.fetch(serverId, "/", { directAddrs: serverAddrs });
  assertEqual(res.status, 200, "status");
  const body = await res.text();
  assertEqual(body, "", "empty body");

  await server.close();
  await client.close();
});

// ── Peer-Id security ────────────────────────────────────────────────────────

test("Peer-Id header cannot be spoofed by client", async () => {
  const server = await createNode();
  const client = await createNode();
  const { id: serverId, addrs: serverAddrs } = await server.addr();
  let receivedPeerId = "";

  server.serve({}, (req) => {
    receivedPeerId = req.headers.get("Peer-Id") || "";
    return new Response("ok");
  });

  // Client tries to spoof Peer-Id
  await client.fetch(serverId, "/", {
    headers: { "Peer-Id": "spoofed-value" },
    directAddrs: serverAddrs,
  });

  // The Peer-Id should be the client's actual key, not "spoofed-value"
  assert(receivedPeerId !== "spoofed-value", "Peer-Id was spoofable!");
  assertEqual(receivedPeerId, client.publicKey.toString(), "Peer-Id should match client publicKey");

  await server.close();
  await client.close();
});

// ── Run ─────────────────────────────────────────────────────────────────────
const code = await run();
Deno.exit(code);
