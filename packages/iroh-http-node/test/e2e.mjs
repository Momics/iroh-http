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
    server.serve(
      { signal: ac.signal },
      (_req) => new Response("hello from node", { status: 200 }),
    );

    const resp = await client.fetch(`httpi://${serverId}/`, {
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

    const resp = await client.fetch(`httpi://${serverId}/echo`, {
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

    const resp = await client.fetch(
      `httpi://${serverId}/some/deep/path`,
      { directAddrs: serverAddrs },
    );
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
          .fetch(`httpi://${serverId}${path}`, {
            directAddrs: serverAddrs,
          })
          .then((r) => r.text())
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
    const handle = server.serve(
      { signal: ac.signal },
      (_req) => new Response("hello", { status: 200 }),
    );

    const resp = await client.fetch(`httpi://${serverId}/`, {
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
      () => node.fetch("https://example.com/"),
      (err) => {
        assert.ok(
          err instanceof TypeError,
          `Expected TypeError, got ${err.constructor.name}`,
        );
        assert.ok(
          err.message.includes("httpi://"),
          `Error should mention httpi://, got: ${err.message}`,
        );
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
      () => node.fetch("http://example.com/"),
      (err) => {
        assert.ok(
          err instanceof TypeError,
          `Expected TypeError, got ${err.constructor.name}`,
        );
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

    const resp = await client.fetch(`httpi://${serverId}/`, {
      directAddrs: serverAddrs,
    });
    assert.equal(resp.status, 500);
    assert.ok(
      captured.some((m) => m.includes("handler blow-up")),
      "expected error log",
    );
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

    const resp = await client.fetch(`httpi://${serverId}/`, {
      directAddrs: serverAddrs,
    });
    assert.equal(resp.status, 500);
    assert.ok(
      captured.some((m) => m.includes("async handler blow-up")),
      "expected error log",
    );
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

// ── Peer-Id header ────────────────────────────────────────────────────────────

test("peer-id header — present, valid base32, consistent", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (req) => {
      const peerId = req.headers.get("peer-id");
      return new Response(peerId || "", { status: 200 });
    });

    const fetchOpts = { directAddrs: serverAddrs };
    const r1 = await client.fetch(`httpi://${serverId}/1`, fetchOpts);
    const id1 = await r1.text();
    const r2 = await client.fetch(`httpi://${serverId}/2`, fetchOpts);
    const id2 = await r2.text();

    // Present and non-empty.
    assert.ok(id1.length >= 52, `peer-id too short: ${id1.length}`);
    // Valid base32: only a-z and 2-7.
    assert.match(id1, /^[a-z2-7]+$/, `peer-id should be base32: ${id1}`);
    // Consistent across requests.
    assert.equal(id1, id2, "peer-id should be consistent");
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
    const resp = await client.fetch(`httpi://${serverId}/upload`, {
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

// ── Stress: many concurrent requests ─────────────────────────────────────────

test("serve + fetch — 100 concurrent requests return correct bodies", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (req) => {
      const path = new URL(req.url).pathname;
      return new Response(`body:${path}`, { status: 200 });
    });

    const N = 100;
    const paths = Array.from({ length: N }, (_, i) => `/r${i}`);
    const texts = await Promise.all(
      paths.map((path) =>
        client
          .fetch(`httpi://${serverId}${path}`, { directAddrs: serverAddrs })
          .then((r) => r.text())
      ),
    );

    for (let i = 0; i < N; i++) {
      assert.equal(texts[i], `body:${paths[i]}`, `response ${i} body mismatch`);
    }
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

// ── Sessions (raw QUIC) ───────────────────────────────────────────────────────

test("session — connect() to live endpoint resolves with IrohSession", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (_req) => new Response("ok"));

    const session = await client.connect(serverId, {
      directAddrs: serverAddrs,
    });
    try {
      assert.equal(typeof session.createBidirectionalStream, "function");
      assert.equal(typeof session.createUnidirectionalStream, "function");
      assert.ok(session.incomingBidirectionalStreams instanceof ReadableStream);
      assert.ok(
        session.incomingUnidirectionalStreams instanceof ReadableStream,
      );
      assert.ok(
        session.datagrams !== null && typeof session.datagrams === "object",
      );
      assert.ok(session.closed instanceof Promise);
    } finally {
      session.close();
    }
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

test("session — createBidirectionalStream() returns readable+writable pair", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (_req) => new Response("ok"));

    const session = await client.connect(serverId, {
      directAddrs: serverAddrs,
    });
    try {
      const bidi = await session.createBidirectionalStream();
      assert.ok(
        bidi.readable instanceof ReadableStream,
        "readable must be ReadableStream",
      );
      assert.ok(
        bidi.writable instanceof WritableStream,
        "writable must be WritableStream",
      );
      // Close the bidi stream cleanly.
      await bidi.writable.close().catch(() => {});
    } finally {
      session.close();
    }
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

test("session — createUnidirectionalStream() returns WritableStream", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (_req) => new Response("ok"));

    const session = await client.connect(serverId, {
      directAddrs: serverAddrs,
    });
    try {
      const writable = await session.createUnidirectionalStream();
      assert.ok(writable instanceof WritableStream, "must be WritableStream");
      await writable.close().catch(() => {});
    } finally {
      session.close();
    }
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

test("session — datagrams.maxDatagramSize is null or a positive number", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (_req) => new Response("ok"));

    const session = await client.connect(serverId, {
      directAddrs: serverAddrs,
    });
    try {
      // Give time for the async maxDatagramSize fetch to settle.
      await new Promise((r) => setTimeout(r, 50));
      const size = session.datagrams.maxDatagramSize;
      assert.ok(
        size === null || (typeof size === "number" && size > 0),
        `maxDatagramSize must be null or positive, got ${size}`,
      );
    } finally {
      session.close();
    }
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

test("session — close() is safe to call multiple times", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (_req) => new Response("ok"));

    const session = await client.connect(serverId, {
      directAddrs: serverAddrs,
    });
    // First close.
    session.close({ closeCode: 0, reason: "done" });
    // Second close must not throw.
    try {
      session.close();
    } catch {
      // Allowed — some implementations may reject on double-close.
    }
    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

test(
  "sessions — yields IrohSession when peer calls node.connect()",
  { timeout: 20_000 },
  async () => {
    const server = await createNode({ bindAddr: "0.0.0.0:0" });
    const client = await createNode({ bindAddr: "0.0.0.0:0" });
    const ac = new AbortController();

    try {
      const { id: serverId, addrs: serverAddrs } = await server.addr();
      const { id: clientId } = await client.addr();

      // Accept the first incoming session in the background.
      const serverSessionPromise = (async () => {
        for await (const session of server.sessions({ signal: ac.signal })) {
          return session;
        }
        return null;
      })();

      const clientSession = await client.connect(serverId, {
        directAddrs: serverAddrs,
      });
      const serverSession = await serverSessionPromise;

      assert.ok(serverSession !== null, "server should have accepted a session");
      assert.equal(
        serverSession.remoteId.toString(),
        clientId,
        "server session remoteId must match client publicKey",
      );

      clientSession.close();
    } finally {
      ac.abort();
      await server.close();
      await client.close();
    }
  },
);

// ── EventTarget / transport events ───────────────────────────────────────────

test("IrohNode — extends EventTarget", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    assert.ok(
      node instanceof EventTarget,
      "IrohNode must be an instance of EventTarget",
    );
    assert.equal(typeof node.addEventListener, "function");
    assert.equal(typeof node.removeEventListener, "function");
    assert.equal(typeof node.dispatchEvent, "function");
  } finally {
    await node.close();
  }
});

test("diagnostics — pool:miss event fires on first fetch", async () => {
  const server = await createNode();
  const client = await createNode();
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (_req) => new Response("ok"));

    const received = [];
    client.addEventListener("diagnostics", (ev) => {
      received.push(ev.detail);
    });

    // First fetch → pool miss (no cached connection yet).
    await client.fetch(`httpi://${serverId}/`, {
      directAddrs: serverAddrs,
    });

    // The diagnostics event loop runs concurrently; give it a turn to flush.
    await new Promise((r) => setImmediate(r));
    await new Promise((r) => setImmediate(r));

    const miss = received.find((e) => e.kind === "pool:miss");
    assert.ok(
      miss,
      `Expected a pool:miss event, got: ${JSON.stringify(received)}`,
    );
    assert.equal(typeof miss.peerId, "string", "pool:miss must have peerId");
    assert.equal(
      typeof miss.timestamp,
      "number",
      "pool:miss must have timestamp",
    );

    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

test("diagnostics — emitted by default (no opt-in required)", async () => {
  const server = await createNode();
  const client = await createNode(); // no special options — events fire by default
  try {
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const ac = new AbortController();
    server.serve({ signal: ac.signal }, (_req) => new Response("ok"));

    const received = [];
    client.addEventListener("diagnostics", (ev) => {
      received.push(ev.detail);
    });

    await client.fetch(`httpi://${serverId}/`, {
      directAddrs: serverAddrs,
    });
    await new Promise((r) => setImmediate(r));
    await new Promise((r) => setImmediate(r));

    // Diagnostics events fire unconditionally — at least one should be present.
    assert.ok(
      received.length > 0,
      `Expected at least one diagnostics event, got none`,
    );

    ac.abort();
  } finally {
    await server.close();
    await client.close();
  }
});

// ── browse / advertise ───────────────────────────────────────────────────────

test("browse — returns an AsyncIterable", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    const iterable = node.browse();
    assert.equal(
      typeof iterable[Symbol.asyncIterator],
      "function",
      "browse() must return an AsyncIterable",
    );
  } finally {
    await node.close();
  }
});

test(
  "advertise — resolves when signal is aborted",
  { timeout: 10_000 },
  async () => {
    const node = await createNode();
    try {
      const ac = new AbortController();
      const p = node.advertise({ signal: ac.signal });
      // Abort immediately — the Promise should resolve.
      ac.abort();
      await p;
    } finally {
      await node.close();
    }
  },
);

test(
  "browse + advertise — discovers peer via mDNS",
  { timeout: 20_000, skip: "mDNS discovery requires multicast UDP; unreliable in CI/sandbox environments" },
  async () => {
    // Use a unique service name to avoid picking up stale advertisements from
    // other test runs on the same machine.
    const svcName = `iroh-http-test-${Date.now()}`;
    const advertiser = await createNode();
    const browser = await createNode();
    const ac = new AbortController();
    // Guard: fire ac.abort() before the 20 s test timeout so the browse loop
    // unblocks (mdnsNextEvent is blocked in Rust; the signal races it via
    // Promise.race) and the finally block can close both nodes cleanly.
    // Without this, node:test's timeout fires while the async fn is still
    // running, leaving dangling QUIC sockets that prevent process exit.
    const guard = setTimeout(() => ac.abort(), 14_000);
    try {
      // Start advertising; resolves when we abort.
      const advDone = advertiser.advertise({
        serviceName: svcName,
        signal: ac.signal,
      });

      // Browse until we see the advertiser or the guard aborts us.
      let found = null;
      for await (
        const peer of browser.browse({
          serviceName: svcName,
          signal: ac.signal,
        })
      ) {
        if (peer.nodeId === advertiser.publicKey.toString()) {
          found = peer;
          break;
        }
      }

      assert.ok(found !== null, "browse() must discover the advertising peer");
      assert.equal(
        found.nodeId,
        advertiser.publicKey.toString(),
        "discovered nodeId must match the advertiser's publicKey",
      );

      await advDone;
    } finally {
      clearTimeout(guard);
      ac.abort(); // no-op if guard already fired
      await advertiser.close();
      await browser.close();
    }
  },
);

// ── pathChanges ───────────────────────────────────────────────────────────────

test("pathChanges — returns an AsyncIterable", async () => {
  const node = await createNode({ disableNetworking: true });
  try {
    const iterable = node.pathChanges(node.publicKey);
    assert.equal(
      typeof iterable[Symbol.asyncIterator],
      "function",
      "pathChanges() must return an AsyncIterable",
    );
  } finally {
    await node.close();
  }
});

// ── Regression #119: fire-and-forget pipes / stale microtasks ─────────────────
//
// Before the fix:
//   1. doPipe() in makeServe was detached — finished resolved before bodies drained.
//   2. Node napi callback tasks were untracked — stale callbacks from a previous
//      iteration called rawRespond() on handles recycled in the current iteration.
//   3. Timed TTL was creation-time only — slow pipes got swept mid-transfer.
//
// This test mirrors the "multiplexing iroh 32 streams" bench pattern that
// reliably triggered "unknown handle" / "sendChunk failed" errors: multiple
// iterations of 32 concurrent fetches followed immediately by stopServe +
// await finished.  Any stale microtask firing on a recycled handle will be
// captured by the console.error spy and cause the assertion to fail.

test(
  "regression #119 — 32-stream burst × 5 iterations: no stale-handle errors after finished",
  { timeout: 120_000 },
  async () => {
    const STREAMS = 32;
    const ITERS = 5;
    const BODY = "x".repeat(4096); // 4 KiB ensures the pipe spans multiple chunks

    const errors = [];
    const originalError = console.error.bind(console);
    console.error = (...args) => {
      const msg = args.map(String).join(" ");
      if (
        msg.includes("unknown handle") ||
        msg.includes("node closed or not found") ||
        msg.includes("sendChunk failed")
      ) {
        errors.push(msg);
      }
      originalError(...args);
    };

    const server = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
    const client = await createNode({ disableNetworking: true, bindAddr: "127.0.0.1:0" });
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    try {
      for (let iter = 0; iter < ITERS; iter++) {
        const ac = new AbortController();
        const handle = server.serve({ signal: ac.signal, loadShed: false }, () =>
          new Response(BODY),
        );

        // 32 concurrent fetches — mirrors "multiplexing iroh 32 streams" bench.
        const responses = await Promise.all(
          Array.from({ length: STREAMS }, () =>
            client
              .fetch(`httpi://${serverId}/data`, {
                directAddrs: serverAddrs,
              })
              .then(async (r) => ({ status: r.status, body: await r.text() })),
          ),
        );

        // Stop the loop and wait for ALL body pipes to drain before proceeding.
        ac.abort();
        await handle.finished;

        // Every response must be 200 with the full body.
        for (let i = 0; i < STREAMS; i++) {
          assert.equal(responses[i].status, 200, `iter ${iter} stream ${i}: expected 200, got ${responses[i].status}`);
          assert.equal(responses[i].body, BODY, `iter ${iter} stream ${i}: body truncated`);
        }
      }

      // Any stale-handle log line is a test failure.
      assert.deepEqual(errors, [], `handle errors detected: ${errors.join(" | ")}`);
    } finally {
      console.error = originalError;
      await server.close();
      await client.close();
    }
  },
);
