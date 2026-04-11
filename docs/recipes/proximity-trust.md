# Proximity Trust

Grant more trust to peers discovered on your LAN than to peers reached through
a relay. Physical proximity is a meaningful signal — no VPN, no allowlist.

## The insight

iroh-http exposes how a connection was established: every request carries
`iroh-node-id` (who) and the library knows whether the path was direct
(LAN/NAT traversal) or relayed (cloud relay server). A peer that is
physically on the same network segment is far less likely to be an adversary
than a random peer from the internet relay.

This lets you build a tiered access model:

```
Tier 1 — LAN-discovered (mDNS)   → full read+write access
Tier 2 — Direct (NAT traversal)  → read + limited write
Tier 3 — Relayed                  → read-only or token required
```

No IP allowlists. No VPN certificates. The transport layer provides the
signal.

## Reading connection provenance

```ts
function trustTier(req: Request): 'lan' | 'direct' | 'relayed' {
  // iroh-http injects this header when the direct path is known
  const via = req.headers.get('iroh-path-type');
  if (via === 'direct-lan') return 'lan';
  if (via === 'direct') return 'direct';
  return 'relayed';
}
```

> **Note:** `iroh-path-type` is injected by the
> [observability feature](../features/observability.md) (Patch 23). Until that
> ships, use the `browseEvents` set below to track which node IDs you
> discovered via mDNS.

## Middleware

```ts
type TrustTier = 'lan' | 'direct' | 'relayed';

function requireTier(minimum: TrustTier): Middleware {
  const order: TrustTier[] = ['relayed', 'direct', 'lan'];
  return (next) => (req) => {
    const tier = trustTier(req);
    if (order.indexOf(tier) < order.indexOf(minimum)) {
      return new Response('Forbidden', { status: 403 });
    }
    return next(req);
  };
}
```

## mDNS allowlist fallback

Until `iroh-path-type` is available, maintain a set of node IDs discovered
via `node.browse()` and check the request's `iroh-node-id` against it:

```ts
const lanPeers = new Set<string>();

async function trackLanPeers(node: IrohNode, signal: AbortSignal) {
  for await (const event of node.browse({ signal })) {
    if (event.type === 'found') lanPeers.add(event.nodeId);
    if (event.type === 'lost')  lanPeers.delete(event.nodeId);
  }
}

function isLanPeer(req: Request): boolean {
  const id = req.headers.get('iroh-node-id');
  return id != null && lanPeers.has(id);
}

function requireLan(): Middleware {
  return (next) => (req) => {
    if (!isLanPeer(req)) return new Response('LAN only', { status: 403 });
    return next(req);
  };
}
```

## Tiered route handler

```ts
node.serve({}, compose(
  // Public — anyone with a valid ticket
  route('GET', '/public', publicHandler),

  // Direct peers — NAT-traversed, no relay
  route('GET', '/internal', compose(
    requireTier('direct'),
    internalReadHandler,
  )),

  // LAN only — full admin access
  route('POST', '/admin', compose(
    requireLan(),
    adminHandler,
  )),
));
```

See [middleware.md](middleware.md) for `compose()` and the `route()` helper.

## Tiered capability token issuance

Proximity trust pairs naturally with short-lived tokens: when a LAN peer
connects, issue them a token with a wider scope and longer expiry than you
would for a relayed peer.

```ts
async function handleAuth(req: Request, secretKey: SecretKey): Promise<Response> {
  const tier = trustTier(req);
  const token = issueToken(secretKey, {
    scope: tier === 'lan' ? '/'        // full access
      : tier === 'direct' ? '/api'     // API only
      : '/api/read',                   // read-only
    expiresIn: tier === 'lan' ? 3600 : 300,
  });
  return Response.json({ token });
}
```

See [capability-tokens.md](capability-tokens.md) for `issueToken()`.

## What "LAN" means here

mDNS discovery only works within a single broadcast domain — the same WiFi
network or wired subnet. It does not traverse routers. A peer that appears in
`node.browse()` events is, by definition, on the same link-local network
segment. That is considerably stronger than "same ISP" or "same city" and is
a reasonable proxy for physical proximity.

## Failure modes

- **mDNS blocked**: some managed networks (corporate WiFi, hotels) disable
  multicast. `lanPeers` stays empty; all peers appear as `'relayed'`. The
  safe fallback: treat unknown path type as `'relayed'` (lowest trust).
  Never default to `'lan'`.
- **LAN peer impersonation**: mDNS announces a node ID but doesn't prove
  the announcer controls that key. The QUIC connection does prove it — iroh
  verifies the node ID against the TLS certificate on handshake. An mDNS
  spoof results in a connection failure, not a trust escalation.
- **VPN tunnels**: a VPN can make a remote peer appear to be on the same
  broadcast domain. A node arriving via VPN will be announced via mDNS and
  appear as `'lan'`. If your VPN is a trusted boundary this is correct
  behaviour; if the VPN is untrusted, gate LAN trust on a separate allow-list.

## Threat model

**Protects against:**
- A random internet peer reaching LAN-only endpoints (they can't appear in
  mDNS unless they're physically on the network)
- Relay-node impersonation (relay nodes can't forge node IDs — QUIC
  authenticates end-to-end)

**Does not protect against:**
- A malicious device physically on your LAN (coffee shop WiFi, shared
  office). Physical network access = LAN trust in this model. For
  higher-security contexts, require a capability token even from LAN peers.
- An attacker who has already compromised a LAN device and is using its
  node ID — mitigated by key rotation when a device is lost.

## When not to use this pattern

If your application has no meaningful distinction between LAN and WAN
accessibility (e.g. a public content mirror), skip proximity tiers and
use capability tokens alone for access control.

## See also

- [Device handoff](device-handoff.md) — proximity trust applied to QR-code
  pairing: only grant the handoff to a LAN-discovered claimer
- [Local-first sync](local-first-sync.md) — restrict sync to LAN peers by
  default, opt in to relay sync explicitly
- [Discovery feature](../features/discovery.md) — `node.browse()` and
  `node.advertise()` API reference
