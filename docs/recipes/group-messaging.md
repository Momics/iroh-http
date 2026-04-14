# Group Messaging

Fan-out messages to a set of named peers without a broker. Each peer
maintains a small list of group members and sends directly.

## Core abstraction

```ts
interface GroupMessage {
  from: string;    // nodeId hex
  body: Uint8Array;
}

interface Group {
  send(body: Uint8Array): Promise<void>;
  messages(): AsyncIterable<GroupMessage>;
  close(): void;
}
```

## Implementation sketch

```ts
function joinGroup(node: IrohNode, opts: {
  name: string;            // identifies the group by convention
  members: string[];       // nodeId hex strings (known peers)
  port?: number;           // iroh-http port on each peer, defaults to 8080
}): Group {
  const path = `/__group/${encodeURIComponent(opts.name)}`;
  const port = opts.port ?? 8080;
  const inbound = new TransformStream<GroupMessage>();
  const writer = inbound.writable.getWriter();

  // Serve incoming messages
  const ac = new AbortController();
  node.serve({ signal: ac.signal }, async (req) => {
    if (new URL(req.url).pathname !== path) {
      return new Response('Not Found', { status: 404 });
    }
    const from = req.headers.get('Peer-Id') ?? 'unknown';
    const body = new Uint8Array(await req.arrayBuffer());
    writer.write({ from, body });
    return new Response(null, { status: 204 });
  });

  return {
    async send(body) {
      await Promise.allSettled(
        opts.members.map((peer) =>
          node.fetch(`iroh://${peer}:${port}${path}`, {
            method: 'POST',
            body,
          }),
        ),
      );
    },

    messages() {
      return inbound.readable[Symbol.asyncIterator]();
    },

    close() {
      ac.abort();
    },
  };
}
```

## Usage

```ts
const group = joinGroup(node, {
  name: 'team-updates',
  members: [aliceNodeId, bobNodeId],
});

// Listen
for await (const msg of group.messages()) {
  console.log('→', hexToString(msg.body));
}

// Send
await group.send(new TextEncoder().encode('Hello everyone'));

group.close();
```

## Member discovery

When members are not known ahead of time, use `node.browse()` with a shared
DNS suffix or mDNS to discover them, then add them to the group:

```ts
for await (const event of node.browse({ signal })) {
  if (event.name.endsWith('._team-updates._iroh')) {
    members.push(event.nodeId);
  }
}
```

See [discovery.md](../features/discovery.md).

## Delivery semantics

- Each `send()` is best-effort: `Promise.allSettled` never throws.
- Delivery order to different peers may differ.
- No persistence — messages not received while a peer is offline are lost.

For durable or ordered messaging, a peer that is always online can act as a
sequencer: send to it, and clients fetch from it in order.

## Notes

- Membership is managed out-of-band here. A more complete system
  (`iroh-http-group`) would solve bootstrapping, membership updates, and
  persistence. See [group-messaging feature](../features/packages/group-messaging.md).
- Because iroh authenticates every connection, you know exactly who `from` is.
  No spoofing.
- For high fan-out (hundreds of peers), batch with `Promise.all` in chunks to
  avoid overwhelming the local connection pool.

## See also

- [Offline-first](offline-first.md) — for messages that must survive peers
  going offline; the outbox pattern applies directly to message fan-out
- [Local-first sync](local-first-sync.md) — group messaging where the
  "message" is a document delta; mDNS discovery replaces the static member list
- [Proximity trust](proximity-trust.md) — treat LAN-discovered group members
  with elevated trust, relayed members with lower trust
