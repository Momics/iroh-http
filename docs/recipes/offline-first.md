# Offline-First with Peer Sync

Buffer writes locally while peers are unreachable. When they reappear — via
mDNS, a relay reconnect, or a manual retry — replay the queue. Merge
conflicts with a strategy that fits your data.

## The insight

In a P2P network, "offline" is relative. Your laptop might be offline from
the internet but still reachable to your phone via mDNS. A Raspberry Pi might
be online but a specific peer is just busy. The write queue is how you turn
"eventually" into a property of the data layer, not something the user has to
think about.

```
User writes                      Peer reappears
    │                                  │
    ▼                                  ▼
  Local store ──► outbox queue ──► replay loop
    │                                  │
    ▼                                  ▼
  UI reads                      remote store synced
  immediately
```

## Queue structure

```ts
interface QueuedWrite {
  id: string;          // unique op ID (for deduplication)
  timestamp: number;   // Unix ms — used for LWW resolution
  path: string;        // e.g. "/doc/note-1"
  method: 'PUT' | 'PATCH' | 'DELETE';
  body?: string;       // JSON-stringified
  headers?: Record<string, string>;
  attempts: number;
}
```

## Outbox

```ts
class Outbox {
  private queue: QueuedWrite[] = [];

  enqueue(op: Omit<QueuedWrite, 'id' | 'attempts'>): void {
    this.queue.push({ ...op, id: crypto.randomUUID(), attempts: 0 });
    this.persist();
  }

  // Called once per flush cycle — see replay loop below
  drain(): QueuedWrite[] {
    return [...this.queue];
  }

  ack(id: string): void {
    this.queue = this.queue.filter((op) => op.id !== id);
    this.persist();
  }

  private persist(): void {
    // Persist to localStorage / SQLite / disk as appropriate for your platform
    localStorage.setItem('iroh_outbox', JSON.stringify(this.queue));
  }

  static load(): Outbox {
    const box = new Outbox();
    const raw = localStorage.getItem('iroh_outbox');
    if (raw) box.queue = JSON.parse(raw);
    return box;
  }
}
```

## Write path — always write locally first

```ts
const outbox = Outbox.load();

function writeLocally(doc: Doc): void {
  store.set(doc.id, doc);

  // Queue the remote write — will be replayed when peer is available
  outbox.enqueue({
    timestamp: Date.now(),
    path: `/doc/${doc.id}`,
    method: 'PUT',
    body: JSON.stringify(doc),
    headers: { 'Content-Type': 'application/json' },
  });
}
```

The UI reads from `store` immediately and is always responsive, regardless of
peer availability.

## Replay loop

```ts
async function replayOutbox(
  node: IrohNode,
  peers: string[],
  signal: AbortSignal,
): Promise<void> {
  while (!signal.aborted) {
    for (const op of outbox.drain()) {
      let replayed = false;

      for (const peer of peers) {
        try {
          const res = await node.fetch(`iroh://${peer}${op.path}`, {
            method: op.method,
            body: op.body,
            headers: {
              ...op.headers,
              // Send original write timestamp so the peer can merge correctly
              'x-write-timestamp': String(op.timestamp),
              // Idempotency key — peer deduplicates if we retry
              'idempotency-key': op.id,
            },
            signal,
          });

          if (res.ok || res.status === 409) {
            // 409 = conflict resolved by peer; still ack locally
            replayed = true;
            break;
          }
        } catch {
          // Peer offline — try next
        }
      }

      if (replayed) {
        outbox.ack(op.id);
      } else {
        op.attempts += 1;
      }
    }

    // Wait before next flush — exponential backoff capped at 30 s
    const backoff = Math.min(1000 * 2 ** Math.min(outbox.drain()[0]?.attempts ?? 0, 5), 30_000);
    await sleep(backoff, signal);
  }
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const t = setTimeout(resolve, ms);
    signal.addEventListener('abort', () => { clearTimeout(t); reject(signal.reason); }, { once: true });
  });
}
```

## Conflict resolution on the server

The peer receiving a replayed write sees the `x-write-timestamp` header and
can apply last-write-wins:

```ts
if (req.method === 'PUT') {
  const incoming: Doc = await req.json();
  const ts = Number(req.headers.get('x-write-timestamp') ?? Date.now());
  const current = store.get(incoming.id);

  if (!current || ts > (current as any)._ts) {
    store.set(incoming.id, { ...incoming, _ts: ts });
    return new Response(null, { status: 204 });
  }

  // Our version is newer — 409 so the caller stops retrying this op
  return Response.json(current, { status: 409 });
}
```

## Trigger replay on peer discovery

Instead of polling, restart the replay immediately when a peer reappears:

```ts
for await (const event of node.browse({ signal })) {
  if (event.type === 'found') {
    // Peer came online — flush immediately
    replayOutbox(node, [event.nodeId], signal).catch(() => {});
  }
}
```

## Merging with CRDTs

For collaborative documents where two users might edit the same field
concurrently, replace the simple `version` integer with a CRDT value (Yjs,
Automerge). The outbox still works the same way — you're just sending a CRDT
delta instead of a full document body.

```ts
import * as Y from 'yjs';

// Encode a Yjs update as the op body
outbox.enqueue({
  path: `/doc/${docId}/crdt`,
  method: 'PATCH',
  body: Buffer.from(Y.encodeStateAsUpdate(ydoc)).toString('base64'),
  headers: { 'Content-Type': 'application/yjs-update' },
  timestamp: Date.now(),
});
```

On the peer, merge with `Y.applyUpdate(ydoc, update)`. Operations are
commutative and idempotent — replay order does not matter.

## See also

- [Local-first sync](local-first-sync.md) — the same store model without the
  queue; suitable when peers are usually available
- [Peer fallback](peer-fallback.md) — the fast path: try multiple peers
  simultaneously rather than queuing for later
- [Group messaging](group-messaging.md) — fan-out to multiple peers; the
  delivery semantics section covers the overlap with outbox patterns
