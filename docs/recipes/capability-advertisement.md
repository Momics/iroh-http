# Capability Advertisement

Peers announce not just that they're online, but *what they offer*: storage,
compute, gateway relay, inbox, media transcoding, model inference. Others
discover matching peers without hardcoding any addresses. The network
self-organises around capability.

## The insight

[Presence](presence.md) answers "who is online." Capability advertisement
answers "who can do what." These are different questions with different
implications. A peer with 2 TB free is not the same as a peer with a GPU.
A peer that's your inbox relay is not interchangeable with a peer that's a
read-only archive.

Advertising capabilities over mDNS (LAN) and a small announce endpoint (WAN)
means the network discovers specialised roles dynamically. You can add a
storage node to your group without reconfiguring every other peer. You can
shed load from a gateway by spinning up a second one — peers discover it
automatically.

```
Node A advertises:  { role: "storage", freeBytes: 50_000_000_000 }
Node B advertises:  { role: "compute", gpu: "M3", tokensPerSec: 40 }
Node C advertises:  { role: "inbox" }
Node D advertises:  { role: "gateway", upstreamUrl: "http://esp32.local" }

You query:  "who has role=storage with freeBytes > 10GB?"
→ Node A
```

## Capability record

```ts
interface CapabilityRecord {
  nodeId: string;
  version: 1;
  roles: RoleDescriptor[];
  publishedAt: number;     // Unix ms
  expiresAt?: number;      // rotate/renew before this
  sig: string;             // node's own key signs this — self-attestation
}

type RoleDescriptor =
  | { role: 'storage';  freeBytes: number; totalBytes: number; pricePolicy?: string }
  | { role: 'compute';  arch: string; cpuCores: number; gpuModel?: string; tokensPerSec?: number }
  | { role: 'inbox';    maxMessagesHeld: number }
  | { role: 'gateway';  upstreamUrl: string; paths?: string[] }
  | { role: 'relay';    bandwidthBps?: number; regionHint?: string }
  | { role: 'archive';  contentHashes?: string[] }   // partial list; full list via /inventory
  | { role: string;     [key: string]: unknown };     // extensible
```

## Signing and publishing

Nodes self-attest. This is not a trust claim — it's a factual advertisement
that peers can choose to verify or ignore based on their own trust level.

```ts
function buildCapabilityRecord(
  secretKey: SecretKey,
  roles: RoleDescriptor[],
  expiresIn = 3600,   // seconds; renew before this lapses
): CapabilityRecord {
  const payload: Omit<CapabilityRecord, 'sig'> = {
    nodeId: secretKey.publicKey.toHex(),
    version: 1,
    roles,
    publishedAt: Date.now(),
    expiresAt: Date.now() + expiresIn * 1000,
  };
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  return { ...payload, sig: signToBase64Url(secretKey, bytes) };
}
```

## Serving the capability endpoint

Every node serves its own record at a well-known path:

```ts
let myRecord: CapabilityRecord;

node.serve({}, async (req) => {
  if (req.method === 'GET' && new URL(req.url).pathname === '/.well-known/capabilities') {
    return Response.json(myRecord);
  }
  // ... other routes
});

// Refresh periodically so freeBytes stays accurate
setInterval(async () => {
  myRecord = buildCapabilityRecord(secretKey, await currentRoles());
}, 60_000);
```

## Discovering capabilities on LAN

Combine with `node.browse()` — fetch each discovered peer's capability record
immediately:

```ts
const peerCaps = new Map<string, CapabilityRecord>();

async function discoverCapabilities(node: IrohNode, signal: AbortSignal) {
  for await (const event of node.browse({ signal })) {
    if (event.type === 'found') {
      fetchCapabilities(node, event.nodeId).then((cap) => {
        if (cap) peerCaps.set(event.nodeId, cap);
      });
    }
    if (event.type === 'lost') peerCaps.delete(event.nodeId);
  }
}

async function fetchCapabilities(
  node: IrohNode,
  nodeId: string,
): Promise<CapabilityRecord | null> {
  try {
    const res = await node.fetch(
      `iroh://${nodeId}/.well-known/capabilities`,
      { signal: AbortSignal.timeout(3000) },
    );
    if (!res.ok) return null;
    const record: CapabilityRecord = await res.json();
    if (!await verifyCapabilityRecord(record)) return null;
    if (record.expiresAt && record.expiresAt < Date.now()) return null;
    return record;
  } catch {
    return null;
  }
}

