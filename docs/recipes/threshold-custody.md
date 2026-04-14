# Threshold Custody

Split a secret across N trusted peers so that any M of them can reconstruct
it — but fewer than M learn nothing. No single peer is a single point of
failure. No single peer can act alone.

## The insight

A backup encryption key stored on one device is one hardware failure away from
loss. A backup key stored on a cloud server is one breach away from exposure.
Threshold custody distributes both the risk of loss and the risk of exposure
across a group of people you already trust.

3-of-5 means: any three of your five trusted peers can help you recover your
key. One peer being unavailable doesn't block you. Two peers colluding can't
expose your secret. This is the cryptographic formalisation of "don't put all
your eggs in one basket."

```
Secret key
    │
    │ split (3-of-5)
    ▼
Share 1 → Alice's node
Share 2 → Bob's node
Share 3 → Home server
Share 4 → Phone
Share 5 → Friend's NAS

Any 3 of the above → reconstruct → secret key
Any 2 or fewer     → learn nothing
```

## Shamir's Secret Sharing

This recipe uses Shamir's (k,n) scheme. The maths: a random polynomial of
degree k-1 is chosen such that f(0) = secret. Each share is a point (i, f(i))
on that polynomial. Any k points determine the polynomial; fewer than k points
give no information about f(0).

```ts
// Minimal GF(2^8) Shamir implementation — production use should use a
// well-audited library (e.g. `secrets.js`, `shamir` npm package, or
// the `shamir` Rust crate via WASM)

function split(secret: Uint8Array, k: number, n: number): Uint8Array[] {
  // Returns n shares; any k reconstruct the secret
  // Implementation delegates to a battle-tested library
  throw new Error('Use a reviewed library — see note below');
}

function reconstruct(shares: Uint8Array[]): Uint8Array {
  throw new Error('Use a reviewed library — see note below');
}
```

> **Use a reviewed library.** Secret sharing maths is subtle. The code above
> is intentionally unimplemented. Recommended:
> - JavaScript: [`secrets.js-grempe`](https://github.com/grempe/secrets.js)
> - Rust (WASM): [`vsss-rs`](https://crates.io/crates/vsss-rs)
> - Pure Rust: [`shamir`](https://crates.io/crates/shamir)

## Share distribution

Each share is sealed to the custodian's public key before transmission.
The custodian never sees anyone else's share.

```ts
async function distributeShares(
  node: IrohNode,
  secret: Uint8Array,
  custodians: { nodeId: string; publicKey: PublicKey }[],
  threshold: number,
) {
  const shares = split(secret, threshold, custodians.length);

  await Promise.all(
    custodians.map(async (c, i) => {
      const sealed = await sealToPeer(shares[i], c.publicKey); // from sealed-messages.md
      await node.fetch(`iroh://${c.nodeId}/custody/store`, {
        method: 'POST',
        body: sealed,
        headers: { 'Content-Type': 'application/octet-stream' },
      });
    }),
  );
}
```

## Custodian side

Each custodian runs a simple store/retrieve endpoint. They store an opaque
blob (the sealed share) and return it to the requester on demand.

```ts
const held = new Map<string, Uint8Array>(); // requestorNodeId → sealed share

node.serve({}, async (req) => {
  const url = new URL(req.url);
  const requestorId = req.headers.get('Peer-Id') ?? '';

  // PUT — store the sealed share
  if (req.method === 'POST' && url.pathname === '/custody/store') {
    const data = new Uint8Array(await req.arrayBuffer());
    held.set(requestorId, data);
    return new Response(null, { status: 204 });
  }

  // GET — return the share to the original depositor only
  if (req.method === 'GET' && url.pathname === '/custody/retrieve') {
    const share = held.get(requestorId);
    if (!share) return new Response('Not Found', { status: 404 });
    return new Response(share);
  }

  return new Response('Not Found', { status: 404 });
});
```

Because the stored blob is sealed to the requestor's public key, a custodian
cannot read the share even if they try. They hold an encrypted blob, not the
share itself.

## Recovery

The requestor collects shares from any k custodians and reconstructs locally:

```ts
async function recover(
  node: IrohNode,
  mySecretKey: SecretKey,
  custodians: string[],   // nodeIds of all custodians
  threshold: number,
): Promise<Uint8Array> {
  const shares: Uint8Array[] = [];

  for (const custodian of custodians) {
    if (shares.length >= threshold) break;
    try {
      const res = await node.fetch(`iroh://${custodian}/custody/retrieve`, {
        signal: AbortSignal.timeout(5000),
      });
      if (!res.ok) continue;

      const sealed = new Uint8Array(await res.arrayBuffer());
      const share = await openFromPeer(sealed, mySecretKey); // from sealed-messages.md
      shares.push(share);
    } catch {
      // Custodian offline — try next
    }
  }

  if (shares.length < threshold) {
    throw new Error(`Only ${shares.length} shares available; need ${threshold}`);
  }

  return reconstruct(shares);
}
```

## Consent-based recovery

For group secrets (not just key recovery), add an explicit consent step before
releasing a share. The custodian requires a signed request from the original
depositor plus a reason:

```ts
interface ReleaseRequest {
  requestorNodeId: string;
  reason: string;
  requestedAt: number;
  sig: string; // requestor signs (reason + requestedAt)
}

// Custodian verifies the signed request before releasing
if (req.method === 'POST' && url.pathname === '/custody/release') {
  const body: ReleaseRequest = await req.json();
  // Verify signature (using stored requestorNodeId's public key)
  const valid = await verifyRequest(body);
  if (!valid) return new Response('Forbidden', { status: 403 });

  // Optionally: notify the custodian's human operator before releasing
  // (out-of-band, e.g. push notification, email)

  const share = held.get(body.requestorNodeId);
  if (!share) return new Response('Not Found', { status: 404 });
  return new Response(share);
}
```

## Use cases

- **Identity key recovery**: split your long-term identity key across five
  friends' devices. Lose your phone — ask three friends. No recovery phrase
  to forget, no cloud backup to breach.
- **Shared vault**: a team splits the vault encryption key (k=3, n=5). Any
  three team members can open the vault. Two departures don't lock everyone
  out. One rogue member can't act alone.
- **Dead man's switch**: split a message across N custodians. Start a timer.
  If you don't renew the timer, custodians collectively reconstruct and
  publish the message. A whistleblower's insurance policy.
- **Group photo album**: split the album decryption key across the people in
  the photos. Any two of them can share the album with a new person; no one
  person controls access.

## See also

- [Sealed messages](sealed-messages.md) — used to encrypt each share in
  transit; the custodian never sees plaintext
- [Cooperative backup](cooperative-backup.md) — complementary: backup stores
  the ciphertext; threshold custody protects the key
- [Multi-device identity](multi-device-identity.md) — threshold custody is
  the recovery mechanism for the identity key; losing it is "losing the account"
- [Ecosystem overview](ecosystem.md) — threshold custody enables the
  "coordination without coordinators" layer
