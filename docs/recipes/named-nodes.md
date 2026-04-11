# Named Nodes

Map human-readable names to node IDs. Claim a name by signing it with your
key. Peers store and relay name records. No registrar, no DNS, no ICANN.

## The insight

A node ID is 64 hex characters. Nobody memorises it. Human-readable names
are how people actually refer to each other. The challenge: without a central
registrar, who decides "alice" maps to which node ID?

The answer is: nobody decides globally, and that's fine. Names are
**contextual** — "alice" within your group of ten friends means something
different from "alice" on the internet. Each group maintains its own mapping,
signed by the name holders themselves. Within a group, names are unique and
verifiable. Across groups, they're independently scoped — like email addresses,
but peer-issued.

```
Alice signs: { name: "alice", nodeId: "abc...", group: "our-team" }
    │
    │  stored and relayed by any peer in "our-team"
    ▼
Bob queries: GET /names/alice?group=our-team
    │
    ▼
Returns: { nodeId: "abc...", sig: "...", resolvedAt: ... }
Bob verifies Alice's key signed this → connects to "abc..."
```

## Name record

```ts
interface NameRecord {
  name: string;        // human-readable, e.g. "alice"
  group: string;       // scoping identifier, e.g. "our-team" or a group nodeId
  nodeId: string;      // the node ID this name points to
  publicKey: string;   // hex — ed25519 pub key of the name claimant
  issuedAt: number;    // Unix ms
  expiresAt?: number;  // Unix ms; omit for non-expiring names
  sig: string;         // base64url — claimant signs all of the above
}
```

The claimant signs with the private key corresponding to the node ID they're
claiming. This proves they control the node.

## Claiming a name

```ts
function claimName(
  secretKey: SecretKey,
  name: string,
  group: string,
  expiresIn?: number,  // seconds
): NameRecord {
  const record: Omit<NameRecord, 'sig'> = {
    name: name.toLowerCase().trim(),
    group,
    nodeId: secretKey.publicKey.toHex(),
    publicKey: secretKey.publicKey.toHex(),
    issuedAt: Date.now(),
    expiresAt: expiresIn ? Date.now() + expiresIn * 1000 : undefined,
  };
  const bytes = new TextEncoder().encode(JSON.stringify(record));
  return { ...record, sig: signToBase64Url(secretKey, bytes) };
}
```

## Serving the name store

```ts
const names = new Map<string, NameRecord>(); // `${group}/${name}` → record

function serveNameStore(node: IrohNode) {
  node.serve({}, async (req) => {
    const url = new URL(req.url);

    // POST /names — publish a name claim
    if (req.method === 'POST' && url.pathname === '/names') {
      const record: NameRecord = await req.json();
      if (!await verifyNameRecord(record)) {
        return new Response('Invalid signature', { status: 400 });
      }
      const key = `${record.group}/${record.name}`;
      const existing = names.get(key);
      // Only allow update by the same key holder
      if (existing && existing.publicKey !== record.publicKey) {
        return new Response('Name taken', { status: 409 });
      }
      names.set(key, record);
      return new Response(null, { status: 204 });
    }

    // GET /names/:name?group=... — resolve a name
    const match = url.pathname.match(/^\/names\/([^/]+)$/);
    if (req.method === 'GET' && match) {
      const name = match[1].toLowerCase();
      const group = url.searchParams.get('group') ?? '';
      const record = names.get(`${group}/${name}`);
      if (!record) return new Response('Not Found', { status: 404 });
      if (record.expiresAt && record.expiresAt < Date.now()) {
        names.delete(`${group}/${name}`);
        return new Response('Expired', { status: 410 });
      }
      return Response.json(record);
    }

    // GET /names?group=... — list all names in a group
    if (req.method === 'GET' && url.pathname === '/names') {
      const group = url.searchParams.get('group') ?? '';
      const groupNames = [...names.values()].filter((r) => r.group === group);
      return Response.json(groupNames);
    }

    return new Response('Not Found', { status: 404 });
  });
}

async function verifyNameRecord(record: NameRecord): Promise<boolean> {
  const { sig, ...payload } = record;
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  try {
    const pub = PublicKey.fromHex(record.publicKey);
    return pub.verify(bytes, fromBase64Url(sig));
  } catch {
    return false;
  }
}
```

## Resolving a name

```ts
async function resolveName(
  node: IrohNode,
  name: string,
  group: string,
  resolvers: string[], // nodeIds of peers that host name records
): Promise<NameRecord | null> {
  for (const resolver of resolvers) {
    try {
      const res = await node.fetch(
        `iroh://${resolver}/names/${encodeURIComponent(name)}?group=${encodeURIComponent(group)}`,
        { signal: AbortSignal.timeout(3000) },
      );
      if (!res.ok) continue;

      const record: NameRecord = await res.json();
      // Always re-verify the record, even if we trust the resolver
      if (!await verifyNameRecord(record)) continue;
      return record;
    } catch { /* try next */ }
  }
  return null;
}
```

## Connecting by name

```ts
async function fetchByName(
  node: IrohNode,
  name: string,
  group: string,
  path: string,
  resolvers: string[],
): Promise<Response> {
  const record = await resolveName(node, name, group, resolvers);
  if (!record) throw new Error(`Name "${name}" not found in group "${group}"`);
  return node.fetch(`iroh://${record.nodeId}${path}`);
}

// Usage — feels like normal HTTP
const res = await fetchByName(node, 'alice', 'our-team', '/files/hello.txt', resolvers);
```

## Petnames: local, private name aliases

Some names are meaningful only to you — "mum's laptop", "office NAS". Store
them locally, never published:

```ts
const petnames = new Map<string, string>(); // local alias → nodeId

function addPetname(alias: string, nodeId: string) {
  petnames.set(alias.toLowerCase(), nodeId);
}

function resolveLocally(name: string): string | null {
  return petnames.get(name.toLowerCase()) ?? null;
}
```

## Name propagation

When you learn a new name record, share it with your group. This is how names
spread without a central server:

```ts
async function publishToGroup(
  node: IrohNode,
  record: NameRecord,
  groupMembers: string[],
) {
  await Promise.allSettled(
    groupMembers.map((peer) =>
      node.fetch(`iroh://${peer}/names`, {
        method: 'POST',
        body: JSON.stringify(record),
        headers: { 'Content-Type': 'application/json' },
      }),
    ),
  );
}
```

## Name conflicts across groups

Two different groups can both have an "alice". That's fine — names are always
scoped to a group. If you're in both groups, you maintain two separate name
stores. Resolution always requires specifying the group.

This is intentional: it mirrors how phone contacts work. "Alice" in your
contacts is not the same "Alice" as in your colleague's contacts. Scoped
names avoid the global namespace collision problem without giving anyone
authority over who gets to be "alice@the-internet."

## See also

- [Multi-device identity](multi-device-identity.md) — the identity key that
  owns a name; changing devices doesn't change the name
- [Peer exchange](peer-exchange.md) — share name records as part of
  introductions: "this is alice, her record is attached"
- [Ecosystem overview](ecosystem.md) — named nodes are the identity layer
  that makes the rest of the ecosystem human-navigable
