# Cooperative Backup

Ask trusted peers to each hold a copy of your data. Verify the copies with a
signed manifest. Reconstruct from any surviving peer. No cloud storage account
required.

## The insight

You already trust your friends' devices with your encryption keys (Signal,
iCloud Family). iroh-http lets you extend that trust to storage. A "backup"
becomes a `PUT` to a peer you already talk to, with a signature to prove it
hasn't been tampered with.

```
         ┌── PUT /blob/abc ──► laptop (friend A)
You      │                         │
node ────┤── PUT /blob/abc ──► NAS (home)   ← 3 copies
         │                         │
         └── PUT /blob/abc ──► phone (friend B)

         ← any one peer can serve GET /blob/abc back to you →
```

## Blob format

```ts
interface BlobEntry {
  id: string;        // sha256 hex of the content
  size: number;
  storedAt: number;  // Unix ms
}
```

Peers store blobs by their content hash. No deduplication library needed —
identical content has the same ID automatically.

## Backup node (peers run this)

```ts
import { IrohNode } from 'iroh-http';

const blobs = new Map<string, Uint8Array>(); // id → bytes

function startBackupServer(node: IrohNode) {
  node.serve({}, async (req) => {
    const url = new URL(req.url);
    const match = url.pathname.match(/^\/blob\/([0-9a-f]{64})$/);
    if (!match) return new Response('Not Found', { status: 404 });
    const id = match[1];

    if (req.method === 'HEAD') {
      if (!blobs.has(id)) return new Response(null, { status: 404 });
      return new Response(null, {
        headers: { 'Content-Length': String(blobs.get(id)!.length) },
      });
    }

    if (req.method === 'GET') {
      const blob = blobs.get(id);
      if (!blob) return new Response('Not Found', { status: 404 });
      return new Response(blob);
    }

    if (req.method === 'PUT') {
      const data = new Uint8Array(await req.arrayBuffer());
      // Verify the caller's claimed ID matches the actual content hash
      const hash = await sha256hex(data);
      if (hash !== id) return new Response('Hash mismatch', { status: 400 });
      blobs.set(id, data);
      return new Response(null, { status: 204 });
    }

    return new Response('Method Not Allowed', { status: 405 });
  });
}

async function sha256hex(data: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', data);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}
```

## Owner side — backup

```ts
async function backupBlob(
  node: IrohNode,
  data: Uint8Array,
  peers: string[],
): Promise<string> {
  const id = await sha256hex(data);

  // Upload to all peers; don't fail if one is offline
  const results = await Promise.allSettled(
    peers.map((peer) =>
      node.fetch(`iroh://${peer}/blob/${id}`, {
        method: 'PUT',
        body: data,
        headers: { 'Content-Type': 'application/octet-stream' },
      })
    ),
  );

  const succeeded = results.filter((r) => r.status === 'fulfilled').length;
  if (succeeded === 0) throw new Error('All peers unavailable');
  console.log(`Stored ${id} on ${succeeded}/${peers.length} peers`);

  return id;
}
```

## Owner side — verify manifest

The manifest is the list of (blob ID, expected size) pairs, signed with the
owner's key. A corrupt or substituted blob fails verification without
contacting the owner.

```ts
async function buildManifest(
  secretKey: SecretKey,
  entries: BlobEntry[],
): Promise<string> {
  const payload = new TextEncoder().encode(JSON.stringify(entries));
  const sig = secretKey.sign(payload);
  const combined = new Uint8Array(payload.length + sig.length);
  combined.set(payload);
  combined.set(sig, payload.length);
  return btoa(String.fromCharCode(...combined))
    .replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, '');
}

async function verifyManifest(
  publicKey: PublicKey,
  manifestStr: string,
): Promise<BlobEntry[]> {
  const raw = Uint8Array.from(
    atob(manifestStr.replace(/-/g, '+').replace(/_/g, '/')),
    (c) => c.charCodeAt(0),
  );
  const sigOffset = raw.length - 64;
  const payload = raw.slice(0, sigOffset);
  const sig = raw.slice(sigOffset);
  if (!publicKey.verify(payload, sig)) throw new Error('Invalid manifest signature');
  return JSON.parse(new TextDecoder().decode(payload));
}
```

See [sign-verify](../features/sign-verify.md) for the `sign`/`verify`
primitives.

## Owner side — restore

```ts
async function restoreBlob(
  node: IrohNode,
  id: string,
  peers: string[],
): Promise<Uint8Array> {
  for (const peer of peers) {
    try {
      const res = await node.fetch(`iroh://${peer}/blob/${id}`);
      if (!res.ok) continue;
      const data = new Uint8Array(await res.arrayBuffer());
      // Verify content hash before trusting
      if (await sha256hex(data) !== id) continue;
      return data;
    } catch {
      // Peer offline — try the next one
    }
  }
  throw new Error(`Could not restore blob ${id} from any peer`);
}
```

## Checking liveness

Periodically confirm peers still hold your blobs without re-downloading them:

```ts
async function auditBackup(
  node: IrohNode,
  entries: BlobEntry[],
  peers: string[],
): Promise<Map<string, string[]>> {
  // Returns: blobId → list of peers that confirmed they have it
  const coverage = new Map<string, string[]>();

  for (const { id } of entries) {
    const holding: string[] = [];
    await Promise.allSettled(
      peers.map(async (peer) => {
        const res = await node.fetch(`iroh://${peer}/blob/${id}`, {
          method: 'HEAD',
        });
        if (res.ok) holding.push(peer);
      }),
    );
    coverage.set(id, holding);
    if (holding.length === 0) console.warn(`⚠ No peers hold blob ${id}`);
  }

  return coverage;
}
```

## Redundancy strategy

- **Minimum 2 peers** before treating a blob as safely backed up.
- **3-2-1 rule** adapted to P2P: 3 peers, 2 different locations (e.g., a
  friend's device on a different network + your home NAS), 1 offline copy if
  the data is irreplaceable.
- Peers can be incentivised with reciprocity: `you store mine, I store yours`.

## See also

- [Signed caching](signed-caching.md) — same hash-as-content-identity
  pattern applied to HTTP caching
- [Capability tokens](capability-tokens.md) — restrict who can store blobs on
  your node; scope `PUT /blob/*` to known owners
- [Peer fallback](peer-fallback.md) — restore drives the same
  try-next-on-failure loop
