# Per-Peer Rate Limiting

## `ServeOptions.maxConnectionsPerPeer`

iroh-http enforces a Rust-level hard cap on simultaneous connections from any
one peer, applied before JavaScript runs. This is the DoS baseline.

```ts
node.serve({ maxConnectionsPerPeer: 3 }, handler);
```

When a peer exceeds the limit, the excess connection is **closed at the QUIC
level** — no JS overhead, no `Request` object created. The remote receives a
transport-level close rather than an HTTP response (the connection was never
fully upgraded to HTTP).

## Application-level rate limiting

For request-level rate limiting (e.g., token bucket per peer), implement
middleware in your application or use a community middleware package. Serve
handlers are plain functions `(req: Request) => Response`, so any middleware
pattern that wraps this signature composes cleanly. See
[recipes/middleware.md](../recipes/middleware.md) for a token-bucket example.

## Notes

- `maxConnectionsPerPeer` prevents connection floods at the transport level.
- `maxConcurrency` (total in-flight requests, all peers) is a separate
  `ServeOptions` field.
- The `Peer-Id` header (injected by the Rust layer on every request) provides
  a verified peer identity for application-level rate limiting.
