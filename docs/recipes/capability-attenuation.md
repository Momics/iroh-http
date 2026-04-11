# Capability Attenuation

Delegate a subset of your permissions to another node, without ever exceeding
what you were given. Each step in the chain can only restrict scope — never
expand it. A chain is verifiable without contacting the original issuer.

## The insight

[Capability tokens](capability-tokens.md) let you issue a token that proves
you authorised something. Attenuation goes further: it lets the *holder* of a
token create a *weaker* token and hand it to someone else — without your
involvement. You issued "read and write /files"; Alice gave Bob "read
/files/shared"; Bob gave Carol "read /files/shared/images". Carol's token
is verifiable, and its constraints are mathematically enforced by the chain.

This is the object-capability model: authority flows downward, never upward.
No call back to a server. No permission database. The chain IS the proof.

```
Root key                          Scope: /  (read+write)
    │ signs
    ▼
Token A (Alice)                   Scope: /files  (read+write)
    │ Alice attenuates
    ▼
Token B (Bob)                     Scope: /files/shared  (read only)
    │ Bob attenuates
    ▼
Token C (Carol)                   Scope: /files/shared/images  (read only)
    │
    └── Carol presents to your server → server verifies full chain → grants access
```

## Attenuated token format

An attenuated token is a chain: each caveat is a restriction applied on top
of the previous token. The chain is serialised as a JSON array, signed at
each step.

```ts
interface Caveat {
  scope?: string;                  // restrict URL path prefix
  methods?: ('GET'|'POST'|'PUT'|'DELETE')[];  // restrict HTTP methods
  expiresAt?: number;              // Unix ms — can only tighten deadline
}

interface ChainLink {
  caveat: Caveat;
  holder: string;          // nodeId hex of the delegate
  issuedAt: number;
  sig: string;             // base64url — previous holder signs (caveat + holder + issuedAt)
}

interface AttenuatedToken {
  root: {
    issuerNodeId: string;
    caveat: Caveat;        // root grant
    sig: string;           // root issuer signs this
  };
  chain: ChainLink[];      // attenuation steps; may be empty
}
```

## Issuing the root token

```ts
function issueRoot(
  secretKey: SecretKey,
  delegate: string,   // nodeId of the first recipient
  caveat: Caveat,
): AttenuatedToken {
  const root = {
    issuerNodeId: secretKey.publicKey.toHex(),
    caveat,
    holder: delegate,
    issuedAt: Date.now(),
  };
  const bytes = new TextEncoder().encode(JSON.stringify(root));
  return {
    root: { ...root, sig: signToBase64Url(secretKey, bytes) },
    chain: [],
  };
}
```

## Attenuating (delegating with restrictions)

```ts
function attenuate(
  token: AttenuatedToken,
  holderSecretKey: SecretKey,  // current holder signs the new link
  newDelegate: string,
  additionalCaveat: Caveat,
): AttenuatedToken {
  // Merge caveats — new constraints can only tighten existing ones
  const effective = mergeCaveats(effectiveCaveat(token), additionalCaveat);
  if (!isTighter(effective, effectiveCaveat(token))) {
    throw new Error('Attenuation can only restrict scope, never expand it');
  }

  const link: Omit<ChainLink, 'sig'> = {
    caveat: additionalCaveat,
    holder: newDelegate,
    issuedAt: Date.now(),
  };
  const bytes = new TextEncoder().encode(JSON.stringify(link));
  return {
    ...token,
    chain: [...token.chain, { ...link, sig: signToBase64Url(holderSecretKey, bytes) }],
  };
}

function effectiveCaveat(token: AttenuatedToken): Caveat {
  return token.chain.reduce(
    (acc, link) => mergeCaveats(acc, link.caveat),
    token.root.caveat,
  );
}

function mergeCaveats(a: Caveat, b: Caveat): Caveat {
  return {
    scope: mostSpecificPath(a.scope, b.scope),
    methods: a.methods && b.methods
      ? a.methods.filter((m) => b.methods!.includes(m))
      : a.methods ?? b.methods,
    expiresAt: Math.min(a.expiresAt ?? Infinity, b.expiresAt ?? Infinity) || undefined,
  };
}

function mostSpecificPath(a?: string, b?: string): string | undefined {
  if (!a) return b;
  if (!b) return a;
  // The more specific path (longer prefix) wins
  return b.startsWith(a) ? b : a.startsWith(b) ? a : a; // if unrelated, keep original
}

function isTighter(proposed: Caveat, existing: Caveat): boolean {
  const scopeOk = !proposed.scope || !existing.scope ||
    proposed.scope.startsWith(existing.scope);
  const methodsOk = !proposed.methods || !existing.methods ||
    proposed.methods.every((m) => existing.methods!.includes(m));
  const expiryOk = (proposed.expiresAt ?? Infinity) <= (existing.expiresAt ?? Infinity);
  return scopeOk && methodsOk && expiryOk;
}
```

