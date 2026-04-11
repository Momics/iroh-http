# Release Channels

Publish software releases over iroh. Clients verify before installing.
Peers propagate the release automatically so the origin barely has to serve
anyone. No CDN, no S3 bucket, no package registry account.

## The insight

Software distribution is a solved problem at scale — for organisations that
can afford a CDN, a code-signing certificate, and a package registry account.
For indie tools, local software, community projects, and air-gapped networks,
the infrastructure cost is often higher than the project itself.

iroh-http makes this a non-problem. The author has a node ID. A release is a
signed message: "version 2.1.0 is content address `abc...`." Peers who've
already fetched the release re-serve it to others. The author's signing key IS
the certificate authority.

This is also the first recipe that composes the entire stack: `append-only-log`
for the release history, `signed-caching`/`content-routing` for distribution,
`capability-advertisement` for peer discovery, and `sign-verify` for
authenticity.

```
Author signs release 2.1.0 → append to release log
    │
    │  peers subscribe to the log
    ▼
Subscriber fetches the release blob from the author (or any peer that has it)
    │
    │  verifies signature before installing
    ▼
Subscriber re-serves to others → author's bandwidth stays constant
```

## Release record

```ts
interface Release {
  channel: string;        // e.g. "stable", "beta", "nightly"
  version: string;        // semver
  contentHash: string;    // sha256 hex of the release archive
  contentSize: number;    // bytes
  description?: string;   // changelog / release notes
  publishedAt: number;    // Unix ms
  author: string;         // publisher's node ID hex
  sig: string;            // publisher's Ed25519 sig over the above
}
```

## Publishing a release

The author publishes in two steps: upload the blob, then append to the log.

```ts
async function publishRelease(
  node: IrohNode,
  secretKey: SecretKey,
  opts: {
    channel: string;
    version: string;
    archive: Uint8Array;
    description?: string;
  },
): Promise<Release> {
  // 1. Compute content address
  const contentHash = await sha256hex(opts.archive);

  // 2. Serve the blob so peers can fetch it
  const blobs = new Map<string, Uint8Array>();
  blobs.set(contentHash, opts.archive);

  // 3. Sign the release record
  const record: Omit<Release, 'sig'> = {
    channel: opts.channel,
    version: opts.version,
    contentHash,
    contentSize: opts.archive.length,
    description: opts.description,
    publishedAt: Date.now(),
    author: secretKey.publicKey.toHex(),
  };
  const bytes = new TextEncoder().encode(JSON.stringify(record));
  const release: Release = { ...record, sig: signToBase64Url(secretKey, bytes) };

  // 4. Append to the release log (see append-only-log.md)
  await releaseLog.append(release);

  return release;
}
```

## The publisher node

Serves both the release log (for discovery) and the blob content (for download):

```ts
const blobs = new Map<string, Uint8Array>();   // contentHash → archive bytes
const releaseLog = new AppendOnlyLog(secretKey);

node.serve({}, async (req) => {
  const url = new URL(req.url);

  // GET /releases/{channel} — latest in channel
  if (req.method === 'GET' && url.pathname.startsWith('/releases/')) {
    const channel = url.pathname.slice(10);
    const latest = [...releaseLog.since(0)]
      .map((e) => e.payload as Release)
      .filter((r) => r.channel === channel)
      .at(-1);
    if (!latest) return new Response('Not Found', { status: 404 });
    return Response.json(latest);
  }

  // GET /releases/{channel}/log — full history
  if (req.method === 'GET' && url.pathname.match(/^\/releases\/[^/]+\/log$/)) {
    const channel = url.pathname.split('/')[2];
    const history = releaseLog.since(0)
      .map((e) => e.payload as Release)
      .filter((r) => r.channel === channel);
    return Response.json(history);
  }

  // GET /content/{hash} — serve the archive
  const blobMatch = url.pathname.match(/^\/content\/([0-9a-f]{64})$/);
  if (req.method === 'GET' && blobMatch) {
    const hash = blobMatch[1];
    const blob = blobs.get(hash);
    if (!blob) return new Response('Not Found', { status: 404 });
    return new Response(blob, {
      headers: {
        'Content-Type': 'application/octet-stream',
        'Cache-Control': 'immutable, max-age=31536000',
        // ETag = content hash = tamper proof (see signed-caching.md)
        'ETag': `"${hash}"`,
      },
    });
  }

  return new Response('Not Found', { status: 404 });
});
```

## Client: check for updates

```ts
async function checkUpdate(
  node: IrohNode,
  publisherNodeId: string,
  channel: string,
  currentVersion: string,
): Promise<Release | null> {
  const res = await node.fetch(`iroh://${publisherNodeId}/releases/${channel}`);
  if (!res.ok) return null;

  const release: Release = await res.json();

  // Verify the publisher signed this
  if (!await verifyRelease(release, publisherNodeId)) return null;

  // Compare versions (naive semver; use a library for production)
  if (release.version <= currentVersion) return null;

  return release;
}

