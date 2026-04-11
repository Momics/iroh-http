# Append-Only Log

Every node maintains a signed, append-only log of its state changes. Other
nodes subscribe to the tail. History is verifiable; replaying the log
reconstructs the state from scratch; and two nodes that diverged can always
merge by replaying both logs in order.

## The insight

A database is a snapshot. A log is a *history*. Given a log, you can always
reconstruct the snapshot. Given only a snapshot, you can never recover what
happened between two versions.

In a P2P network with no central database, the log is the primitive. Each
node is the authoritative source of its own history. Other nodes subscribe to
that log and maintain derived state locally. The network is a graph of
interlinked logs, each signed by its owner.

This is event sourcing without an event bus, CRDT without a runtime, and
git's data model applied to arbitrary state — all using iroh-http as the
transport.

```
Node A log:          Node B (subscriber):
 [0] genesis         replays → derives state
 [1] set key=hello
 [2] set key=world   ← subscribes from entry 2
 [3] del key         ← receives 3 → updates local view
```

## Entry format

```ts
interface LogEntry {
  seq: number;       // monotonically increasing, gap-free
  parentHash: string;// sha256 hex of the previous entry's canonical form
  timestamp: number; // Unix ms — informational only, not enforced
  author: string;    // nodeId hex
  payload: unknown;  // application-defined
  sig: string;       // base64url Ed25519 over (seq + parentHash + payload)
}

const GENESIS_HASH = '0'.repeat(64);
```

## Appending

```ts
class AppendOnlyLog {
  private entries: LogEntry[] = [];
  private secretKey: SecretKey;

  constructor(secretKey: SecretKey) {
    this.secretKey = secretKey;
  }

  async append(payload: unknown): Promise<LogEntry> {
    const seq = this.entries.length;
    const parent = seq === 0 ? GENESIS_HASH : await this.hash(this.entries[seq - 1]);
    const entry: Omit<LogEntry, 'sig'> = {
      seq,
      parentHash: parent,
      timestamp: Date.now(),
      author: this.secretKey.publicKey.toHex(),
      payload,
    };
    const bytes = new TextEncoder().encode(JSON.stringify(entry));
    const sig = signToBase64Url(this.secretKey, bytes);
    const full: LogEntry = { ...entry, sig };
    this.entries.push(full);
    return full;
  }

  private async hash(entry: LogEntry): Promise<string> {
    const bytes = new TextEncoder().encode(JSON.stringify(entry));
    const digest = await crypto.subtle.digest('SHA-256', bytes);
    return Array.from(new Uint8Array(digest))
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');
  }

  since(seq: number): LogEntry[] {
    return this.entries.slice(seq);
  }

  head(): LogEntry | undefined {
    return this.entries[this.entries.length - 1];
  }
}
```

## Serving the log

```ts
function serveLog(node: IrohNode, log: AppendOnlyLog) {
  node.serve({}, async (req) => {
    const url = new URL(req.url);

    // GET /log — full log
    if (req.method === 'GET' && url.pathname === '/log') {
      const since = Number(url.searchParams.get('since') ?? '0');
      return Response.json(log.since(since));
    }

    // GET /log/head — current head (seq + hash) for cheap polling
    if (req.method === 'GET' && url.pathname === '/log/head') {
      return Response.json(log.head() ?? null);
    }

    return new Response('Not Found', { status: 404 });
  });
}
```

## Verifying and replaying

Subscribers verify every entry before applying it. A single invalid signature
or broken parent chain means the log has been tampered with.

```ts
async function verifyEntry(
  entry: LogEntry,
  prev: LogEntry | null,
  authorKey: PublicKey,
): Promise<boolean> {
  // Check sequence
  if (prev && entry.seq !== prev.seq + 1) return false;
  if (!prev && entry.seq !== 0) return false;

  // Check parent hash
  const expectedParent = prev
    ? await sha256json(prev)
    : GENESIS_HASH;
  if (entry.parentHash !== expectedParent) return false;

  // Check signature
  const { sig, ...payload } = entry;
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  return authorKey.verify(bytes, fromBase64Url(sig));
}

async function replayLog(
  entries: LogEntry[],
  authorKey: PublicKey,
  apply: (entry: LogEntry) => void,
): Promise<void> {
  for (let i = 0; i < entries.length; i++) {
    const valid = await verifyEntry(entries[i], entries[i - 1] ?? null, authorKey);
    if (!valid) throw new Error(`Entry ${entries[i].seq} failed verification`);
    apply(entries[i]);
  }
}
```

## Subscribing — polling

```ts
async function followLog(
  node: IrohNode,
  sourceNodeId: string,
  authorKey: PublicKey,
  onEntry: (entry: LogEntry) => void,
  signal: AbortSignal,
) {
  let since = 0;

  while (!signal.aborted) {
    try {
      const res = await node.fetch(
        `iroh://${sourceNodeId}/log?since=${since}`,
        { signal: AbortSignal.any([signal, AbortSignal.timeout(10_000)]) },
      );
      if (res.ok) {
        const entries: LogEntry[] = await res.json();
        for (const entry of entries) {
          await verifyEntry(entry, null /* simplified */, authorKey);
          onEntry(entry);
          since = entry.seq + 1;
        }
      }
    } catch { /* offline — retry */ }

    await sleep(5000, signal);
  }
}
```

## Subscribing — streaming (WebTransport)

For lower latency, the publisher can stream new entries over a bidi stream as
they're appended:

```ts
// Publisher — push entries as they're written
session.accept().then(async ({ readable }) => {
  const writer = /* get writable to subscriber */;
  log.onAppend((entry) => {
    writer.write(new TextEncoder().encode(JSON.stringify(entry) + '\n'));
  });
});
```

See [webtransport](../features/webtransport.md) for the bidi stream API.

## Merging two logs

When two nodes have independently appended to their own fork of a shared log,
merge is deterministic: sort all entries by (seq, author), apply in order.
This is only well-defined if entries are independent (no entry refers to
another's internal state). For dependent entries, use vector clocks or a CRDT.

```ts
function mergeLogs(a: LogEntry[], b: LogEntry[]): LogEntry[] {
  const all = [...a, ...b];
  // Deduplicate by (author, seq) — same author+seq means same entry
  const seen = new Set<string>();
  const deduped = all.filter((e) => {
    const key = `${e.author}:${e.seq}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  return deduped.sort((a, b) => a.timestamp - b.timestamp || a.author.localeCompare(b.author));
}
```

## Applications

- **Audit trail**: every change to a shared resource is a log entry. Any node
  can replay the log and verify the full history.
- **Collaborative document**: entries are CRDT deltas. Subscribers apply them
  in any order. State converges.
- **Activity feed**: entries are events (post, like, comment). Subscribers
  build derived timelines. The log IS the social graph.
- **Config changelog**: who changed what, when, proven by the author's key.
  Deploy by replaying the log onto a fresh container.

## See also

- [Local-first sync](local-first-sync.md) — simpler (no log, just latest-wins
  version); upgrade to append-only-log when you need history
- [Witness receipts](witness-receipts.md) — a third node co-signs specific
  entries in the log; stronger accountability than self-signed logs alone
- [Cooperative backup](cooperative-backup.md) — store log snapshots across
  peers so no single peer's failure loses history
- [Ecosystem overview](ecosystem.md) — how logs compose into the full network
  coordination layer