## Verifying a chain

The server verifies each link's signature against the previous holder's
public key. The root is verified against the issuer's key. No network call
required.

```ts
async function verifyChain(
  token: AttenuatedToken,
  trustedIssuers: Map<string, PublicKey>,   // nodeIds you accept root grants from
  request: { path: string; method: string },
): Promise<boolean> {
  // 1. Verify root signature
  const issuerKey = trustedIssuers.get(token.root.issuerNodeId);
  if (!issuerKey) return false;

  const rootPayload = { ...token.root };
  delete (rootPayload as any).sig;
  if (!issuerKey.verify(
    new TextEncoder().encode(JSON.stringify(rootPayload)),
    fromBase64Url(token.root.sig),
  )) return false;

  // 2. Verify each chain link against the previous holder
  let prevHolderNodeId = token.root.issuerNodeId;
  // Track accumulated caveat
  let caveat = token.root.caveat;

  for (const link of token.chain) {
    const holderKey = await resolveKey(prevHolderNodeId);
    const linkPayload = { ...link };
    delete (linkPayload as any).sig;
    if (!holderKey.verify(
      new TextEncoder().encode(JSON.stringify(linkPayload)),
      fromBase64Url(link.sig),
    )) return false;

    caveat = mergeCaveats(caveat, link.caveat);
    prevHolderNodeId = link.holder;
  }

  // 3. Check request against effective caveat
  if (caveat.scope && !request.path.startsWith(caveat.scope)) return false;
  if (caveat.methods && !caveat.methods.includes(request.method as any)) return false;
  if (caveat.expiresAt && caveat.expiresAt < Date.now()) return false;

  return true;
}
```

## Middleware

```ts
function requireAttenuation(
  trustedIssuers: Map<string, PublicKey>,
): Middleware {
  return (next) => async (req) => {
    const header = req.headers.get('authorization');
    if (!header?.startsWith('IrohChain ')) {
      return new Response('Unauthorized', { status: 401 });
    }

    const token: AttenuatedToken = JSON.parse(
      atob(header.slice(10).replace(/-/g, '+').replace(/_/g, '/')),
    );
    const url = new URL(req.url);
    const valid = await verifyChain(token, trustedIssuers, {
      path: url.pathname,
      method: req.method,
    });

    if (!valid) return new Response('Forbidden', { status: 403 });
    return next(req);
  };
}
```

## A concrete delegation scenario

```ts
// Root: you grant Alice full access to /files
const rootToken = issueRoot(mySecretKey, aliceNodeId, {
  scope: '/files',
  methods: ['GET', 'POST', 'PUT', 'DELETE'],
  expiresAt: Date.now() + 7 * 24 * 3600 * 1000, // 1 week
});

// Alice attenuates for Bob: read-only on /files/shared
const aliceToken = attenuate(rootToken, aliceSecretKey, bobNodeId, {
  scope: '/files/shared',
  methods: ['GET'],
});

// Bob attenuates for Carol: read-only on /files/shared/images, expires in 1 hour
const bobToken = attenuate(aliceToken, bobSecretKey, carolNodeId, {
  scope: '/files/shared/images',
  expiresAt: Date.now() + 3600_000,
});

// Carol presents bobToken to your server — you verify the full chain
// and grant access to GET /files/shared/images only, for 1 hour
```

## Why this is better than ACLs

- No server round-trip to check permissions — the chain is self-contained.
- Alice and Bob can delegate without asking you — reducing coordination overhead.
- Revocation is expiry-based — adjust the root `expiresAt` to limit blast radius.
- Auditable — the chain proves exactly who delegated what to whom, and when.

## See also

- [Capability tokens](capability-tokens.md) — single-hop tokens; start here
  before building chains
- [Peer exchange](peer-exchange.md) — combine with introductions: Alice
  attenuates a token when she introduces Bob to you, so Bob can act immediately
- [Ecosystem overview](ecosystem.md) — capability attenuation is the trust
  layer that makes the full network coordination stack composable
