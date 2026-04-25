---
id: "001"
title: "Peer identity exposure in the fetch API"
status: accepted
date: 2026-04-13
resolved: 2026-04-25
area: api | identity
tags: [identity, fetch, public-key, verification]
---

# [001] Peer identity exposure in the fetch API

## Context

Every Iroh node has a cryptographic identity — an Ed25519 public key — baked
into the transport layer. This means a `fetch` call in iroh-http is not just
"GET this resource from this address." It is "GET this resource from this
cryptographically verified peer." That distinction is fundamental and has no
equivalent in standard HTTP.

Currently the JS/Python caller addresses a peer by node key in the URL
(`httpi://<node-key>/path`), but the response object returned by `fetch` does
not surface any identity information. The verified peer key lives only inside
the Rust core.

> **Resolved.** All three questions below have been answered through
> implementation and architectural analysis. See [Decisions](#decisions).

## Questions

1. Should the verified peer public key be surfaced to the caller on a
   successful response — and if so, where (a custom header, a response
   property, a sidecar object)?
2. Should `fetch` accept an optional assertion — "only complete this request if
   the responding peer matches this key" — so the caller can pin to an expected
   identity?
3. Is identity pinning a first-class API concern, or should it be built as a
   middleware/wrapper on top of the existing surface?

## What we know

- The QUIC handshake in Iroh authenticates the remote node; the verified public
  key is available to the Rust core after the connection is established.
- Standard `fetch` has no concept of a verified remote identity; surfacing this
  would be a deliberate extension to the familiar interface.
- The `httpi://` URL scheme already encodes the target node key; the request
  therefore implicitly pins identity. The question is whether that pinning is
  *visible* and *assertable* from the caller side.
- **Shipped:** The server injects a `Peer-Id` header on every incoming request,
  containing the authenticated peer's base32-encoded public key.
- **Shipped:** `PublicKey.fromPeerId(id)` parses a peer ID string (from the
  header) into a `PublicKey` instance. `PublicKey.prototype.toURL(path?)` builds
  an `httpi://` URL. Together they provide ergonomic round-tripping (#118).
- **Verified:** Iroh's QUIC TLS 1.3 handshake cryptographically enforces that
  the remote peer's Ed25519 public key matches the `node_id` decoded from the
  `httpi://` URL. A mismatch causes the connection to fail before any HTTP
  traffic flows. This is not opt-in — it is always enforced by the transport.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Add `x-iroh-peer-key` response header | Familiar header model, no API change | Headers are mutable; callers might not notice it |
| Extend the response object with a `peerKey` property | Explicit, typed, discoverable | Diverges from the standard `Response` interface |
| Pass an `expectedPeer` option to `fetch` | Explicit pinning at the call site | Adds coupling; how to handle mismatch — reject or error? |
| Do nothing / document that URL already pins | Zero API complexity | Invisible to callers; easy to misuse |

## Decisions

**Q1 — Surfacing peer identity:** Resolved via `Peer-Id` request header. The
server side injects the authenticated peer's public key on every inbound
request. The caller (fetch side) already knows the peer key — it's in the
URL they called — so a response-side header is unnecessary.

**Q2 — Identity assertion on fetch:** Not needed as a separate option. Iroh's
QUIC TLS 1.3 handshake enforces identity pinning at the transport layer.
`endpoint.connect(node_id, ...)` will reject any peer whose key does not match
the `node_id` from the URL. This is always-on and cryptographically enforced.

**Q3 — First-class vs. middleware:** First-class, baked into the transport.
Identity pinning is not a wrapper or opt-in feature — it is an inherent
property of every `httpi://` connection.

## Implications

- Affects all FFI surfaces (Node, Deno, Tauri) and their type definitions.
  `Peer-Id` header and `PublicKey.fromPeerId()` are in `iroh-http-shared`,
  so all adapters get them automatically.
- No wire-boundary changes were needed — the peer key flows as a standard
  HTTP header, not a custom FFI type.
- Identity verification is invisible to the caller (a strength, not a
  weakness): it is impossible to accidentally skip it.

## Next steps

- [x] Survey how other identity-aware transports surface peer identity.
- [x] Prototype identity exposure — shipped as `Peer-Id` header + `PublicKey.fromPeerId()`.
- [x] Decide whether identity assertion belongs in core — yes, at the
  transport layer (QUIC handshake), not as an application-level option.
