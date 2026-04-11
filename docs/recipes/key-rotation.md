# Key Rotation and Recovery

What to do when a device is lost, compromised, or retired. How to rotate keys
without losing your identity or breaking trust with peers who know you.

## The insight

In a conventional system, "reset my account" is an API call to a server that
owns your identity. In iroh-http, *you* own your identity key — which means
you also own the problem of what happens when a device holding a key is
stolen, broken, or just upgraded. There is no "forgot my password."

That sounds scary. It's actually an opportunity: a well-designed key rotation
procedure is more secure than any server-side reset flow — no helpdesk social
engineering, no email takeover, no SIM swap. But it requires planning in
advance, not crisis-mode improvisation.

The three scenarios, in order of severity:

1. **Planned rotation** — I'm replacing a device; I want the old key to stop
   working and the new key to take over cleanly.
2. **Device loss** — my laptop was stolen; I need the old key revoked before
   the attacker uses it.
3. **Catastrophic loss** — I lost every device and have no backup route in.
   The identity key must be recovered from threshold custody.

---

## Setup: what to prepare before anything goes wrong

> Most key rotation failures happen because the recovery path wasn't set up
> before the emergency. Do this at identity creation time.

### 1. Back up your identity key with threshold custody

```ts
import { splitSecret, distributeShares } from './threshold-custody'; // see threshold-custody.md

const identityKeyBytes = myIdentitySecretKey.toBytes();

await distributeShares(node, identityKeyBytes, [
  { nodeId: aliceNodeId, publicKey: alicePub },
  { nodeId: bobNodeId,   publicKey: bobPub   },
  { nodeId: homeServer,  publicKey: homeServerPub },
], 2); // any 2-of-3 can reconstruct

console.log('Identity key backed up. Any 2 of 3 custodians can recover it.');
```

See [threshold-custody.md](threshold-custody.md).

### 2. Tell your peers which key can authorise rotation

Include a `rotationPolicy` in your identity manifest:

```ts
interface RotationPolicy {
  // Node IDs that may co-sign a rotation event (quorum: any 2)
  authorisers: string[];
  // Require a witness receipt as proof of rotation? (recommended)
  requireWitness: boolean;
}
```

Store this in your device manifest (see [multi-device-identity.md](multi-device-identity.md)).
Peers who cache your manifest will know how to verify a rotation claim.

---

## Scenario 1: planned device rotation

You're replacing `old-laptop` with `new-laptop`. Both devices are available.

```ts
interface RotationEvent {
  type: 'rotation';
  identityPub: string;     // your long-term identity key (unchanged)
  oldDeviceNodeId: string; // retiring this node ID
  newDeviceNodeId: string; // new node ID to trust going forward
  reason: 'planned' | 'loss' | 'compromise';
  rotatedAt: number;
  sig: string;             // identity key signs the above
}

function signRotation(
  identityKey: SecretKey,
  oldNodeId: string,
  newNodeId: string,
  reason: RotationEvent['reason'],
): RotationEvent {
  const payload: Omit<RotationEvent, 'sig'> = {
    type: 'rotation',
    identityPub: identityKey.publicKey.toHex(),
    oldDeviceNodeId: oldNodeId,
    newDeviceNodeId: newNodeId,
    reason,
    rotatedAt: Date.now(),
  };
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  return { ...payload, sig: signToBase64Url(identityKey, bytes) };
}
```

Broadcast the event to all known peers:

```ts
async function broadcastRotation(
  node: IrohNode,
  event: RotationEvent,
  knownPeers: string[],
) {
  await Promise.allSettled(
    knownPeers.map((peer) =>
      node.fetch(`iroh://${peer}/identity/rotation`, {
        method: 'POST',
        body: JSON.stringify(event),
        headers: { 'Content-Type': 'application/json' },
      }),
    ),
  );
}

// On old-laptop: sign, broadcast, then stop the node
const event = signRotation(identityKey, oldNode.nodeId(), newNodeId, 'planned');
await broadcastRotation(oldNode, event, allPeers);
await oldNode.close();
```

### Receiving and verifying a rotation

```ts
async function handleRotation(
  event: RotationEvent,
  knownIdentities: Map<string, { publicKey: PublicKey; devices: string[] }>,
): Promise<boolean> {
  const identity = [...knownIdentities.values()]
    .find((i) => i.publicKey.toHex() === event.identityPub);
  if (!identity) return false; // don't know this identity

  // Verify the identity key signed this
  const { sig, ...payload } = event;
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  if (!identity.publicKey.verify(bytes, fromBase64Url(sig))) return false;

  // Update local device list
  identity.devices = identity.devices.filter((d) => d !== event.oldDeviceNodeId);
  if (event.newDeviceNodeId) identity.devices.push(event.newDeviceNodeId);

  console.log(`Rotation verified: ${event.oldDeviceNodeId} → ${event.newDeviceNodeId}`);
  return true;
}
```

---

## Scenario 2: compromised or lost device, identity key safe

The device is gone but you have access to the identity key (on another device
or recovered from threshold custody). Steps:

1. **Sign a rotation event with `reason: 'loss'` and no `newDeviceNodeId`** —
   this tells peers to stop trusting the old node ID immediately.
2. **Add the new device** with a second rotation event.
3. **Reissue any outstanding capability tokens** — old tokens signed by the
   lost device key should be treated as potentially compromised.

```ts
// Step 1: revoke old device, add no replacement yet
const revoke = signRotation(identityKey, lostDeviceNodeId, '', 'loss');
// newDeviceNodeId = '' means "remove from trusted list, add nothing"