async function verifyCapabilityRecord(record: CapabilityRecord): Promise<boolean> {
  try {
    const { sig, ...payload } = record;
    const bytes = new TextEncoder().encode(JSON.stringify(payload));
    return PublicKey.fromHex(record.nodeId).verify(bytes, fromBase64Url(sig));
  } catch {
    return false;
  }
}
```

## Querying: find peers by role

```ts
function findPeers(
  role: string,
  filter?: (desc: RoleDescriptor) => boolean,
): string[] {
  const results: string[] = [];
  for (const [nodeId, record] of peerCaps) {
    if (record.expiresAt && record.expiresAt < Date.now()) continue;
    const match = record.roles.find(
      (r) => r.role === role && (filter ? filter(r) : true),
    );
    if (match) results.push(nodeId);
  }
  return results;
}

// Examples:
const storageNodes = findPeers('storage',
  (r) => (r as any).freeBytes > 10 * 1024 ** 3,  // >10 GB free
);

const gpuNodes = findPeers('compute',
  (r) => (r as any).gpuModel != null,
);

const inboxNodes = findPeers('inbox');
```

## WAN capability registry

For WAN peers (not on your LAN), maintain a simple registry node that
aggregates capability records. Peers POST their records; clients GET to query.

```ts
// Registry node
const registry = new Map<string, CapabilityRecord>();

node.serve({}, async (req) => {
  const url = new URL(req.url);

  // POST /registry — peer announces itself
  if (req.method === 'POST' && url.pathname === '/registry') {
    const record: CapabilityRecord = await req.json();
    if (!await verifyCapabilityRecord(record)) {
      return new Response('Invalid signature', { status: 400 });
    }
    // Only the owner can update their own record
    if (record.nodeId !== req.headers.get('iroh-node-id')) {
      return new Response('Forbidden', { status: 403 });
    }
    registry.set(record.nodeId, record);
    return new Response(null, { status: 204 });
  }

  // GET /registry?role=storage — query by role
  if (req.method === 'GET' && url.pathname === '/registry') {
    const role = url.searchParams.get('role');
    const now = Date.now();
    const results = [...registry.values()].filter((r) => {
      if (r.expiresAt && r.expiresAt < now) return false;
      if (role && !r.roles.some((rd) => rd.role === role)) return false;
      return true;
    });
    return Response.json(results);
  }

  return new Response('Not Found', { status: 404 });
});

// Peers announce to the registry periodically
async function keepAlive(node: IrohNode, registryNodeId: string, record: CapabilityRecord) {
  await node.fetch(`iroh://${registryNodeId}/registry`, {
    method: 'POST',
    body: JSON.stringify(record),
    headers: { 'Content-Type': 'application/json' },
  });
}
```

## Composing with other recipes

**Dynamic inbox selection** — instead of hardcoding an inbox node ID in
[sealed-messages.md](sealed-messages.md), discover it:

```ts
const [inbox] = findPeers('inbox');
if (!inbox) throw new Error('No inbox peer available');
await sendSealed(node, inbox, recipientNodeId, message);
```

**Dynamic backup selection** — find available storage nodes and distribute
shares across them ([cooperative-backup.md](cooperative-backup.md)):

```ts
const storageNodes = findPeers('storage',
  (r) => (r as any).freeBytes > blobSize * 2,
);
await backupBlob(node, data, storageNodes.slice(0, 3));
```

**Dynamic job routing** — send work to available compute nodes
([job-dispatch.md](job-dispatch.md)):

```ts
const workers = findPeers('compute');
```

## Failure modes

- **Stale records**: a peer goes offline but their record hasn't expired yet.
  Always attempt to connect before trusting that a peer with a valid record
  is actually reachable. Use short expiry + keepalive to bound staleness.
- **Self-attestation is unverified**: a peer can claim more free space than
  they have, or a GPU they don't own. For high-stakes decisions (large
  backups, paid compute), verify by probing: send a small test blob, or run a
  small test job, before committing.
- **Registry node as single point of failure**: for WAN discovery, run the
  registry on multiple nodes and query all of them. The records are
  self-signed so any replica of the registry data is equally valid.

## When not to use this pattern

If your peer set is small and stable (you and three friends, always the same
devices), hardcoding node IDs is simpler. Capability advertisement pays off
when the peer set grows, changes, or includes strangers in a shared community.

## See also

- [Presence](presence.md) — who is online (prerequisite for checking
  capability records)
- [Job dispatch](job-dispatch.md) — the compute role in action
- [Sealed messages](sealed-messages.md) — the inbox role in action
- [Cooperative backup](cooperative-backup.md) — the storage role in action
- [Ecosystem overview](ecosystem.md) — capability advertisement is the
  service-discovery layer of the full network stack
