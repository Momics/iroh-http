# Witness Receipts

When two nodes exchange something — a file, a payment promise, an agreement —
a third node witnesses and counter-signs. Both parties receive cryptographic
proof of the exchange. Disputes can be resolved by presenting the receipt.

## The insight

Contracts require witnesses. Traditionally that means a notary, an escrow
service, a bank. In a P2P network, any mutually trusted third node can serve
this role — your home server, a friend's device, a community witness node.

The witness doesn't need to understand what's being exchanged. They only need
to attest to two facts: "Alice made this offer at this time" and "Bob accepted
at this time." Their Ed25519 signature is the notarial stamp.

```
Alice                    Witness                    Bob
  │── (1) OFFER ─────────►│                          │
  │                        │── (2) OFFER forwarded ──►│
  │                        │◄── (3) ACCEPTANCE ───────│
  │◄── (4) SIGNED RECEIPT ─┤── (4) SIGNED RECEIPT ──►│
  │                        │                          │
  └─────────── cryptographic proof of exchange ───────┘
```

## Receipt format

```ts
interface ExchangeOffer {
  from: string;       // proposer nodeId
  to: string;         // counterparty nodeId
  witness: string;    // witness nodeId
  description: string;// human-readable: "transfer of file abc.txt"
  payloadHash: string;// sha256 of the exchanged payload — not the payload itself
  expiresAt: number;  // offer lapses if not accepted by this time
  nonce: string;      // random, prevents replay
  sig: string;        // proposer's signature over the above
}

interface ExchangeAcceptance {
  offerHash: string;  // sha256 of the canonical ExchangeOffer
  from: string;       // acceptor nodeId
  acceptedAt: number;
  sig: string;        // acceptor's signature
}

interface WitnessReceipt {
  offer: ExchangeOffer;
  acceptance: ExchangeAcceptance;
  witnessedAt: number;
  witness: string;    // witness nodeId
  sig: string;        // witness signature over (offer + acceptance + witnessedAt)
}
```

## Alice: making an offer

```ts
async function makeOffer(
  secretKey: SecretKey,
  opts: {
    to: string;
    witness: string;
    description: string;
    payload: Uint8Array;
    expiresIn: number; // seconds
  },
): Promise<ExchangeOffer> {
  const payloadHash = await sha256hex(opts.payload);
  const offer: Omit<ExchangeOffer, 'sig'> = {
    from: secretKey.publicKey.toHex(),
    to: opts.to,
    witness: opts.witness,
    description: opts.description,
    payloadHash,
    expiresAt: Date.now() + opts.expiresIn * 1000,
    nonce: crypto.randomUUID(),
  };
  const bytes = new TextEncoder().encode(JSON.stringify(offer));
  return { ...offer, sig: signToBase64Url(secretKey, bytes) };
}
```

## Witness node

The witness receives offers, forwards them, collects acceptances, and issues
signed receipts:

