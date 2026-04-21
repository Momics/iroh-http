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

