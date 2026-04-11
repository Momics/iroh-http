# Sealed Messages

Encrypt a message to a peer's public key. They can decrypt it later, even if
they were offline when you sent it. No server can read the content, and the
recipient needs no pre-shared secret.

## The insight

iroh's transport layer authenticates both sides — you know exactly who you're
talking to. But that authentication is ephemeral: the QUIC session keys are
discarded after the connection closes. Sealing a message to a public key is
different: it produces a ciphertext that only the holder of the corresponding
private key can ever open, regardless of when, where, or through how many
intermediaries it travels.

This is the P2P equivalent of email encryption — but without a mail server,
a key server, or PGP's web of trust ceremony. You already have the recipient's
public key: it's their node ID.

```
Sender                               Recipient
  │                                      │
  │  seal(message, recipient.publicKey)  │
  │  → ciphertext                        │
  │                                      │
  │──── POST /inbox/{recipientId} ──────►│  (or via intermediary)
  │                                      │
  │                                      │  open(ciphertext, mySecretKey)
  │                                      │  → message
```

## Key conversion: Ed25519 → X25519

iroh node IDs are Ed25519 keys. Authenticated Diffie-Hellman encryption uses
X25519. The conversion is deterministic and well-specified (RFC 8032 §5.1.5).

```ts
// Requires: @noble/curves (or similar — any library that exposes the
// ed25519-to-x25519 point transformation)
import { edwardsToMontgomeryPub, edwardsToMontgomeryPriv } from '@noble/curves/ed25519';

function toX25519Public(ed25519Pub: Uint8Array): Uint8Array {
  return edwardsToMontgomeryPub(ed25519Pub);
}

function toX25519Private(ed25519Priv: Uint8Array): Uint8Array {
  return edwardsToMontgomeryPriv(ed25519Priv);
}
```

## Sealing

Use X25519 Diffie-Hellman + AES-GCM (the standard ECIES construction):

```ts
async function seal(
  plaintext: Uint8Array,
  recipientEdPub: Uint8Array,  // ed25519 public key bytes (32 bytes)
): Promise<Uint8Array> {
  const recipientX = toX25519Public(recipientEdPub);

  // Ephemeral key pair — never reused
  const ephemPriv = crypto.getRandomValues(new Uint8Array(32));
  const ephemPub  = x25519.getPublicKey(ephemPriv);

  // Shared secret
  const sharedSecret = x25519.getSharedSecret(ephemPriv, recipientX);

  // Derive encryption key
  const keyMaterial = await crypto.subtle.importKey(
    'raw', sharedSecret, 'HKDF', false, ['deriveKey'],
  );
  const key = await crypto.subtle.deriveKey(
    { name: 'HKDF', hash: 'SHA-256', salt: ephemPub, info: new Uint8Array() },
    keyMaterial,
    { name: 'AES-GCM', length: 256 },
    false, ['encrypt'],
  );

  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = new Uint8Array(
    await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, plaintext),
  );

  // Layout: ephemPub (32) || iv (12) || ciphertext
  const out = new Uint8Array(32 + 12 + ciphertext.length);
  out.set(ephemPub, 0);
  out.set(iv, 32);
  out.set(ciphertext, 44);
  return out;
}
```

## Opening

```ts
async function open(
  sealed: Uint8Array,
  myEdPriv: Uint8Array,  // ed25519 secret key bytes (32 bytes)
): Promise<Uint8Array> {
  const ephemPub   = sealed.slice(0, 32);
  const iv         = sealed.slice(32, 44);
  const ciphertext = sealed.slice(44);

  const myXPriv = toX25519Private(myEdPriv);
  const sharedSecret = x25519.getSharedSecret(myXPriv, ephemPub);

  const keyMaterial = await crypto.subtle.importKey(
    'raw', sharedSecret, 'HKDF', false, ['deriveKey'],
  );
  const key = await crypto.subtle.deriveKey(
    { name: 'HKDF', hash: 'SHA-256', salt: ephemPub, info: new Uint8Array() },
    keyMaterial,
    { name: 'AES-GCM', length: 256 },
    false, ['decrypt'],
  );

  return new Uint8Array(
    await crypto.subtle.decrypt({ name: 'AES-GCM', iv }, key, ciphertext),
  );
}
```

## Inbox server

Peers can leave sealed messages for each other through an inbox node — a
device that's usually online (home server, cloud VM, another peer). The inbox
stores ciphertexts without being able to read them.

```ts
// Inbox node — just holds ciphertexts
const inbox = new Map<string, Uint8Array[]>(); // recipientId → sealed messages

node.serve({}, async (req) => {
  const url = new URL(req.url);
  const match = url.pathname.match(/^\/inbox\/([0-9a-f]+)$/);
  if (!match) return new Response('Not Found', { status: 404 });
  const recipientId = match[1];

  if (req.method === 'POST') {
    const sealed = new Uint8Array(await req.arrayBuffer());
    if (!inbox.has(recipientId)) inbox.set(recipientId, []);
    inbox.get(recipientId)!.push(sealed);
    return new Response(null, { status: 204 });
  }

  if (req.method === 'GET') {
    const messages = inbox.get(recipientId) ?? [];
    inbox.delete(recipientId); // fetch-and-clear
    const body = JSON.stringify(messages.map((m) => btoa(String.fromCharCode(...m))));
    return new Response(body, { headers: { 'Content-Type': 'application/json' } });
  }

  return new Response('Method Not Allowed', { status: 405 });
});
```