```ts
const pendingOffers = new Map<string, ExchangeOffer>(); // offerHash → offer

function serveWitness(node: IrohNode, secretKey: SecretKey) {
  node.serve({}, async (req) => {
    const url = new URL(req.url);

    // POST /witness/offer — Alice submits an offer
    if (req.method === 'POST' && url.pathname === '/witness/offer') {
      const offer: ExchangeOffer = await req.json();
      if (!await verifyOffer(offer)) {
        return new Response('Invalid offer signature', { status: 400 });
      }
      if (offer.expiresAt < Date.now()) {
        return new Response('Offer expired', { status: 410 });
      }

      const hash = await sha256json(offer);
      pendingOffers.set(hash, offer);

      // Forward to Bob
      await node.fetch(`iroh://${offer.to}/inbox/offer`, {
        method: 'POST',
        body: JSON.stringify(offer),
        headers: { 'Content-Type': 'application/json' },
      });

      return Response.json({ offerHash: hash });
    }

    // POST /witness/accept — Bob submits acceptance
    if (req.method === 'POST' && url.pathname === '/witness/accept') {
      const acceptance: ExchangeAcceptance = await req.json();
      const offer = pendingOffers.get(acceptance.offerHash);
      if (!offer) return new Response('Offer not found', { status: 404 });
      if (offer.expiresAt < Date.now()) {
        pendingOffers.delete(acceptance.offerHash);
        return new Response('Offer expired', { status: 410 });
      }
      if (!await verifyAcceptance(acceptance, offer)) {
        return new Response('Invalid acceptance signature', { status: 400 });
      }

      // Issue receipt
      const witnessedAt = Date.now();
      const receiptPayload = { offer, acceptance, witnessedAt, witness: node.nodeId() };
      const bytes = new TextEncoder().encode(JSON.stringify(receiptPayload));
      const receipt: WitnessReceipt = {
        ...receiptPayload,
        sig: signToBase64Url(secretKey, bytes),
      };

      pendingOffers.delete(acceptance.offerHash);

      // Send to both parties
      await Promise.all([
        node.fetch(`iroh://${offer.from}/inbox/receipt`, {
          method: 'POST',
          body: JSON.stringify(receipt),
          headers: { 'Content-Type': 'application/json' },
        }),
        node.fetch(`iroh://${offer.to}/inbox/receipt`, {
          method: 'POST',
          body: JSON.stringify(receipt),
          headers: { 'Content-Type': 'application/json' },
        }),
      ]);

      return Response.json(receipt);
    }

    return new Response('Not Found', { status: 404 });
  });
}
```

## Bob: accepting an offer

```ts
async function acceptOffer(
  node: IrohNode,
  secretKey: SecretKey,
  offer: ExchangeOffer,
): Promise<WitnessReceipt> {
  const offerHash = await sha256json(offer);
  const acceptance: Omit<ExchangeAcceptance, 'sig'> = {
    offerHash,
    from: secretKey.publicKey.toHex(),
    acceptedAt: Date.now(),
  };
  const bytes = new TextEncoder().encode(JSON.stringify(acceptance));
  const signed: ExchangeAcceptance = { ...acceptance, sig: signToBase64Url(secretKey, bytes) };

  const res = await node.fetch(`iroh://${offer.witness}/witness/accept`, {
    method: 'POST',
    body: JSON.stringify(signed),
    headers: { 'Content-Type': 'application/json' },
  });

  return res.json();
}
```

## Verifying a receipt later

Any node — including a future arbitrator — can verify the receipt:

```ts
async function verifyReceipt(
  receipt: WitnessReceipt,
  witnesses: Map<string, PublicKey>,  // trusted witness nodeIds → keys
): Promise<boolean> {
  const witnessKey = witnesses.get(receipt.witness);
  if (!witnessKey) return false;

  const { sig, ...payload } = receipt;
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  return witnessKey.verify(bytes, fromBase64Url(sig));
}
```

## Use cases

- **Equipment lending**: "I borrowed Alice's camera on April 11" — signed by
  Alice, accepted by me, witnessed by our shared friend. Neither party can
  later deny it.
- **Shared resource scheduling**: "This data centre slot is reserved for Bob
  from 14:00–16:00" — the community witness node signs the booking.
- **Content delivery proof**: "I sent you this file at 09:32, hash abc123" —
  the recipient's acceptance confirms delivery without a read-receipt server.
- **Promise tracking**: "I will share my 10 GB bandwidth quota with you this
  month" — reciprocity commitments in a bandwidth-sharing cooperative.

## Choosing a witness

The witness is a node both parties trust to be honest and available. Options:
- A community node run by a neutral party (a local co-op, a nonprofit)
- A friend both parties know
- A node belonging to one party but constrained to only forward and sign,
  not to alter (the code is open-source and auditable)

Unlike a notary, the witness can't forge the content — only attest to who
offered what and who accepted. The payload hash proves the content without
requiring the witness to see it.

## See also

- [Append-only log](append-only-log.md) — store receipts in an append-only
  log for a durable, auditable history of all exchanges
- [Capability attenuation](capability-attenuation.md) — include an attenuated
  token in the offer payload so acceptance grants access automatically
- [Ecosystem overview](ecosystem.md) — witness receipts are the coordination
  layer that enables informal agreements and resource-sharing in a community
  mesh