async function verifyRelease(release: Release, expectedAuthor: string): Promise<boolean> {
  if (release.author !== expectedAuthor) return false;
  const { sig, ...payload } = release;
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  try {
    return PublicKey.fromHex(release.author).verify(bytes, fromBase64Url(sig));
  } catch {
    return false;
  }
}
```

## Client: fetch and verify before installing

```ts
async function fetchRelease(
  node: IrohNode,
  release: Release,
  sources: string[],   // publisher + any peer that may have a copy
): Promise<Uint8Array> {
  for (const source of sources) {
    try {
      const res = await node.fetch(
        `iroh://${source}/content/${release.contentHash}`,
        { signal: AbortSignal.timeout(30_000) },
      );
      if (!res.ok) continue;

      const data = new Uint8Array(await res.arrayBuffer());

      // Content address = integrity proof (no separate signature needed)
      const actualHash = await sha256hex(data);
      if (actualHash !== release.contentHash) {
        console.warn(`Hash mismatch from ${source} — skipping`);
        continue;
      }

      // Cache locally so we can re-serve to peers (content-routing pattern)
      localBlobs.set(release.contentHash, data);
      return data;
    } catch { /* try next source */ }
  }

  throw new Error(`Could not fetch release ${release.version} from any source`);
}
```

## Subscribing to a release channel

Clients subscribe to the release log and get notified when new versions are
published — no polling infrastructure required:

```ts
async function subscribeChannel(
  node: IrohNode,
  publisherNodeId: string,
  channel: string,
  onRelease: (r: Release) => void,
  signal: AbortSignal,
) {
  await followLog(                        // from append-only-log.md
    node,
    publisherNodeId,
    publisherPublicKey,
    (entry) => {
      const release = entry.payload as Release;
      if (release.channel === channel) onRelease(release);
    },
    signal,
  );
}
```

## Peer propagation

Peers that have downloaded a release re-serve it. Combine with
`capability-advertisement`:

```ts
// Announce that you have a specific release blob
node.advertise({
  roles: [{
    role: 'archive',
    contentHashes: [...localBlobs.keys()],
  }],
});

// Clients discover archive peers and prefer them over the origin
const archivePeers = findPeers('archive',
  (r) => (r as any).contentHashes?.includes(release.contentHash),
);
const sources = [...archivePeers, publisherNodeId];
const archive = await fetchRelease(node, release, sources);
```

## Pinning a trusted publisher

The author's node ID IS the package identity. Pinning is trivial:

```ts
const TRUSTED_PUBLISHER = 'abc123...'; // author's node ID hex — hardcoded in the app

// verifyRelease checks that release.author === TRUSTED_PUBLISHER
// and that the signature validates against that key.
// No certificate authority. No TOFU ceremony. Just the key.
```

To change publishers (project transfer, author compromise), publish a
signed delegation to the release log pointing to the new author's key — the
same rotation pattern as [key-rotation.md](key-rotation.md).

## Failure modes

- **Publisher offline**: clients fetch from peers that have the archive. If no
  peer has it, they fall back to polling the publisher periodically via the
  [offline-first](offline-first.md) outbox pattern.
- **Compromised publisher key**: an attacker publishes a malicious release. The
  content hash won't match a known good version, but clients who update
  automatically could be tricked. Mitigations: require human approval before
  installing major version bumps; pin a minimum signing date; use threshold
  signing for critical releases.
- **Corrupted relay**: a relay node tampers with the archive bytes. The
  content address check (`actualHash !== release.contentHash`) catches this
  immediately — the release is rejected and the next source is tried.

## Threat model

**Protects against:**
- Man-in-the-middle archive tampering (content hash)
- Impersonating the publisher (Ed25519 signature)
- CDN compromise (no CDN involved)
- Typosquatting / namespace hijacking (author key IS the package identity)

**Does not protect against:**
- The publisher's signing key being compromised (→ use key rotation)
- A client that skips `verifyRelease()` — ensure verification is mandatory, not
  optional, in your update logic
- Rollback attacks — add a `minVersion` field to the pinned publisher record
  so clients refuse to downgrade

## When not to use this pattern

For public software with millions of users, a proper package registry
(npm, PyPI, Homebrew) has better search, dependency resolution, and ecosystem
tooling. iroh release channels are best for:
- Internal tools distributed within a team or community
- Air-gapped or restricted networks
- Software where the publisher key IS the trust anchor (security tools,
  cryptographic libraries, community-run projects)

## See also

- [Append-only log](append-only-log.md) — the release history log
- [Content routing](content-routing.md) — peer propagation of archive blobs
- [Signed caching](signed-caching.md) — the immutable ETag on archive responses
- [Key rotation](key-rotation.md) — what to do if the publisher's signing key
  is compromised
- [Capability advertisement](capability-advertisement.md) — archive peers
  announcing they hold release blobs
