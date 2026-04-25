---
id: "008"
title: "Adversarial peer threat model"
status: open
date: 2026-04-13
area: security
tags: [security, threat-model, adversarial, peer, dos, malformed-http]
---

# [008] Adversarial peer threat model

## Context

A normal HTTP server's threat model centres on anonymous, potentially
malicious *clients* sending malformed or abusive requests over TCP. Defences
(rate limiting, body size limits, request timeouts) are designed with that
model in mind.

In iroh-http, the model is different. A "client" is a *peer* — a node with a
known cryptographic identity that explicitly connected to you. But a known
identity does not mean a trusted peer. A malicious actor can generate an Iroh
node key trivially, and your server will accept their connection just as readily
as any other.

The specific attack surface a peer has is different from a traditional HTTP
client. The question is whether the current defences account for those
differences.

## Questions

1. What can a malicious peer do that a malicious traditional HTTP client
   cannot?
2. Are the existing server-limits defences (documented in
   `docs/features/server-limits.md`) sufficient against peer-specific attacks?
3. Should the threat model be documented explicitly so that security-conscious
   users understand what iroh-http does and does not protect against?
4. Does the peer identity layer create any new *opportunities* for defence
   (e.g. blocking by peer key, reputation, rate-limiting per identity)?

## What we know

- Iroh peers can send malformed HTTP/1.1 over the QUIC stream; hyper will
  handle parse errors, but stream-level attacks (held connections, slow-read,
  infinite bodies) are a real concern.
- The server-limits feature doc describes connection limits, body size caps,
  and timeout configuration — these cover the most common DoS vectors.
- The rate-limiting feature doc describes per-connection rate limiting; per-peer
  rate limiting is also implemented (`maxConnectionsPerPeer`, default 8).
- Because peers have stable identities, per-key blocking and reputation-based
  access control are technically feasible and have no equivalent in traditional
  HTTP servers.
- **Shipped:** `docs/threat-model.md` documents what the transport provides
  (mutual authentication, confidentiality, integrity, replay protection) and
  what it does not (authorization, Sybil resistance).
- **Shipped:** Per-peer connection limits are enforced in `server.rs`
  (`max_connections_per_peer`, default 8). Exposed as
  `connections.maxPerPeer` in `NodeOptions`.
- **Shipped:** The tower stack includes `LoadShed`, `ConcurrencyLimit`, and
  `Timeout` middleware for resource protection.
- **Key insight:** The threat model is fundamentally similar to traditional
  HTTP with one addition: peers are cryptographically identified. This means
  you can *trust a specific peer* if you choose to, but an unknown peer with
  a valid key is no more trustworthy than an anonymous TCP client. An attacker
  can generate unlimited Ed25519 keys trivially, so per-peer rate limiting
  does not prevent Sybil-style attacks (many identities, 8 connections each).
  The threat model explicitly documents this as out of scope.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Document threat model, rely on existing limits | Low effort; honest | Peer-specific vectors may be under-addressed |
| Add per-peer-key rate limiting and blocking | Strong defence; uses unique P2P property | Requires persistent state; more complex |
| Add a peer allowlist/denylist API | Simple to use for closed networks | Doesn't help for open or semi-open networks |
| Formal threat model document | Clarity for users and contributors | Time investment; needs expert review |

## Implications

- The core threat model is documented and the most important server-side
  defences (connection limits, per-peer limits, timeouts, body size caps)
  are implemented.
- Per-key blocking (allowlist/denylist) would be a meaningful capability
  for closed or semi-open networks. This is an application-level concern
  that could be documented as a recipe.
- Sybil resistance (defending against many identities) is explicitly out
  of scope. Defending against it requires mechanisms outside the transport
  layer (proof-of-work, external identity, trust networks).
- The documented threat model is honest about what iroh-http does and does
  not protect against. This is the right approach for a library.

## Next steps

- [x] List peer-specific attack vectors — documented in `threat-model.md`.
- [x] Evaluate per-peer-key rate limiting — implemented (`maxConnectionsPerPeer`).
- [x] Draft a threat model document — shipped as `docs/threat-model.md`.
- [ ] Consider a recipe for peer allowlist/denylist patterns at the
  application level.
