/**
 * Smoke test — verifies the native addon loads and basic operations work.
 *
 * Run (after `deno task build`):
 *   deno test --allow-read --allow-ffi test/smoke.ts
 *
 * Or as a plain script:
 *   deno run --allow-read --allow-ffi test/smoke.ts
 */

import { assertEquals, assertExists, assertInstanceOf, assert } from "jsr:@std/assert@^1";
import { createNode } from "../mod.ts";
import { secretKeySign, publicKeyVerify, generateSecretKey } from "../mod.ts";

// ── Node creation ──────────────────────────────────────────────────────────────

Deno.test("createNode — nodeId is a non-empty base32 string", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    assertExists(node.nodeId, "nodeId must exist");
    assert(node.nodeId.length > 10, `nodeId too short: ${node.nodeId}`);
    console.log(`  nodeId = ${node.nodeId}`);
  } finally {
    await node.close();
  }
});

Deno.test("createNode — keypair is a 32-byte Uint8Array", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    assertInstanceOf(node.keypair, Uint8Array, "keypair must be Uint8Array");
    assertEquals(node.keypair.length, 32, "keypair must be 32 bytes");
  } finally {
    await node.close();
  }
});

Deno.test("createNode — same key bytes produce same nodeId", async () => {
  const key = new Uint8Array(32).fill(0xab);
  const n1 = await createNode({ key, disableNetworking: true });
  const n2 = await createNode({ key, disableNetworking: true });
  try {
    assertEquals(n1.nodeId, n2.nodeId, "deterministic key must yield deterministic nodeId");
  } finally {
    await n1.close();
    await n2.close();
  }
});

Deno.test("createNode — ticket() returns a non-trivial string", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    const ticket = await node.ticket();
    assert(typeof ticket === "string" && ticket.length > 20, "ticket must be a substantial string");
  } finally {
    await node.close();
  }
});

Deno.test("createNode — addr() returns id and address array", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    const info = await node.addr();
    assertExists(info.id, "addr must have id");
    assert(Array.isArray(info.addrs), "addr.addrs must be an array");
  } finally {
    await node.close();
  }
});

// ── Cryptography ───────────────────────────────────────────────────────────────

Deno.test("generateSecretKey — returns 32 bytes", async () => {
  const key = await generateSecretKey();
  assertInstanceOf(key, Uint8Array);
  assertEquals(key.length, 32);
});

Deno.test("generateSecretKey — successive calls differ", async () => {
  const k1 = await generateSecretKey();
  const k2 = await generateSecretKey();
  assert(
    !k1.every((b: number, i: number) => b === k2[i]),
    "Two generated keys must differ",
  );
});

Deno.test("secretKeySign — returns 64-byte signature", async () => {
  const key = await generateSecretKey();
  const sig = await secretKeySign(key, new TextEncoder().encode("hello"));
  assertInstanceOf(sig, Uint8Array);
  assertEquals(sig.length, 64);
});

Deno.test("secretKeySign — deterministic for same key + message", async () => {
  const key = await generateSecretKey();
  const msg = new TextEncoder().encode("deterministic");
  const s1 = await secretKeySign(key, msg);
  const s2 = await secretKeySign(key, msg);
  assertEquals(s1, s2);
});

Deno.test("publicKeyVerify — valid signature passes", async () => {
  const key = await generateSecretKey();
  const node = await createNode({ key, disableNetworking: true });
  const msg = new TextEncoder().encode("test message");
  const sig = await secretKeySign(key, msg);

  const pubBytes = node.publicKey.bytes;
  try {
    assert(await publicKeyVerify(pubBytes, msg, sig), "Valid signature must verify");
    const tampered = new Uint8Array(sig);
    tampered[0] ^= 0xff;
    assert(!(await publicKeyVerify(pubBytes, msg, tampered)), "Tampered signature must fail");
  } finally {
    await node.close();
  }
});

// ── Serve / fetch round-trip ───────────────────────────────────────────────────
//
// Networking tests use ticket() instead of addr().directAddrs so that iroh
// connects through the relay server rather than attempting QUIC loopback.
// QUIC loopback to 127.0.0.1 is unreliable on macOS; relay works everywhere.

