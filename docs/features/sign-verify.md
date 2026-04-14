# Sign / Verify / Encrypt

`SecretKey` and `PublicKey` are the cryptographic primitives of iroh-http.
Every node has an Ed25519 identity keypair. The same keys that authenticate
the transport are available for signing, verifying, and sealing messages at the
application layer.

## Importing the key classes

`PublicKey` and `SecretKey` are exported from each adapter package:

```ts
// Node.js
import { createNode, PublicKey, SecretKey } from "@momics/iroh-http-node";

// Deno
import { createNode, PublicKey, SecretKey } from "@momics/iroh-http-deno";

// Tauri
import { createNode, PublicKey, SecretKey } from "@momics/iroh-http-tauri";
```

A node's own keys are also accessible directly:

```ts
const node = await createNode();
const sk: SecretKey  = node.secretKey;   // Ed25519 secret key
const pk: PublicKey  = node.publicKey;   // Ed25519 public key (= node ID)
```

## Sign

Sign arbitrary bytes with a `SecretKey`. Returns a 64-byte Ed25519 signature.

```ts
const data = new TextEncoder().encode("hello iroh");
const sig: Uint8Array = await node.secretKey.sign(data);
```

`SecretKey.generate()` creates a standalone key that is not tied to a node:

```ts
const key = SecretKey.generate();
const sig = await key.sign(data);
```

## Verify

Verify a signature against any `PublicKey`. Returns `false` rather than
throwing on an invalid signature.

```ts
// Verify using the sender's known node ID:
const senderKey = PublicKey.fromString(senderNodeId);
const ok: boolean = await senderKey.verify(data, sig);

// Or using the public key already on a node object:
const ok = await node.publicKey.verify(data, sig);
```

## Encrypt / Decrypt

`publicKey.encrypt` seals a message so that only the holder of the matching
`SecretKey` can open it. Uses a sealed-box construction:
Ed25519→X25519 key conversion, ephemeral ECDH, HKDF-SHA256 key derivation,
AES-GCM-256 authenticated encryption.

```ts
// Encrypt to a recipient public key (e.g. their node ID):
const recipient = PublicKey.fromString(recipientNodeId);
const ciphertext: Uint8Array = await recipient.encrypt(plaintext);

// Decrypt with the matching secret key:
const plaintext: Uint8Array = await node.secretKey.decrypt(ciphertext);
```

The ciphertext format is self-contained: `[32B ephemeral pub] [12B IV] [ciphertext + 16B tag]`.
`decrypt` throws `IrohError` if authentication fails.

## Types summary

| Value | Type | Description |
|---|---|---|
| `sig` | `Uint8Array` (64 bytes) | Ed25519 signature |
| `ciphertext` | `Uint8Array` | Sealed-box ciphertext (≥ 60 bytes overhead) |
| `publicKey.verify` result | `boolean` | `false` on invalid sig, never throws |
| `secretKey.decrypt` result | `Uint8Array` | Plaintext, or throws on auth failure |

All cryptographic operations are **async** — always `await` them.

## Platform support

| Feature | Node / Deno / Tauri | Python |
|---------|:---:|:---:|
| **Sign** (`secretKey.sign`) | ✅ class method | ✅ `secret_key_sign(key, data)` module function |
| **Verify** (`publicKey.verify`) | ✅ class method | ✅ `public_key_verify(key, data, sig)` module function |
| **Generate key** (`SecretKey.generate`) | ✅ class method | ✅ `generate_secret_key()` module function |
| **Encrypt** (`publicKey.encrypt`) | ✅ | ❌ not implemented |
| **Decrypt** (`secretKey.decrypt`) | ✅ | ❌ not implemented |

> **Python note:** Sign/verify/generate are module-level functions accepting
> raw `bytes` keys, not class methods on `PublicKey`/`SecretKey` objects.
> This matches PyO3 conventions where lightweight wrappers expose the
> underlying Rust functions directly.

## Python

Python uses module-level functions with `bytes` arguments — there are no
`PublicKey`/`SecretKey` class objects. The node's keys are accessible as plain bytes.

```python
from iroh_http import sign, verify, encrypt, decrypt, generate_secret_key

# Sign with the node's own secret key:
sig: bytes = sign(node.secret_key, data)

# Verify against any peer's public key (derive bytes from their node ID):
import base64
def node_id_to_bytes(node_id: str) -> bytes:
    pad = (8 - len(node_id) % 8) % 8
    return base64.b32decode(node_id.upper() + "=" * pad)

ok: bool = verify(node_id_to_bytes(peer_node_id), data, sig)

# Generate a standalone key (not tied to a node):
key: bytes = generate_secret_key()  # 32 bytes

# Encrypt a message to a peer:
ciphertext: bytes = encrypt(node_id_to_bytes(recipient_node_id), plaintext)

# Decrypt with own secret key:
plaintext: bytes = decrypt(node.secret_key, ciphertext)
```

`verify` returns `False` on an invalid signature — it does not raise.
`decrypt` raises `ValueError` on authentication failure.

**Cross-platform interop:** The ciphertext format is identical between JS and
Python. A message encrypted with `encrypt(...)` in Python can be decrypted with
`await node.secretKey.decrypt(ciphertext)` in Node/Deno/Tauri and vice versa.

## What to avoid

**JS:** Do not use the lower-level `secretKeySign` / `publicKeyVerify` functions
that older adapter versions exported. Those take raw `Uint8Array` keys instead of
typed class instances, are inconsistently available across adapters, and are
removed in the current API. Use the class methods above instead.

**Python:** `secret_key_sign` and `public_key_verify` are deprecated aliases for
`sign` and `verify`. They still work but emit `DeprecationWarning`. Use `sign` and
`verify` instead.

## See also

- [sealed-messages](../recipes/sealed-messages.md) — encrypt messages for offline delivery
- [capability-tokens](../recipes/capability-tokens.md) — signed access tokens
- [witness-receipts](../recipes/witness-receipts.md) — tamper-evident audit logs
