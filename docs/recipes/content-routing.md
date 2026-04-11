# Content Routing

Fetch content from the nearest peer that has it, fall back to the origin only
when necessary. Peers that have already fetched a blob re-serve it to others.
Bandwidth concentrates at the edge; the origin barely feels it.

## The insight

BitTorrent proved that distributing the act of serving content across all
consumers dramatically reduces origin load. iroh-http makes the same pattern
available over standard HTTP semantics — no DHT, no tracker, no specialized
protocol. A peer that has fetched `/release/v2.0.tar.gz` can serve it to
others. The content hash is the address.

```
                Origin (Pi, home server)
                   │        ▲
             fetches │        │ only on cache miss
                   ▼        │
  Peer A ────────────────── Peer B (has a copy)
      │                         │
      │   iroh QUIC direct      │
      │                         │
  Peer C ◄───── serves ─────────┘
  (saves origin bandwidth + relay latency)
```

## Content addresses

Content is identified by its SHA-256 hash, not its URL. Two peers serving the
same bytes have the same content address, regardless of where they got it.

```ts
async function contentAddress(data: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', data);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}
```

## Serving what you have

Every node that fetches a blob re-serves it automatically:

```ts
const localCache = new Map<string, Uint8Array>(); // hash → bytes

function startContentRouter(node: IrohNode) {
  node.serve({}, async (req) => {
    const url = new URL(req.url);
    const match = url.pathname.match(/^\/content\/([0-9a-f]{64})$/);
    if (!match) return new Response('Not Found', { status: 404 });
    const hash = match[1];

    const cached = localCache.get(hash);
    if (cached) {
      return new Response(cached, {
        headers: {
          'Content-Type': 'application/octet-stream',
          'x-served-by': node.nodeId(),  // so the requester knows who served it
          'Cache-Control': 'immutable, max-age=31536000',
        },
      });
    }

    return new Response('Not Found', { status: 404 });
  });
}
```

## Fetching with peer discovery

When you need a blob, first ask known peers; only hit the origin on a miss:

```ts
async function fetchContent(
  node: IrohNode,
  hash: string,
  opts: {
    peers: string[];           // peers to try first (LAN peers should be first)
    origin: string;            // authoritative nodeId — always has it
    verify?: boolean;          // re-hash after download (default: true)
  },
): Promise<Uint8Array> {
  const verify = opts.verify ?? true;
  const path = `/content/${hash}`;

  // Try peers first (cheapest: LAN → WAN → origin)
  const allSources = [...opts.peers, opts.origin];
  for (const source of allSources) {
    try {
      const res = await node.fetch(`iroh://${source}${path}`, {
        signal: AbortSignal.timeout(source === opts.origin ? 30_000 : 5_000),
      });
      if (!res.ok) continue;

      const data = new Uint8Array(await res.arrayBuffer());

      if (verify && await contentAddress(data) !== hash) {
        console.warn(`Hash mismatch from peer ${source} — skipping`);
        continue;
      }

      // Cache locally and re-serve to others
      localCache.set(hash, data);
      return data;
    } catch {
      // Peer offline or timeout — try next
    }
  }

  throw new Error(`Content ${hash} not available from any source`);
}
```

## Announcing what you have

Peers broadcast their content inventory so others know who to ask:

```ts
// GET /inventory — returns the list of content hashes this node is serving
node.serve({}, async (req) => {
  if (req.method === 'GET' && new URL(req.url).pathname === '/inventory') {
    return Response.json([...localCache.keys()]);
  }
  // content routes above...
});

// Query a peer's inventory before deciding to route through them
async function getPeerInventory(
  node: IrohNode,
  peerNodeId: string,
): Promise<Set<string>> {
  try {
    const res = await node.fetch(`iroh://${peerNodeId}/inventory`, {
      signal: AbortSignal.timeout(2000),
    });
    if (!res.ok) return new Set();
    return new Set(await res.json() as string[]);
  } catch {
    return new Set();
  }
}
```

## Smart peer selection

On LAN, query inventories first and pick a peer that already has the content:

```ts
async function smartFetch(
  node: IrohNode,
  hash: string,
  lanPeers: string[],
  origin: string,
): Promise<Uint8Array> {
  // Check who has it (in parallel, fast timeout)
  const inventories = await Promise.all(
    lanPeers.map(async (peer) => ({
      peer,
      has: (await getPeerInventory(node, peer)).has(hash),
    })),
  );

  const havingPeers = inventories.filter((i) => i.has).map((i) => i.peer);
  const otherPeers = lanPeers.filter((p) => !havingPeers.includes(p));

  // Try having-peers first, then others, then origin
  return fetchContent(node, hash, {
    peers: [...havingPeers, ...otherPeers],
    origin,
  });
}
```

## Eviction

For a simple LRU cache to bound memory:

```ts
class BoundedCache {
  private data = new Map<string, Uint8Array>();
  private readonly maxBytes: number;
  private currentBytes = 0;

  constructor(maxMb = 500) {
    this.maxBytes = maxMb * 1024 * 1024;
  }

  set(hash: string, data: Uint8Array): void {
    if (this.data.has(hash)) return; // already cached
    while (this.currentBytes + data.length > this.maxBytes && this.data.size > 0) {
      // Evict oldest entry
      const oldest = this.data.keys().next().value!;
      this.currentBytes -= this.data.get(oldest)!.length;
      this.data.delete(oldest);
    }
    this.data.set(hash, data);
    this.currentBytes += data.length;
  }

  get(hash: string): Uint8Array | undefined {
    if (!this.data.has(hash)) return undefined;
    // Move to end (most-recently-used)
    const val = this.data.get(hash)!;
    this.data.delete(hash);
    this.data.set(hash, val);
    return val;
  }
}
```

## What this enables

- **Software distribution**: publish a release once; all peers who downloaded
  it re-serve it to new peers automatically. Origin load is O(1) regardless
  of download count.
- **Shared media in a group**: when one person in a chat downloads an image,
  all group members on the same LAN get it from each other rather than the
  origin.
- **Resilient docs**: a team's shared documents can be served by any team
  member's device. If the primary is offline, any peer who has fetched the
  document recently can serve it.

## See also

- [Cooperative backup](cooperative-backup.md) — store blobs intentionally
  rather than as a side-effect of fetching; add redundancy guarantees
- [Signed caching](signed-caching.md) — add a signature to the served bytes
  so the requester can verify authenticity even from an untrusted peer
- [Peer fallback](peer-fallback.md) — the underlying try-next pattern used
  in `fetchContent()`