Deno.test("serve + fetch — basic round-trip", async () => {
  const server = await createNode();
  const client = await createNode();

  try {
    // ticket() encodes the relay URL; passing it as peerId routes via relay.
    const ticket = await server.ticket();

    const ac = new AbortController();
    const handle = server.serve({ signal: ac.signal }, (_req: Request) =>
      new Response("hello from deno", { status: 200 }),
    );

    const resp = await client.fetch(ticket, "https://example.com/");
    assertEquals(resp.status, 200);
    const text = await resp.text();
    assertEquals(text, "hello from deno");

    ac.abort();
    await handle.finished;
  } finally {
    await server.close();
    await client.close();
  }
});

Deno.test("serve + fetch — POST with body", async () => {
  const server = await createNode();
  const client = await createNode();

  try {
    const ticket = await server.ticket();

    const ac = new AbortController();
    const handle = server.serve({ signal: ac.signal }, async (req: Request) => {
      const body = await req.text();
      return new Response(body.toUpperCase(), { status: 201 });
    });

    const resp = await client.fetch(ticket, "https://example.com/echo", {
      method: "POST",
      body: "ping",
    });
    assertEquals(resp.status, 201);
    assertEquals(await resp.text(), "PING");

    ac.abort();
    await handle.finished;
  } finally {
    await server.close();
    await client.close();
  }
});

// ── Regression: concurrent FFI call buffer race ────────────────────────────────
//
// Before the fix, `iroh_http_call` was nonblocking (concurrent) but all calls
// shared one output buffer — concurrent responses would overwrite each other,
// producing corrupted JSON ("Unexpected non-whitespace character after JSON").

Deno.test("serve + fetch — concurrent requests return correct bodies (no buffer race)", async () => {
  const server = await createNode();
  const client = await createNode();

  try {
    const ticket = await server.ticket();

    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (req: Request) => {
      const path = new URL(req.url).pathname;
      return new Response(`echo:${path}`, { status: 200 });
    });

    // Fire 10 requests simultaneously — if buffers are shared this will corrupt.
    const N = 10;
    const paths = Array.from({ length: N }, (_, i) => `/path${i}`);
    const texts = await Promise.all(
      paths.map((path) =>
        client
          .fetch(ticket, `https://example.com${path}`)
          .then((r) => r.text())
      ),
    );

    for (let i = 0; i < N; i++) {
      assertEquals(texts[i], `echo:${paths[i]}`, `response ${i} body mismatch`);
    }

    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

// ── Regression: invalid trailer sender handle for plain responses ──────────────
//
// Before the fix, serve.ts called bridge.sendTrailers() for every response,
// but the Rust server removes the trailer sender handle from its slab when the
// response carries no `Trailer:` header.  This produced:
//   [iroh-http] response body pipe error: IrohHandleError: invalid trailer sender handle

Deno.test("serve + fetch — plain response produces no internal pipe errors", async () => {
  const server = await createNode();
  const client = await createNode();

  // Intercept console.error to catch any [iroh-http] internal errors.
  const internalErrors: string[] = [];
  const origConsoleError = console.error;
  console.error = (...args: unknown[]) => {
    const msg = args.map(String).join(" ");
    if (msg.includes("[iroh-http]")) {
      internalErrors.push(msg);
    } else {
      origConsoleError(...args);
    }
  };

  try {
    const ticket = await server.ticket();

    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (_req: Request) =>
      new Response("hello", { status: 200 })
    );

    const resp = await client.fetch(ticket, "https://example.com/");
    assertEquals(resp.status, 200);
    assertEquals(await resp.text(), "hello");

    // Yield briefly so any async pipe errors from doPipe() have time to surface.
    await new Promise((resolve) => setTimeout(resolve, 150));

    assertEquals(
      internalErrors,
      [],
      `Unexpected internal errors:\n${internalErrors.join("\n")}`,
    );

    ac.abort();
  } finally {
    console.error = origConsoleError;
    await server.close();
    await client.close();
  }
});
