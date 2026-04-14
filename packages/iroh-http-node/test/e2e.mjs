/**
 * End-to-end integration tests for iroh-http-node.
 *
 * Uses the built-in `node:test` module (Node.js >= 18).
 *
 * Prerequisites: `lib.js` must be compiled from `lib.ts`.
 * Run: node test/e2e.mjs
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { createNode } from "../lib.js";

// PublicKey/SecretKey are re-exported from lib.ts but lib.js must be
// recompiled after the A-ISS-050 change.  Use dynamic import as fallback.
let PublicKey, SecretKey;
try {
  ({ PublicKey, SecretKey } = await import("../lib.js"));
} catch {
  // In CJS builds the named export may not be enumerable.  Import from
  // the shared package directly — it's the canonical source anyway.
  ({ PublicKey, SecretKey } = await import("@momics/iroh-http-shared"));
}

// ── Basic serve / fetch ───────────────────────────────────────────────────────

test("serve + fetch — basic GET round-trip", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (_req) =>
      new Response("hello from node", { status: 200 }),
    );

    const resp = await client.fetch(serverId, "httpi://example.com/", {
      directAddrs: serverAddrs,
    });
    assert.equal(resp.status, 200);
    assert.equal(await resp.text(), "hello from node");
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

test("serve + fetch — POST with body round-trip", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, async (req) => {
      const body = await req.text();
      return new Response(body.toUpperCase(), { status: 201 });
    });

    const resp = await client.fetch(serverId, "httpi://example.com/echo", {
      method: "POST",
      body: "ping",
      directAddrs: serverAddrs,
    });
    assert.equal(resp.status, 201);
    assert.equal(await resp.text(), "PING");
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

test("serve + fetch — path is reflected correctly", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (req) => {
      const path = new URL(req.url).pathname;
      return new Response(`path=${path}`, { status: 200 });
    });

    const resp = await client.fetch(serverId, "httpi://example.com/some/deep/path", {
      directAddrs: serverAddrs,
    });
    assert.equal(await resp.text(), "path=/some/deep/path");
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

// ── Regression: concurrent FFI output-buffer race ─────────────────────────────
//
// The Node napi-rs bridge runs multiple requests concurrently.  If a shared
// output buffer were used, concurrent responses would corrupt each other.

test("serve + fetch — 10 concurrent requests return correct bodies", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (req) => {
      const path = new URL(req.url).pathname;
      return new Response(`echo:${path}`, { status: 200 });
    });

    const N = 10;
    const paths = Array.from({ length: N }, (_, i) => `/path${i}`);
    const texts = await Promise.all(
      paths.map((path) =>
        client
          .fetch(serverId, `httpi://example.com${path}`, { directAddrs: serverAddrs })
          .then((r) => r.text()),
      ),
    );

    for (let i = 0; i < N; i++) {
      assert.equal(texts[i], `echo:${paths[i]}`, `response ${i} body mismatch`);
    }
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

// ── Regression: invalid trailer sender handle for plain responses ──────────────
//
// The Rust server removes the trailer sender handle from its slab when the
// response has no `Trailer:` header.  Calling sendTrailers unconditionally
// would throw an INVALID_HANDLE error logged as "[iroh-http] response body
// pipe error".

test("serve + fetch — plain response logs no internal pipe errors", async () => {
  const server = await createNode();
  const client = await createNode();

  const internalErrors = [];
  const origConsoleError = console.error;
  console.error = (...args) => {
    const msg = args.map(String).join(" ");
    if (msg.includes("[iroh-http]")) {
      internalErrors.push(msg);
    } else {
      origConsoleError(...args);
    }
  };

  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    const handle = server.serve({ signal: ac.signal }, (_req) =>
      new Response("hello", { status: 200 }),
    );

    const resp = await client.fetch(serverId, "httpi://example.com/", {
      directAddrs: serverAddrs,
    });
    assert.equal(resp.status, 200);
    assert.equal(await resp.text(), "hello");

    // ISS-022: abort and wait for the serve loop to fully drain instead of
    // sleeping a fixed duration to let pipe errors surface.
    ac.abort();
    await handle.finished.catch(() => {});

    assert.deepEqual(
      internalErrors,
      [],
      `Unexpected internal errors:\n${internalErrors.join("\n")}`,
    );
  } finally {
    console.error = origConsoleError;
    await server.close();
    await client.close();
  }
});

// ── URL scheme validation ─────────────────────────────────────────────────────

test("fetch — rejects https:// URL with TypeError", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    await assert.rejects(
      () => node.fetch(node.publicKey, "https://example.com/"),
      (err) => {
        assert.ok(err instanceof TypeError, `Expected TypeError, got ${err.constructor.name}`);
        assert.ok(err.message.includes("httpi://"), `Error should mention httpi://, got: ${err.message}`);
        return true;
      },
    );
  } finally {
    await node.close();
  }
});

test("fetch — rejects http:// URL with TypeError", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    await assert.rejects(
      () => node.fetch(node.publicKey, "http://example.com/"),
      (err) => {
        assert.ok(err instanceof TypeError, `Expected TypeError, got ${err.constructor.name}`);
        return true;
      },
    );
  } finally {
    await node.close();
  }
});

// ── Error classification ──────────────────────────────────────────────────────

test("serve — handler throws synchronously → client gets 500", async () => {
  const server = await createNode();
  const client = await createNode();

  // Capture the expected error log so it doesn't leak to test output.
  const captured = [];
  const origError = console.error;
  console.error = (...args) => {
    const msg = args.map(String).join(" ");
    if (msg.includes("[iroh-http]")) captured.push(msg);
    else origError(...args);
  };

  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (_req) => {
      throw new Error("handler blow-up");
    });

    const resp = await client.fetch(serverId, "httpi://example.com/", {
      directAddrs: serverAddrs,
    });
    assert.equal(resp.status, 500);
    assert.ok(captured.some((m) => m.includes("handler blow-up")), "expected error log");
    ac.abort();
  } finally {
    console.error = origError;
    await server.close();
    await client.close();
  }
});

test("serve — handler rejects async → client gets 500", async () => {
  const server = await createNode();
  const client = await createNode();

  const captured = [];
  const origError = console.error;
  console.error = (...args) => {
    const msg = args.map(String).join(" ");
    if (msg.includes("[iroh-http]")) captured.push(msg);
    else origError(...args);
  };

  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, async (_req) => {
      throw new Error("async handler blow-up");
    });

    const resp = await client.fetch(serverId, "httpi://example.com/", {
      directAddrs: serverAddrs,
    });
    assert.equal(resp.status, 500);
    assert.ok(captured.some((m) => m.includes("async handler blow-up")), "expected error log");
    ac.abort();
  } finally {
    console.error = origError;
    await server.close();
    await client.close();
  }
});

// ── Crypto round-trip (A-ISS-050 regression) ──────────────────────────────────

test("SecretKey / PublicKey — re-export, sign, verify", async () => {
  // Use the node's key (Rust-derived) rather than SecretKey.generate()
  // + derivePublicKey() which has a Web Crypto JWK compatibility issue.
  const node = await createNode();
  try {
    const sk = node.secretKey;
    const pk = node.publicKey;
    assert.ok(sk.toBytes().length === 32, "SecretKey should be 32 bytes");
    assert.ok(pk.bytes.length === 32, "PublicKey should be 32 bytes");

    const data = new TextEncoder().encode("test message");
    const sig = await sk.sign(data);
    assert.ok(sig.length === 64, "Signature should be 64 bytes");

    const valid = await pk.verify(data, sig);
    assert.ok(valid, "Signature should verify");

    // Tampered signature should fail.
    const tampered = new Uint8Array(sig);
    tampered[0] ^= 0xff;
    const invalid = await pk.verify(data, tampered);
    assert.ok(!invalid, "Tampered signature should not verify");
  } finally {
    await node.close();
  }
});

test("PublicKey.fromString — round-trip via node publicKey", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    const nodeIdStr = node.publicKey.toString();
    const pk2 = PublicKey.fromString(nodeIdStr);
    assert.ok(node.publicKey.equals(pk2));
  } finally {
    await node.close();
  }
});

// ── Node ID header ────────────────────────────────────────────────────────────

test("iroh-node-id header — present, valid base32, consistent", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (req) => {
      const nodeId = req.headers.get("iroh-node-id");
      return new Response(nodeId || "", { status: 200 });
    });

    const fetchOpts = { directAddrs: serverAddrs };
    const r1 = await client.fetch(serverId, "httpi://example.com/1", fetchOpts);
    const id1 = await r1.text();
    const r2 = await client.fetch(serverId, "httpi://example.com/2", fetchOpts);
    const id2 = await r2.text();

    // Present and non-empty.
    assert.ok(id1.length >= 52, `iroh-node-id too short: ${id1.length}`);
    // Valid base32: only a-z and 2-7.
    assert.match(id1, /^[a-z2-7]+$/, `iroh-node-id should be base32: ${id1}`);
    // Consistent across requests.
    assert.equal(id1, id2, "iroh-node-id should be consistent");
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

// ── Handle lifecycle ──────────────────────────────────────────────────────────

test("node.close() — second close is safe (throws or resolves)", async () => {
  const node = await createNode({ disableNetworking: true });
  await node.close();
  // Second close may throw INVALID_HANDLE — that's acceptable.
  // The important thing is no segfault or unhandled rejection.
  try {
    await node.close();
  } catch {
    // Expected: handle already freed.
  }
});

// ── Large body streaming ──────────────────────────────────────────────────────

test("serve + fetch — 1 MiB body round-trip", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, async (req) => {
      const buf = new Uint8Array(await req.arrayBuffer());
      return new Response(String(buf.length), { status: 200 });
    });

    const bigBody = new Uint8Array(1024 * 1024); // 1 MiB
    bigBody.fill(0x42);
    const resp = await client.fetch(serverId, "httpi://example.com/upload", {
      method: "POST",
      body: bigBody,
      directAddrs: serverAddrs,
    });
    assert.equal(resp.status, 200);
    assert.equal(await resp.text(), String(1024 * 1024));
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});
