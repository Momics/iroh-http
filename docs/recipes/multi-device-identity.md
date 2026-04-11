# Multi-Device Identity

One person, multiple devices. Each device has its own node ID, but they all
represent the same identity. A peer who trusts you on your laptop should
recognise you on your phone — cryptographically, without a server.

## The insight

A node ID is a device identity, not a human identity. In iroh-http, the
private key lives in the device; there's no account to log into. This is a
strength — no server to breach — but also a problem: if you get a new phone,
nobody recognises it.

The solution is a two-level key hierarchy. Your **identity key** is a
long-term Ed25519 key pair you control (kept backed up, possibly in a
hardware device). Your **device keys** are node IDs. The identity key signs
each device key, producing a **device certificate**. Any peer can verify that
a device belongs to you by checking the certificate against your identity
public key.

```
Identity key (yours)
  │
  ├── signs ──► Device cert: laptop node ID
  ├── signs ──► Device cert: phone node ID
  └── signs ──► Device cert: home server node ID
```

## Identity certificate

```ts
interface DeviceCert {
  identityPub: string;  // hex — the human identity's public key
  deviceNodeId: string; // hex — this device's iroh node ID
  label: string;        // e.g. "MacBook Pro", "Pixel 9"
  issuedAt: number;     // Unix ms
  sig: string;          // base64url — identity key signs the above fields
}
```

## Issuing a certificate

Done once per device, typically during device setup:

```ts
function issueDeviceCert(
  identitySecretKey: SecretKey,  // your identity key — keep this safe
  deviceNodeId: string,
  label: string,
): DeviceCert {
  const payload: Omit<DeviceCert, 'sig'> = {
    identityPub: identitySecretKey.publicKey.toHex(),
    deviceNodeId,
    label,
    issuedAt: Date.now(),
  };
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  return {
    ...payload,
    sig: signToBase64Url(identitySecretKey, bytes),
  };
}

// On first run of a new device:
const cert = issueDeviceCert(myIdentityKey, node.nodeId(), 'iPhone 17');
```

## Serving the certificate

Each device publishes its own certificate so peers can verify it:

```ts
node.serve({}, async (req) => {
  if (req.method === 'GET' && new URL(req.url).pathname === '/.well-known/device-cert') {
    return Response.json(myDeviceCert);
  }
  // ...
});
```

## Verifying a peer's identity

When you connect to a new device node ID, fetch and verify its certificate
against the identity public key you already trust:

```ts
async function resolvePeerIdentity(
  node: IrohNode,
  deviceNodeId: string,
  trustedIdentities: Map<string, string>, // identityPub hex → human name
): Promise<string | null> {
  try {
    const res = await node.fetch(
      `iroh://${deviceNodeId}/.well-known/device-cert`,
      { signal: AbortSignal.timeout(3000) },
    );
    if (!res.ok) return null;

    const cert: DeviceCert = await res.json();

    // Verify the signature
    const { sig, ...payload } = cert;
    const bytes = new TextEncoder().encode(JSON.stringify(payload));
    const identityPub = fromHex(cert.identityPub);
    const publicKey = PublicKey.fromBytes(identityPub);
    if (!publicKey.verify(bytes, fromBase64Url(sig))) return null;

    // Check if this identity is in our trusted set
    const name = trustedIdentities.get(cert.identityPub);
    if (!name) return null;

    return name; // e.g. "Alice"
  } catch {
    return null;
  }
}
```

## Device list

The identity key also signs a **device manifest** — the canonical list of all
active devices. Peers can fetch the manifest to discover all of your devices
at once:

```ts
interface DeviceManifest {
  identityPub: string;
  devices: { nodeId: string; label: string; addedAt: number }[];
  updatedAt: number;
  sig: string;
}

function signManifest(
  identityKey: SecretKey,
  devices: DeviceManifest['devices'],
): DeviceManifest {
  const payload = {
    identityPub: identityKey.publicKey.toHex(),
    devices,
    updatedAt: Date.now(),
  };
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  return { ...payload, sig: signToBase64Url(identityKey, bytes) };
}
```

Store the manifest somewhere your devices can reach — your home server, your
most reliable device, or any cooperative peer. Devices publish its location
in their cert as `manifestNodeId`.

## Adding a new device

1. Generate the new device's node ID (first launch).
2. Fetch the current manifest from another device you own.
3. Add the new node ID to the device list.
4. Sign the updated manifest with the identity key.
5. Publish the new manifest.

No server, no account recovery flow, no email. The identity key IS the
account.

## Revoking a device

Remove the node ID from the manifest and re-sign. Peers who check the
manifest will stop trusting the removed device. Peers who don't check won't
know — this is eventual consistency. For sensitive contexts, set `expiresAt`
on the manifest and require peers to re-verify periodically.

## Bootstrapping: where is the manifest?

The hardest problem — how does a new peer find the manifest if they only know
your identity public key?

Options in increasing complexity:
1. **Out-of-band**: share the manifest node ID alongside your identity public
   key (e.g. in a QR code that encodes both).
2. **Well-known LAN peer**: when you're on the same network, mDNS discovery
   + certificate check resolves the identity, and any device can serve the
   manifest.
3. **Designated anchor**: one of your devices (home server, always-on Pi) serves
   as the permanent manifest host; its node ID is the stable reference. See
   [reverse ingress](reverse-ingress.md).

## See also

- [Sealed messages](sealed-messages.md) — seal to the identity public key so
  all your devices can open the message, not just one
- [Device handoff](device-handoff.md) — when setting up a new device, use
  handoff to transfer the identity key material securely
- [Cooperative backup](cooperative-backup.md) — back up the identity key and
  manifest across trusted peers; losing the identity key means losing the
  account