The inbox node has zero knowledge of message contents. It's a dumb relay for
opaque bytes.

## Sending a sealed message

```ts
async function sendSealed(
  node: IrohNode,
  inboxNodeId: string,
  recipientNodeId: string,
  plaintext: Uint8Array,
): Promise<void> {
  // recipientNodeId is 64-char hex — decode to 32 bytes
  const recipientPub = hexToBytes(recipientNodeId);
  const sealed = await seal(plaintext, recipientPub);

  await node.fetch(`iroh://${inboxNodeId}/inbox/${recipientNodeId}`, {
    method: 'POST',
    body: sealed,
  });
}
```

## Fetching and decrypting

```ts
async function fetchMessages(
  node: IrohNode,
  inboxNodeId: string,
  myNodeId: string,
  mySecretKeyBytes: Uint8Array,
): Promise<Uint8Array[]> {
  const res = await node.fetch(`iroh://${inboxNodeId}/inbox/${myNodeId}`);
  const encoded: string[] = await res.json();

  return Promise.all(
    encoded.map((b64) => {
      const sealed = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      return open(sealed, mySecretKeyBytes);
    }),
  );
}
```

## Direct delivery

When the recipient is online, skip the inbox entirely — send directly:

```ts
// Recipient runs this
node.serve({}, async (req) => {
  if (req.method === 'POST' && new URL(req.url).pathname === '/message') {
    const sealed = new Uint8Array(await req.arrayBuffer());
    const plaintext = await open(sealed, mySecretKeyBytes);
    handleMessage(plaintext);
    return new Response(null, { status: 204 });
  }
  return new Response('Not Found', { status: 404 });
});

// Sender:
// Try direct delivery first; fall back to inbox
async function deliver(node: IrohNode, recipientId: string, msg: Uint8Array) {
  const sealed = await seal(msg, hexToBytes(recipientId));
  try {
    const res = await node.fetch(`iroh://${recipientId}/message`, {
      method: 'POST', body: sealed,
      signal: AbortSignal.timeout(3000),
    });
    if (res.ok) return;
  } catch { /* offline */ }
  // Fall back to inbox
  await sendSealed(node, INBOX_NODE_ID, recipientId, msg);
}
```

## Properties

- **Forward secrecy per message**: each seal uses a fresh ephemeral key.
  Compromising the recipient's long-term key doesn't expose old messages.
- **Authentication**: the recipient knows the message came from someone who
  knew their public key — combine with `sign-verify` to also prove sender
  identity.
- **Inbox blindness**: the relay node cannot read, modify, or correlate
  message contents — only recipient IDs.

## Failure modes

- **Inbox node offline**: the sender retries later via the
  [offline-first](offline-first.md) outbox pattern, or tries direct delivery
  first and falls back to an inbox.
- **Inbox node lost**: all undelivered messages are lost along with the inbox.
  Mitigate by using multiple inbox nodes and sending to all of them (only
  one delivery per recipient is needed).
- **Key compromise before opening**: if the recipient's node key is
  compromised, the attacker can open sealed messages. Key rotation
  ([key-rotation.md](key-rotation.md)) limits the blast radius — rotate the
  key, notify senders, messages sealed to old key are inaccessible.
- **Replay**: a message body can be re-submitted to the inbox or re-POSTed
  to the recipient's `/message` endpoint. Add a nonce inside the sealed
  payload and deduplicate by nonce on the receiver side.

## Threat model

**Protects against:**
- Inbox node reading or modifying message content (encrypted to recipient's key)
- Network observers reading messages in transit (QUIC + encryption layer)
- Spoofing the recipient's node ID (iroh QUIC authenticates node IDs)

**Does not protect against:**
- Sender identity — anyone who knows the recipient's public key can seal a
  message. Add a sender signature inside the sealed payload for authorship
  proof (see [sign-verify](../features/sign-verify.md)).
- Metadata: the inbox node knows who is messaging whom (sender node ID →
  recipient node ID). Use onion routing or a blind-relay pattern if metadata
  privacy is required.
- Recipient's node key being compromised — sealed messages are only as
  private as the recipient's private key storage.

## When not to use this pattern

For synchronous communication between online peers, a direct `POST` to the
recipient's `/message` endpoint is simpler. Sealed messages (with an inbox)
are for async delivery: the sender doesn't know if the recipient is online,
or the message needs to survive hours or days before delivery.

## See also

- [Sign/verify feature](../features/sign-verify.md) — add a sender signature
  inside the sealed payload to prove authorship
- [Device handoff](device-handoff.md) — sealed messages enable handoff even
  when the receiver isn't online at the same time
- [Multi-device identity](multi-device-identity.md) — seal to an identity
  key, not a device key, so any of the recipient's devices can open it
