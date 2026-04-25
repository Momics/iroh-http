## Reporting a Vulnerability

Please do **not** open a public issue for security vulnerabilities.

Use GitHub's private vulnerability reporting flow:

- **Security** tab → **Report a vulnerability**

If private reporting is unavailable, contact maintainers and include enough
detail to reproduce the issue safely.

## Response Targets

- Initial response within **72 hours**
- Critical issue patch or mitigation within **14 days**

## Key Material — Node Identity

Every `IrohEndpoint` has a 32-byte Ed25519 private key that is its permanent
cryptographic identity. `secretKeyBytes()` / `secret_key_bytes()` returns this
value directly so it can be persisted and later reloaded via `NodeOptions.secretKey`.

**Treat these bytes exactly like a password or root certificate private key.**

### Rules

| Rule | Rationale |
|------|-----------|
| Encrypt at rest | Store in a system keychain, hardware secure enclave, or secrets vault — never in plaintext config files or databases |
| Never log | Debug formatters, tracing spans, and generic error handlers are the most common accidental leak vectors |
| Never include in error payloads or analytics | Any serialised error that reaches a logging system or third-party analytics service permanently exposes the key |
| Zeroize after use | The returned `Uint8Array` / `Vec<u8>` / `[u8; 32]` is not zeroed on drop; overwrite with zeros once you have written to encrypted storage |
| No key rotation / revocation | If the key leaks, the node's identity is permanently compromised — there is no way to rotate or revoke it |

### Key-persistence workflow

```
// 1. Obtain key bytes immediately after endpoint creation
const { keypair, endpointHandle, nodeId } = await createEndpoint(opts);

// 2. Encrypt and persist to secure storage (example — use your platform keychain)
await secureStorage.set('iroh-node-key', encrypt(keypair));

// 3. Zeroize the in-memory buffer
keypair.fill(0);

// --- Later, on startup ---

// 4. Retrieve and decrypt
const keypair = decrypt(await secureStorage.get('iroh-node-key'));

// 5. Pass back to createEndpoint — node identity is restored
const { endpointHandle } = await createEndpoint({ secretKey: keypair, ... });

// 6. Zeroize immediately
keypair.fill(0);
```

### What NOT to do

```
// ❌ Do NOT log key material
console.log('endpoint info:', JSON.stringify(endpointInfo));   // leaks keypair

// ❌ Do NOT store in plaintext
fs.writeFileSync('config.json', JSON.stringify(endpointInfo)); // leaks keypair

// ❌ Do NOT include in error reports
Sentry.captureException(err, { extra: { endpointInfo } });     // leaks keypair
```

## Key Revocation — Current Limitation

iroh-http has **no built-in key revocation mechanism**. An Ed25519 keypair is a
permanent node identity for the lifetime that peers choose to trust it.

**What this means in practice:**

If a node's private key is compromised, the attacker can impersonate that node
to any peer that still has its public key on an allowlist. There is no
certificate authority, revocation list, or key-rotation protocol to push a
"this key is no longer valid" signal to peers automatically.

**Immediate mitigation steps when a key is compromised:**

1. Generate a new keypair and start a fresh endpoint.
2. Out-of-band, notify all peers that trusted the old public key and have them
   remove it from their allowlists and add the new public key.
3. Destroy all persisted copies of the old private key.

**Future mitigation (roadmap):**

The capability-token system described in
[`docs/adr/002-capability-url-system.md`](docs/adr/002-capability-url-system.md)
will provide short-lived, revocable, scoped tokens that can be invalidated
without rotating the underlying node identity.  Until that system is available,
operators must manage revocation out-of-band via allowlist updates.

See [`docs/threat-model.md`](docs/threat-model.md) for a full description of
the security properties iroh-http provides and does not provide.

