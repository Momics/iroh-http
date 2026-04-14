# Capability Tokens

Issue and verify signed access tokens using the Ed25519 keys that iroh-http
already provides. No separate auth library needed.

## Token format

```
base64url( nodeId_32  ||  expiry_u64_be  ||  scope_utf8  ||  sig_64 )
```

Passed as a standard `Authorization` header:

```
Authorization: IrohToken <base64url-token>
```

## Issuing

```ts
function issueToken(secretKey: SecretKey, opts: {
  scope: string;
  expiresIn?: number;  // seconds; omit for no expiry
}): string {
  const nodeId = secretKey.publicKey.bytes;       // 32 bytes
  const expiry = opts.expiresIn
    ? BigInt(Math.floor(Date.now() / 1000) + opts.expiresIn)
    : 0n;

  const expiryBytes = new Uint8Array(8);
  new DataView(expiryBytes.buffer).setBigUint64(0, expiry, false);

  const scopeBytes = new TextEncoder().encode(opts.scope);

  // Payload = nodeId || expiry || scope
  const payload = new Uint8Array(nodeId.length + 8 + scopeBytes.length);
  payload.set(nodeId, 0);
  payload.set(expiryBytes, nodeId.length);
  payload.set(scopeBytes, nodeId.length + 8);

  const sig = secretKey.sign(payload);             // 64 bytes

  const token = new Uint8Array(payload.length + sig.length);
  token.set(payload);
  token.set(sig, payload.length);

  return btoa(String.fromCharCode(...token))
    .replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, '');
}
```

## Verifying

```ts
function verifyToken(header: string | null, opts: {
  issuer: PublicKey;
  scope: string;
}): { ok: true } | { ok: false; reason: string } {
  if (!header?.startsWith('IrohToken ')) return { ok: false, reason: 'missing' };

  const raw = Uint8Array.from(
    atob(header.slice(10).replace(/-/g, '+').replace(/_/g, '/')),
    (c) => c.charCodeAt(0),
  );

  if (raw.length < 32 + 8 + 64) return { ok: false, reason: 'malformed' };

  const sigOffset = raw.length - 64;
  const payload = raw.slice(0, sigOffset);
  const sig = raw.slice(sigOffset);

  if (!opts.issuer.verify(payload, sig)) return { ok: false, reason: 'invalid signature' };

  // Check expiry
  const expiry = new DataView(payload.buffer, payload.byteOffset + 32, 8).getBigUint64(0, false);
  if (expiry !== 0n && expiry < BigInt(Math.floor(Date.now() / 1000))) {
    return { ok: false, reason: 'expired' };
  }

  // Check scope
  const scope = new TextDecoder().decode(payload.slice(40));
  if (!opts.scope.startsWith(scope)) return { ok: false, reason: 'scope mismatch' };

  return { ok: true };
}
```

## Middleware

```ts
function requireToken(issuer: PublicKey): Middleware {
  return (next) => (req) => {
    const result = verifyToken(req.headers.get('authorization'), {
      issuer,
      scope: new URL(req.url).pathname,
    });
    if (!result.ok) {
      return new Response('Forbidden', { status: 403 });
    }
    return next(req);
  };
}
```

See [middleware.md](middleware.md) for how to compose this with other
middleware.

## Notes

- Verification is zero-round-trip — no database, no network call. The
  signature proves the token was issued by the holder of `secretKey` without
  contacting anyone.
- The transport also authenticates the peer's identity via `Peer-Id`.
  For many use cases, that alone is sufficient — tokens add scope/expiry
  control on top of identity.
- Revocation requires short expiry + reissuance. There is no revocation list
  in this pattern.

## Failure modes

- **Clock skew between issuer and verifier**: if the clocks differ by more
  than the token lifetime the token will appear expired. Use short expiry
  (≤5 min) and let clients re-request; or include an `issuedAt` field and
  accept tokens within a ±30 s window of the server's clock.
- **Token interception**: a token presented over iroh is already encrypted
  by the QUIC layer. Between systems (e.g. a token shared via QR code), treat
  it like a password — it grants access to anyone who holds it.
- **Scope mismatch false positives**: the scope check is a path prefix
  (`startsWith`). A token for `/api` allows `/api-admin`. Use trailing slashes
  or exact-match for sensitive paths: scope `/api/` not `/api`.

## Threat model

**Protects against:**
- Unauthenticated access (token required, signature verified)
- Token forgery (Ed25519 — computationally infeasible without the secret key)
- Expired tokens (expiry field enforced on every request)

**Does not protect against:**
- Token theft — a valid, unexpired token grants access regardless of who
  holds it. Combine with `Peer-Id` header check to bind a token to a
  specific peer.
- Revocation before expiry — there is no revocation list. Use short expiry
  (≤15 min) for sensitive operations. For immediate revocation, rotate
  the issuing secret key (see [key-rotation.md](key-rotation.md)).

## When not to use this pattern

If the only clients are peers you already trust by node ID (you know exactly
who they are and the QUIC layer proves it), tokens add friction for no
security gain. Use tokens when you need scope or expiry control on top of
identity, or when you want to issue access to a peer you haven't met yet.

## Dependencies

Requires [sign/verify helpers](../features/sign-verify.md) (Patch 25).
