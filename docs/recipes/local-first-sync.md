# Local-First Sync

Two devices on the same LAN exchange changes directly, without a server.
When the cloud is available they can use it too — but the cloud is optional,
not required. The P2P path is always the fast path.

## The insight

Conventional sync services route data through a cloud server even when two
devices are sitting next to each other on the same WiFi. iroh-http's mDNS
discovery lets `laptop-a` and `laptop-b` find each other and sync at LAN
speed, with no cloud account, no internet connection, and no configuration.

```
laptop-a ──── mDNS ────► laptop-b
    │                         │
    └── iroh QUIC direct ─────┘
         (no server)
```

If one device is remote, the relay handles the connection transparently.
The application code doesn't change.

## Data model

Use a simple document store keyed by a logical ID and a monotonic version
counter. Any two peers can merge by taking the higher version.

```ts
interface Doc {
  id: string;
  version: number;   // monotonically increasing
  updatedAt: number; // Unix ms — for human display only
  body: unknown;
}
```

## Server side (both devices run the same code)

```ts
import { IrohNode } from 'iroh-http';

const store = new Map<string, Doc>();

async function startSyncServer(node: IrohNode) {
  node.serve({}, async (req) => {
    const url = new URL(req.url);

    // GET /doc/:id — return current version
    if (req.method === 'GET' && url.pathname.startsWith('/doc/')) {
      const id = url.pathname.slice(5);
      const doc = store.get(id);
      if (!doc) return new Response('Not Found', { status: 404 });
      return Response.json(doc);
    }

    // PUT /doc/:id — accept if version is higher
    if (req.method === 'PUT' && url.pathname.startsWith('/doc/')) {
      const id = url.pathname.slice(5);
      const incoming: Doc = await req.json();
      const current = store.get(id);
      if (!current || incoming.version > current.version) {
        store.set(id, incoming);
        return new Response(null, { status: 204 });
      }
      // Our version is newer — return it so the caller can update
      return Response.json(current, { status: 409 });
    }

    // GET /manifest — full list of (id, version) pairs for bulk comparison
    if (req.method === 'GET' && url.pathname === '/manifest') {
      const manifest = [...store.values()].map(({ id, version }) => ({ id, version }));
      return Response.json(manifest);
    }

    return new Response('Not Found', { status: 404 });
  });
}
```

## Sync protocol

```ts
async function syncWith(node: IrohNode, peer: string) {
  // 1. Fetch their manifest
  const res = await node.fetch(`iroh://${peer}/manifest`);
  const theirManifest: { id: string; version: number }[] = await res.json();

  for (const { id, version: theirVersion } of theirManifest) {
    const ours = store.get(id);

    if (!ours || theirVersion > ours.version) {
      // They have a newer version — pull it
      const docRes = await node.fetch(`iroh://${peer}/doc/${id}`);
      const doc: Doc = await docRes.json();
      store.set(id, doc);
    } else if (ours.version > theirVersion) {
      // We have a newer version — push it
      await node.fetch(`iroh://${peer}/doc/${id}`, {
        method: 'PUT',
        body: JSON.stringify(ours),
        headers: { 'Content-Type': 'application/json' },
      });
    }
    // Equal versions: nothing to do
  }

  // Push any docs they don't have at all
  for (const [id, doc] of store) {
    if (!theirManifest.find((m) => m.id === id)) {
      await node.fetch(`iroh://${peer}/doc/${id}`, {
        method: 'PUT',
        body: JSON.stringify(doc),
        headers: { 'Content-Type': 'application/json' },
      });
    }
  }
}
```

## Discovery loop

```ts
async function startDiscoverySync(node: IrohNode, signal: AbortSignal) {
  const seen = new Set<string>();

  for await (const event of node.browse({ signal })) {
    if (seen.has(event.nodeId)) continue;
    seen.add(event.nodeId);

    // Fire-and-forget; errors are expected (peer went away)
    syncWith(node, event.nodeId).catch(() => {});
  }
}
```

## Write path

When the local user edits a document, bump the version and trigger a
background sync:

```ts
function writeDoc(doc: Omit<Doc, 'version' | 'updatedAt'>): Doc {
  const existing = store.get(doc.id);
  const updated: Doc = {
    ...doc,
    version: (existing?.version ?? 0) + 1,
    updatedAt: Date.now(),
  };
  store.set(doc.id, updated);
  return updated;
}
```

## Running

```ts
const node = await IrohNode.spawn({ port: 8080 });
const ac = new AbortController();

await node.advertise({ signal: ac.signal });
await startSyncServer(node);
await startDiscoverySync(node, ac.signal);
```

## What this does not solve

- **Conflict resolution for concurrent edits to the same document.** This
  recipe uses last-write-wins by version number. For richer merging, use a
  CRDT library (Automerge, Yjs) for the `body` field and keep the version
  counter as the sync layer only. See [offline-first.md](offline-first.md)
  for the queue-and-replay approach.
- **Large binary blobs.** For files, replace the JSON body with a streaming
  `GET` and chunked `PUT`. The sync manifest can hold a content hash instead
  of a version number. See [cooperative-backup.md](cooperative-backup.md) for
  the multi-peer blob pattern.
- **Access control between strangers.** This recipe trusts any peer that
  discovers you via mDNS. For mixed LAN/relay environments with unknown
  peers, add [proximity trust](proximity-trust.md) or
  [capability tokens](capability-tokens.md).
