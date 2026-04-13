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
- The rate-limiting feature doc describes per-connection rate limiting; it is
  unclear whether limits can be applied per peer key rather than per
  connection.
- Because peers have stable identities, per-key blocking and reputation-based
  access control are technically feasible and have no equivalent in traditional
  HTTP servers.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Document threat model, rely on existing limits | Low effort; honest | Peer-specific vectors may be under-addressed |
| Add per-peer-key rate limiting and blocking | Strong defence; uses unique P2P property | Requires persistent state; more complex |
| Add a peer allowlist/denylist API | Simple to use for closed networks | Doesn't help for open or semi-open networks |
| Formal threat model document | Clarity for users and contributors | Time investment; needs expert review |

## Implications

- Relevant to the rate-limiting and server-limits feature specs.
- Per-key blocking would be a meaningful capability that traditional HTTP
  servers cannot offer — worth considering as a differentiator.
- A documented threat model is required before promoting iroh-http for
  security-sensitive use cases.

## Next steps

- [ ] List the peer-specific attack vectors not addressed by the current
  server-limits feature.
- [ ] Evaluate whether per-peer-key rate limiting is feasible with the
  current connection pool architecture.
- [ ] Draft a threat model section for inclusion in `docs/principles.md` or
  as a standalone security doc.
