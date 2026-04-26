/**
 * Event tests — EventTarget, peerconnect, peerdisconnect, pathchange,
 * diagnostics events.
 *
 * Shared across all runtimes.
 */

export function eventTests({ createNode, test, assert, assertEqual }) {
  // ── EventTarget basics ─────────────────────────────────────────────────────

  test("node is an EventTarget", async () => {
    const node = await createNode();
    assert(node instanceof EventTarget, "node is not an EventTarget");
    assert(typeof node.addEventListener === "function", "no addEventListener");
    assert(typeof node.removeEventListener === "function", "no removeEventListener");
    assert(typeof node.dispatchEvent === "function", "no dispatchEvent");
    await node.close();
  });

  test("addEventListener / removeEventListener work", async () => {
    const node = await createNode();
    let called = false;
    const handler = () => { called = true; };

    node.addEventListener("diagnostics", handler);
    node.removeEventListener("diagnostics", handler);

    node.dispatchEvent(new CustomEvent("diagnostics", { detail: {} }));
    assert(!called, "handler should not be called after removal");

    await node.close();
  });

  // ── peerconnect ────────────────────────────────────────────────────────────

  test("peerconnect fires when a peer connects", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const connectEvents = [];
    server.addEventListener("peerconnect", (ev) => {
      connectEvents.push(ev.detail);
    });

    server.serve({}, () => new Response("hello"));
    const res = await client.fetch(`httpi://${serverId}/`, { directAddrs: serverAddrs });
    await res.text();

    await new Promise((r) => setTimeout(r, 200));

    assert(connectEvents.length >= 1, `expected peerconnect, got ${connectEvents.length} events`);
    assert(typeof connectEvents[0].nodeId === "string", "nodeId should be string");
    assertEqual(connectEvents[0].nodeId, client.publicKey.toString(), "nodeId matches client");

    await server.close();
    await client.close();
  });

  // ── peerdisconnect ─────────────────────────────────────────────────────────

  test("peerdisconnect fires after peer disconnects", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const disconnectEvents = [];
    server.addEventListener("peerdisconnect", (ev) => {
      disconnectEvents.push(ev.detail);
    });

    server.serve({}, () => new Response("hello"));
    const res = await client.fetch(`httpi://${serverId}/`, { directAddrs: serverAddrs });
    await res.text();

    await client.close();
    await new Promise((r) => setTimeout(r, 2000));

    assert(disconnectEvents.length >= 1, `expected peerdisconnect, got ${disconnectEvents.length} events`);
    assert(typeof disconnectEvents[0].nodeId === "string", "nodeId should be string");

    await server.close();
  });

  // ── diagnostics ────────────────────────────────────────────────────────────

  test("diagnostics events fire on fetch (pool:miss)", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const diagEvents = [];
    client.addEventListener("diagnostics", (ev) => {
      diagEvents.push(ev.detail);
    });

    server.serve({}, () => new Response("ok"));
    const res = await client.fetch(`httpi://${serverId}/`, { directAddrs: serverAddrs });
    await res.text();

    await new Promise((r) => setTimeout(r, 500));

    assert(diagEvents.length >= 1, `expected diagnostics events, got ${diagEvents.length}`);

    const poolMiss = diagEvents.find((d) => d.kind === "pool:miss");
    if (poolMiss) {
      assert(typeof poolMiss.peerId === "string", "pool:miss should have peerId");
      assert(typeof poolMiss.timestamp === "number", "pool:miss should have timestamp");
    }
    assert(typeof diagEvents[0].kind === "string", "diagnostics event should have kind");

    await server.close();
    await client.close();
  });

  test("diagnostics events fire without opt-in", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const diagEvents = [];
    client.addEventListener("diagnostics", (ev) => {
      diagEvents.push(ev.detail);
    });

    server.serve({}, () => new Response("ok"));
    const res = await client.fetch(`httpi://${serverId}/`, { directAddrs: serverAddrs });
    await res.text();

    await new Promise((r) => setTimeout(r, 500));

    assert(diagEvents.length >= 1, "diagnostics events should fire without opt-in");

    await server.close();
    await client.close();
  });

  // ── pathchange ─────────────────────────────────────────────────────────────

  test("pathchange event has expected shape when fired", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const pathEvents = [];
    client.addEventListener("pathchange", (ev) => {
      pathEvents.push(ev.detail);
    });

    server.serve({}, () => new Response("ok"));
    const res = await client.fetch(`httpi://${serverId}/`, { directAddrs: serverAddrs });
    await res.text();

    // pathchange may or may not fire depending on network conditions
    await new Promise((r) => setTimeout(r, 2000));

    if (pathEvents.length > 0) {
      const ev = pathEvents[0];
      assert(typeof ev.nodeId === "string", "pathchange.nodeId should be string");
      assert(typeof ev.relay === "boolean", "pathchange.relay should be boolean");
      assert(typeof ev.addr === "string", "pathchange.addr should be string");
      assert(typeof ev.timestamp === "number", "pathchange.timestamp should be number");
    }
    // Not asserting pathEvents.length > 0 — network-dependent

    await server.close();
    await client.close();
  });
}
