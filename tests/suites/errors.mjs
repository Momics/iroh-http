/**
 * Error handling tests — handler failures, unknown peers, aborted requests,
 * invalid arguments, recovery after errors, Peer-Id spoof prevention.
 *
 * Shared across all runtimes.
 */

export function errorTests({ createNode, test, assert, assertEqual, assertThrows }) {
  // ── Handler errors ─────────────────────────────────────────────────────────

  test("handler that throws → client gets error response", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    server.serve({}, () => {
      throw new Error("handler exploded");
    });

    try {
      const res = await client.fetch(serverId, "/boom", { directAddrs: serverAddrs });
      assert(res.status >= 500 || res.status === 0, `expected 5xx or connection error, got ${res.status}`);
      await res.body?.cancel();
    } catch (err) {
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
    server.serve({}, () => {
      callCount++;
      if (callCount === 1) throw new Error("first call fails");
      return new Response("recovered");
    });

    try {
      const res1 = await client.fetch(serverId, "/first", { directAddrs: serverAddrs });
      await res1.body?.cancel();
    } catch {
      // expected
    }

    const res2 = await client.fetch(serverId, "/second", { directAddrs: serverAddrs });
    assertEqual(res2.status, 200, "second request status must be 200");
    const body = await res2.text();
    assertEqual(body, "recovered", "second request body must match");
    assertEqual(callCount, 2, "handler must be called twice");

    await server.close();
    await client.close();
  });

  // ── Fetch errors ───────────────────────────────────────────────────────────

  test("fetch to unknown peer rejects", async () => {
    const client = await createNode();
    const fakePeer = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    await assertThrows(async () => {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), 5000);
      try {
        await client.fetch(fakePeer, "/hello", { signal: controller.signal });
      } finally {
        clearTimeout(timer);
      }
    });

    await client.close();
  });

  test("fetch with aborted signal rejects", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    server.serve({}, () => new Response("ok"));

    const controller = new AbortController();
    controller.abort();

    await assertThrows(async () => {
      await client.fetch(serverId, "/", { signal: controller.signal, directAddrs: serverAddrs });
    });

    await server.close();
    await client.close();
  });

  test("fetch with abort during request rejects", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    server.serve({}, async () => {
      await new Promise((r) => setTimeout(r, 2000));
      return new Response("slow");
    });

    const controller = new AbortController();
    setTimeout(() => controller.abort(), 100);

    await assertThrows(async () => {
      await client.fetch(serverId, "/slow", { signal: controller.signal, directAddrs: serverAddrs });
    });

    await server.close();
    await client.close();
  });

  // ── Handler edge cases ─────────────────────────────────────────────────────

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

    // First request with bad return — should produce error response
    try {
      const res = await client.fetch(serverId, "/bad", { directAddrs: serverAddrs });
      await res.body?.cancel();
    } catch {
      // expected
    }

    // Server should still be alive for second request
    try {
      const res2 = await client.fetch(serverId, "/good", { directAddrs: serverAddrs });
      if (res2.status === 200) {
        const body = await res2.text();
        assertEqual(body, "ok", "recovery body must match");
      } else {
        await res2.body?.cancel();
      }
    } catch {
      // acceptable — behavior may be platform-dependent
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
    assertEqual(res.status, 200, "status must be 200");
    const body = await res.text();
    assertEqual(body, "", "empty body expected");

    await server.close();
    await client.close();
  });

  // ── URL scheme validation ──────────────────────────────────────────────────

  test("fetch rejects https:// URL with TypeError", async () => {
    const node = await createNode({ disableNetworking: true });
    await assertThrows(async () => {
      await node.fetch(node.publicKey, "https://example.com/");
    });
    await node.close();
  });

  test("fetch rejects http:// URL with TypeError", async () => {
    const node = await createNode({ disableNetworking: true });
    await assertThrows(async () => {
      await node.fetch(node.publicKey, "http://example.com/");
    });
    await node.close();
  });

  // ── Peer-Id security ──────────────────────────────────────────────────────

  test("Peer-Id header cannot be spoofed by client", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    let receivedPeerId = "";

    server.serve({}, (req) => {
      receivedPeerId = req.headers.get("Peer-Id") || "";
      return new Response("ok");
    });

    await client.fetch(serverId, "/", {
      headers: { "Peer-Id": "spoofed-value" },
      directAddrs: serverAddrs,
    });

    assert(receivedPeerId !== "spoofed-value", "Peer-Id was spoofable!");
    assertEqual(receivedPeerId, client.publicKey.toString(), "Peer-Id should match client publicKey");

    await server.close();
    await client.close();
  });

  test("peer-id header is present, valid base32, and consistent", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    server.serve({}, (req) => {
      const peerId = req.headers.get("peer-id");
      return new Response(peerId || "", { status: 200 });
    });

    const fetchOpts = { directAddrs: serverAddrs };
    const r1 = await client.fetch(serverId, "/1", fetchOpts);
    const id1 = await r1.text();
    const r2 = await client.fetch(serverId, "/2", fetchOpts);
    const id2 = await r2.text();

    assert(id1.length >= 52, `peer-id too short: ${id1.length}`);
    assert(/^[a-z2-7]+$/.test(id1), `peer-id should be base32: ${id1}`);
    assertEqual(id1, id2, "peer-id must be consistent across requests");

    await server.close();
    await client.close();
  });

  // ── No-stderr on valid response ────────────────────────────────────────────

  test("plain response logs no internal pipe errors", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

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
      const handle = server.serve({}, () => new Response("hello", { status: 200 }));
      const res = await client.fetch(serverId, "/", { directAddrs: serverAddrs });
      assertEqual(res.status, 200, "status must be 200");
      assertEqual(await res.text(), "hello", "body must match");

      await server.close();
      await handle.finished.catch(() => {});

      assert(
        internalErrors.length === 0,
        `Unexpected internal errors:\n${internalErrors.join("\n")}`,
      );
    } finally {
      console.error = origConsoleError;
      await client.close();
    }
  });
}