// Step 2: add new device
const add = signRotation(identityKey, '', newDeviceNodeId, 'planned');

await broadcastRotation(newNode, revoke, allPeers);
await broadcastRotation(newNode, add, allPeers);
```

### Capability token reissuance

Any token that included the old device's node ID in its `holder` chain should
be re-signed. If you don't know who holds those tokens, shorten the expiry on
all root grants and let them re-request:

```ts
// Emergency: tighten expiry on all root capability grants
// (existing tokens expire in ≤1 hour regardless of original grant)
function emergencyExpiryTighten(capability: AttenuatedToken): AttenuatedToken {
  return attenuate(capability, identityKey, capability.root.holder, {
    expiresAt: Date.now() + 3600_000,
  });
}
```

---

## Scenario 3: catastrophic loss — recovering the identity key

All devices are lost. Identity key was backed up with threshold custody.

```ts
// 1. Reconstruct the identity key from custodians
const tempNode = await IrohNode.spawn();
const identityKeyBytes = await recover(tempNode, mySecretKey, custodians, 2);
const identityKey = SecretKey.fromBytes(identityKeyBytes);

// 2. Generate a completely new device
const newDeviceNode = await IrohNode.spawn();

// 3. Sign a rotation that removes all old devices and adds the new one
// (you don't know which devices the attacker may have)
const catastrophicReset: RotationEvent = {
  type: 'rotation',
  identityPub: identityKey.publicKey.toHex(),
  oldDeviceNodeId: '*',  // wildcard: revoke ALL previously known devices
  newDeviceNodeId: newDeviceNode.nodeId(),
  reason: 'compromise',
  rotatedAt: Date.now(),
  sig: '',
};
const bytes = new TextEncoder().encode(JSON.stringify({ ...catastrophicReset, sig: undefined }));
catastrophicReset.sig = signToBase64Url(identityKey, bytes);

// 4. Broadcast to all peers — stored in contacts, reach via relay
await broadcastRotation(newDeviceNode, catastrophicReset, knownPeerIds);
```

Peers receiving `oldDeviceNodeId: '*'` clear their entire device list for
this identity and start fresh with just the new device.

---

## Trust decay after rotation

Peers who received a rotation event should treat the period between the
device's last verified contact and the rotation timestamp as a risk window —
any actions attributed to that device during that window are potentially
attacker-authored.

```ts
interface RotationRecord {
  event: RotationEvent;
  receivedAt: number;
  // Actions logged by old device between (receivedAt - uncertainty) and rotatedAt
  // should be flagged for human review if they're sensitive
  riskWindowMs: number;
}
```

A conservative policy: refuse to honour any action from the old device that
was signed after the rotation timestamp, and flag any action from the 24h
before the rotation for review.

---

## Threat model

**Protects against:**
- A lost or stolen device being used to impersonate you after rotation
- An attacker who only has the device key (not the identity key) issuing
  new capability grants in your name

**Does not protect against:**
- An attacker who has both the device key *and* the identity key
- Rotation events not reaching peers (network partition during broadcast) —
  mitigated by repeat broadcast, but gaps exist
- Peers who cached a stale manifest and don't check for rotation events

**Mitigations:**
- Keep the identity key on a hardware token or in threshold custody — never
  stored on a device that travels
- Use short expiry on capability tokens so even a compromised device's grants
  expire quickly
- Peers should revalidate manifests periodically (e.g. on every new session)

---

## Bootstrapping

**Starting empty:** at identity creation, generate the identity key, generate
the first device node ID, issue a device cert (see
[multi-device-identity.md](multi-device-identity.md)), and immediately set up
threshold custody. Without custody, catastrophic loss has no recovery path.

**Degraded (no custodians available):** rotation requires the identity key.
If it's inaccessible, the identity is effectively frozen. Design UX to make
"set up recovery custodians" a required onboarding step, not an optional one.

---

## When not to use this pattern

If your application has no persistent identity across sessions — anonymous or
ephemeral nodes — key rotation is unnecessary overhead. This pattern is only
meaningful when a node ID represents a durable human identity that peers
accumulate trust for over time.

---

## See also

- [Multi-device identity](multi-device-identity.md) — the device manifest
  that rotation events modify
- [Threshold custody](threshold-custody.md) — how the identity key is backed
  up for catastrophic recovery
- [Capability attenuation](capability-attenuation.md) — the tokens that need
  reissuance after a compromise rotation
- [Witness receipts](witness-receipts.md) — use a witness to co-sign and
  timestamp a rotation event, creating an externally verifiable record
