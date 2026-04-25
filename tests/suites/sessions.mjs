/**
 * Session tests — node.connect(), node.sessions(), IrohSession properties,
 * bidirectional streams, unidirectional streams, datagrams.
 *
 * Shared across all runtimes.
 */

// Helper: start accepting one session in the background, return a promise
// that resolves with the accepted server-side session.
function acceptOne(server) {
  const ac = new AbortController();
  const promise = (async () => {
    for await (const session of server.sessions({ signal: ac.signal })) {
      ac.abort();
      return session;
    }
    return null;
  })();
  return { promise, abort: () => ac.abort() };
}

export function sessionTests({ createNode, test, assert, assertEqual }) {
  // ── connect basics ─────────────────────────────────────────────────────────

  test("connect() returns an IrohSession", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const accept = acceptOne(server);
    const session = await client.connect(serverId, { directAddrs: serverAddrs });
    await accept.promise;

    assert(session != null, "session is null");
    assert(typeof session.close === "function", "session.close is not a function");
    assert(session.closed instanceof Promise, "session.closed is not a Promise");

    session.close();
    await server.close();
    await client.close();
  });

  test("session.remoteId matches server publicKey", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const accept = acceptOne(server);
    const session = await client.connect(serverId, { directAddrs: serverAddrs });
    await accept.promise;

    assert(session.remoteId != null, "remoteId is null");
    assertEqual(
      session.remoteId.toString(),
      server.publicKey.toString(),
      "remoteId matches server",
    );

    session.close();
    await server.close();
    await client.close();
  });

  test("session.ready resolves", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const accept = acceptOne(server);
    const session = await client.connect(serverId, { directAddrs: serverAddrs });
    await accept.promise;

    await session.ready;

    session.close();
    await server.close();
    await client.close();
  });

  // ── session.close() ────────────────────────────────────────────────────────

  test("session.close() is safe to call twice", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const accept = acceptOne(server);
    const session = await client.connect(serverId, { directAddrs: serverAddrs });
    await accept.promise;

    session.close();
    try {
      session.close();
    } catch {
      // acceptable
    }

    await server.close();
    await client.close();
  });

  test("session.closed resolves after close", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const accept = acceptOne(server);
    const session = await client.connect(serverId, { directAddrs: serverAddrs });
    await accept.promise;

    session.close({ closeCode: 0, reason: "test" });
    const info = await session.closed;
    assert(info != null, "closed resolved with null");

    await server.close();
    await client.close();
  });

  // ── sessions() accept ──────────────────────────────────────────────────────

  test("sessions() yields incoming session with correct remoteId", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();
    const { id: clientId } = await client.addr();

    const accept = acceptOne(server);
    const clientSession = await client.connect(serverId, { directAddrs: serverAddrs });
    const serverSession = await accept.promise;

    assert(serverSession != null, "server should have accepted a session");
    assertEqual(
      serverSession.remoteId.toString(),
      clientId,
      "server session remoteId matches client publicKey",
    );

    clientSession.close();
    await server.close();
    await client.close();
  });

  // ── Bidirectional streams ──────────────────────────────────────────────────

  test("createBidirectionalStream returns readable + writable", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const accept = acceptOne(server);
    const session = await client.connect(serverId, { directAddrs: serverAddrs });
    await accept.promise;

    const bidi = await session.createBidirectionalStream();
    assert(bidi != null, "bidi stream is null");
    assert(bidi.readable instanceof ReadableStream, "no readable");
    assert(bidi.writable instanceof WritableStream, "no writable");

    try { await bidi.writable.close(); } catch {}
    try { await bidi.readable.cancel(); } catch {}
    session.close();
    await server.close();
    await client.close();
  });

  // ── Unidirectional streams ─────────────────────────────────────────────────

  test("createUnidirectionalStream returns a WritableStream", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const accept = acceptOne(server);
    const session = await client.connect(serverId, { directAddrs: serverAddrs });
    await accept.promise;

    const writable = await session.createUnidirectionalStream();
    assert(writable instanceof WritableStream, "not a WritableStream");

    try { await writable.close(); } catch {}
    session.close();
    await server.close();
    await client.close();
  });

  // ── Datagrams ──────────────────────────────────────────────────────────────

  test("session.datagrams has expected shape", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const accept = acceptOne(server);
    const session = await client.connect(serverId, { directAddrs: serverAddrs });
    await accept.promise;

    const dg = session.datagrams;
    assert(dg != null, "datagrams is null");
    assert(dg.readable instanceof ReadableStream, "datagrams.readable missing");
    assert(dg.writable instanceof WritableStream, "datagrams.writable missing");
    assert(
      dg.maxDatagramSize === null || dg.maxDatagramSize > 0,
      `unexpected maxDatagramSize: ${dg.maxDatagramSize}`,
    );

    session.close();
    await server.close();
    await client.close();
  });

  // ── Bidi stream data round-trip ────────────────────────────────────────────

  test("bidirectional stream echo round-trip", async () => {
    const server = await createNode();
    const client = await createNode();
    const { id: serverId, addrs: serverAddrs } = await server.addr();

    const ac = new AbortController();

    // Server: accept session, read from bidi stream, echo back
    const serverLoop = (async () => {
      for await (const session of server.sessions({ signal: ac.signal })) {
        const reader = session.incomingBidirectionalStreams.getReader();
        const { value: bidi } = await reader.read();
        if (!bidi) return;

        const streamReader = bidi.readable.getReader();
        const writer = bidi.writable.getWriter();

        const { value: chunk } = await streamReader.read();
        if (chunk) {
          await writer.write(chunk); // echo
        }
        await writer.close();
        reader.releaseLock();
        ac.abort();
      }
    })();

    // Client: connect, open bidi, send data, read echo
    const session = await client.connect(serverId, { directAddrs: serverAddrs });
    const bidi = await session.createBidirectionalStream();

    const writer = bidi.writable.getWriter();
    const payload = new TextEncoder().encode("hello-bidi");
    await writer.write(payload);
    await writer.close();

    const reader = bidi.readable.getReader();
    const { value: echo } = await reader.read();
    assert(echo != null, "no echo data");
    const echoText = new TextDecoder().decode(echo);
    assertEqual(echoText, "hello-bidi", "echo content");

    await serverLoop.catch(() => {});
    session.close();
    await server.close();
    await client.close();
  });
}
