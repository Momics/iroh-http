# Peer Exchange

When you know Alice and Alice knows Bob, Alice can introduce you to Bob
directly — without Alice staying in the loop. The introduction is signed by
Alice, so Bob knows she vouched for you. This is how trust propagates in a
decentralized network without a central directory.

## The insight

In a centralized app, "find people I know" hits a database. In a P2P network,
discovery is a graph problem: you only know who you already know. Peer
exchange — giving someone else's ticket to a third party — extends the graph
one hop at a time. The signature on the introduction is the cryptographic
equivalent of saying "a mutual friend sent me."

```
     You ──────── know ──────► Alice
                                  │
                    signs + sends │  introduction card
                                  │
                                  ▼
     You ─── iroh QUIC direct ──► Bob
     (Bob knows You were vouched for by Alice)
```

## Introduction card

An introduction is Alice's signed endorsement of Bob's ticket, sent to you:

```ts
interface Introduction {
  ticket: string;       // Bob's ticket (encodes nodeId + addresses)
  note?: string;        // e.g. "This is Bob, my colleague"
  introducerNodeId: string;
  issuedAt: number;     // Unix ms
  sig: string;          // base64url Ed25519 signature over the above fields
}
```

## Creating an introduction

Alice creates this and sends it to you:

```ts
function createIntroduction(
  secretKey: SecretKey,
  opts: { ticket: string; note?: string },
): Introduction {
  const payload: Omit<Introduction, 'sig'> = {
    ticket: opts.ticket,
    note: opts.note,
    introducerNodeId: secretKey.publicKey.toHex(),
    issuedAt: Date.now(),
  };
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  const sig = signToBase64Url(secretKey, bytes);
  return { ...payload, sig };
}
```

## Verifying an introduction

You receive the introduction from Alice and verify her signature before
connecting to Bob:

```ts
async function verifyIntroduction(
  intro: Introduction,
  knownPeers: Map<string, { publicKey: PublicKey }>,
): Promise<boolean> {
  const introducer = knownPeers.get(intro.introducerNodeId);
  if (!introducer) return false; // Don't accept introductions from strangers

  const { sig, ...payload } = intro;
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  return introducer.publicKey.verify(bytes, fromBase64Url(sig));
}
```

## Introduction endpoint

Each node hosts an endpoint to receive introductions:

```ts
const pendingIntroductions: Introduction[] = [];

node.serve({}, async (req) => {
  if (req.method === 'POST' && new URL(req.url).pathname === '/introduce') {
    const intro: Introduction = await req.json();
    const senderNodeId = req.headers.get('iroh-node-id');

    // Only accept introductions from peers you already know
    if (!knownPeers.has(senderNodeId ?? '')) {
      return new Response('Unknown sender', { status: 403 });
    }

    const valid = await verifyIntroduction(intro, knownPeers);
    if (!valid) return new Response('Invalid signature', { status: 400 });

    pendingIntroductions.push(intro);
    return new Response(null, { status: 204 });
  }
  // ...
});
```

## Sending an introduction

Alice sends Bob's introduction to you:

```ts
async function introduce(
  node: IrohNode,
  secretKey: SecretKey,
  recipientNodeId: string,
  subjectTicket: string,
  note?: string,
): Promise<void> {
  const intro = createIntroduction(secretKey, { ticket: subjectTicket, note });
  await node.fetch(`iroh://${recipientNodeId}/introduce`, {
    method: 'POST',
    body: JSON.stringify(intro),
    headers: { 'Content-Type': 'application/json' },
  });
}

// Alice introduces Bob to You:
await introduce(aliceNode, aliceSecretKey, yourNodeId, bobTicket, 'This is Bob');
```

## Acting on introductions

When you receive a valid introduction, you can connect to the new peer and
record the social context:

```ts
async function processIntroductions(node: IrohNode) {
  for (const intro of pendingIntroductions.splice(0)) {
    // Connect via the ticket
    const res = await node.fetch(`iroh://${intro.ticket}/hello`, {
      method: 'POST',
      body: JSON.stringify({ introducedBy: intro.introducerNodeId }),
    });

    if (res.ok) {
      const bobNodeId = decodeTicket(intro.ticket).nodeId;
      knownPeers.set(bobNodeId, {
        publicKey: await fetchPublicKey(node, bobNodeId),
        via: intro.introducerNodeId,
        note: intro.note,
      });
    }
  }
}
```

## Trust transitivity

An introduction vouches for connectivity, not character. Decide how much
trust to extend:

```ts
function trustForIntroduced(intro: Introduction): TrustTier {
  const introducerTrust = myTrustFor(intro.introducerNodeId);
  // One level below the introducer — trust decays each hop
  const tiers: TrustTier[] = ['relayed', 'direct', 'lan'];
  const idx = tiers.indexOf(introducerTrust);
  return tiers[Math.max(0, idx - 1)];
}
```

This implements a simple web of trust: the more trusted the introducer, the
more trust you extend to the introduced. Trust doesn't amplify — it can only
decay across hops.

## Mutual introduction

When two parties don't know each other's ticket at all, a third party can
facilitate a rendezvous:

```ts
// Carol knows both Alice and Bob
// She sends each of them the other's introduction simultaneously

async function rendezvous(
  carol: IrohNode,
  carolKey: SecretKey,
  aliceNodeId: string,
  aliceTicket: string,
  bobNodeId: string,
  bobTicket: string,
) {
  await Promise.all([
    introduce(carol, carolKey, aliceNodeId, bobTicket,  'Meet Bob'),
    introduce(carol, carolKey, bobNodeId,   aliceTicket, 'Meet Alice'),
  ]);
  // Alice and Bob can now connect directly — Carol is no longer needed
}
```

## Failure modes

- **Introducer offline when you try to connect**: the introduction contains
  the subject's ticket (addresses + node ID). You can connect to the subject
  directly — the introducer is not needed after delivery.
- **Stale ticket**: tickets encode direct addresses that may have changed.
  If the connection fails, resolve via mDNS or relay using just the node ID
  extracted from the ticket.
- **Forged introduction**: a malicious peer intercepts and replaces the
  introduction payload. The signature check (`verifyIntroduction`) on the
  recipient side catches this — a bad signature is rejected, not trusted.
- **Trust chain growing unbounded**: if you automatically accept introductions
  from anyone you know, an adversary who compromises one peer can introduce
  themselves to your entire network. Apply `trustForIntroduced()` decay and
  set a maximum chain depth.

## Threat model

**Protects against:**
- Fake introductions (each link is signed by the actual key holder)
- An attacker impersonating a known peer to make an introduction
  (iroh-node-id on the connection is the introducer's verified key)

**Does not protect against:**
- A compromised introducer making legitimate-looking introductions to
  adversaries — the signature is valid, but the introducee is malicious.
  Trust decay limits the blast radius.
- Social engineering outside the protocol — someone asking Alice to introduce
  them under false pretences.

## When not to use this pattern

If your peer set is closed and fully known in advance (a fixed list of
devices), introductions add no value. They matter when the peer graph is
open-ended and grows organically through social relationships.

## See also

- [Proximity trust](proximity-trust.md) — extend introduced peers a trust
  tier based on how you met them, not who vouched for them
- [Capability tokens](capability-tokens.md) — include a scoped token in the
  introduction so the introduced peer can act immediately without a second
  round-trip for auth
- [Sealed messages](sealed-messages.md) — send the introduction as a sealed
  message if the recipient might be offline
