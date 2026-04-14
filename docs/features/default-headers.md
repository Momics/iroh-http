# Default Headers

## What

iroh-http automatically injects and manages certain HTTP headers on every request and response. These carry metadata derived from the underlying QUIC connection — information the application layer cannot forge, modify, or omit.

## Injected headers

### `Peer-Id` (request header, server-side)

Every incoming `Request` delivered to a `serve` handler carries a `Peer-Id` header containing the **verified public key** of the sending peer.

```ts
node.serve({}, (req) => {
  const peerId = req.headers.get('Peer-Id');
  // peerId is the base32-encoded Ed25519 public key of the caller.
  // It is cryptographically guaranteed by the QUIC connection — not spoofable.
  return Response.json({ peer: peerId });
});
```

This header is injected by the Rust layer from the authenticated connection identity. The remote cannot set or spoof it — it is stripped from the wire before the handler sees the request, then re-injected from the verified connection state. A handler can rely on `Peer-Id` for access control without any additional authentication step.

The value is the same string returned by `node.publicKey.toString()` on the sending node.

### `Peer-Id` (request header, client-side)

When `node.fetch` sends a request, the Rust layer injects `Peer-Id: <local-node-id>` into the outgoing headers. The server can use this to know who is calling without the client having to set it manually.

Because the header value is derived from the authenticated key, it matches the identity the server will see in its own `Peer-Id` — both sides see the same verified identity.

## Diagnostic data

Relay status and round-trip time are accessible programmatically via
`node.peerStats()` (see [observability](observability.md)).  Automatic injection
of `iroh-relay` and `iroh-rtt-ms` headers is not yet implemented — this is
tracked for a future release.

## What is NOT injected

- `Host` — iroh-http is not a host-based protocol. The peer is identified by public key, not by a domain name.
- `User-Agent` — not injected by default. Callers can add it via `init.headers` if needed.
- `Content-Type` — never assumed. Callers must set it explicitly when sending structured bodies.
- `Authorization` — never touched. Callers manage their own auth (see `capability-tokens.md`).

## Security note

Because `Peer-Id` is unforgeable, it is correct and safe to use it as the basis for access control decisions:

```ts
const ALLOWED = new Set(['abc123...', 'def456...']);

node.serve({}, (req) => {
  const peer = req.headers.get('Peer-Id')!;
  if (!ALLOWED.has(peer)) {
    return new Response('Forbidden', { status: 403 });
  }
  return handleRequest(req);
});
```

No HMAC, no token, no session cookie needed — the transport layer already did the authentication.
