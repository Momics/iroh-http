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
    server.serve({ signal: ac.signal }, (_req) =>
      new Response("hello", { status: 200 }),
    );

    const resp = await client.fetch(serverId, "httpi://example.com/", {
      directAddrs: serverAddrs,
    });
    assert.equal(resp.status, 200);
    assert.equal(await resp.text(), "hello");

    // Yield so any async pipe errors from the serve loop have time to surface.
    await new Promise((resolve) => setTimeout(resolve, 150));

    assert.deepEqual(
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

// ── URL scheme validation ─────────────────────────────────────────────────────

test("fetch — rejects https:// URL with TypeError", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    await assert.rejects(
      () => node.fetch(node.nodeId, "https://example.com/"),
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
      () => node.fetch(node.nodeId, "http://example.com/"),
      (err) => {
        assert.ok(err instanceof TypeError, `Expected TypeError, got ${err.constructor.name}`);
        return true;
      },
    );
  } finally {
    await node.close();
  }
});
