---
id: "002"
title: "URLs as capability references"
status: rejected
date: 2026-04-13
resolved: 2026-04-25
area: api | identity
tags: [capability, url-scheme, httpi, auth, delegation]
---

# [002] URLs as capability references

## Context

In standard HTTP, a URL is an address: it locates a resource on a server
reachable over DNS and TCP. In iroh-http, a URL encodes a node's public key —
making it closer to an *unforgeable reference* than an address. You cannot
guess or forge a valid `httpi://` URL without knowing the target peer's key.

This is the defining property of an *object-capability* system: possession of
a reference grants the right to use it. iroh-http may already be a capability
URL system without having been designed as one. This exploration asks whether
that is intentional and what the implications are.

> **Rejected.** An `httpi://` URL is an address with cryptographic identity,
> not a capability. See [Decisions](#decisions).

## Questions

1. Is a `httpi://` URL a capability — i.e. does possession of it grant access,
   and is that the *intended* access-control model?
2. Should the system support *attenuation*: URLs that carry additional
   constraints (read-only, rate-limited, time-bounded) derivable from a root
   key?
3. Should URLs be *delegatable* — can peer A hand a valid `httpi://` URL to
   peer B in a way that doesn't require peer B to know A's key?
4. What is the relationship between `httpi://` URLs and the ticket system
   already described in the features docs?

## What we know

- The `httpi://` scheme encodes the node key in the host position. Knowing the
  URL is sufficient to attempt a connection — there is no separate
  authentication credential.
- The features docs already describe a ticket concept. Tickets may overlap
  with or be an instance of the capability URL idea.
- Object-capability systems (e.g. OCAP, Spritely, Cap'n Proto) have
  well-developed theory on attenuation, delegation, and revocation that could
  inform design.
- Possessing a URL only tells you *who* to talk to — it says nothing about
  *what you're allowed to do*. The URL is an address with identity, not a
  capability token.
- Access control, authorization, and permissions are application-level
  concerns. Users can layer JWT tokens, signed requests, or other mechanisms
  on top of iroh-http. The library does not and should not handle this.
- Sharing an `httpi://` URL with someone is equivalent to telling them how to
  find a peer on the network. Nothing more.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Treat URLs as pure addresses, add auth separately | Familiar HTTP mental model | Misses the unique property of peer-key addressing |
| Embrace capability semantics, document explicitly | Principled access-control story | Requires users to think in capability terms |
| Build attenuation via signed URL tokens | Enables delegation without extra infra | Increases protocol complexity |

## Decisions

**Q1 — Is an `httpi://` URL a capability?** No. Possession of the URL grants
the ability to *contact* a peer, not to *access* anything. The peer's server
handler decides what to allow. This is the same as knowing an IP address in
traditional HTTP — it gets you to the door, not through it.

**Q2 — Should URLs carry constraints (attenuation)?** No. URLs should not
carry permissions, rate limits, or time bounds. Users who need capability-based
access control should use application-level mechanisms (JWT, signed tokens,
custom headers). iroh-http provides the transport; authorization is not its
concern.

**Q3 — Should URLs be delegatable?** They already are, trivially — anyone can
share an `httpi://` URL string. But sharing it only shares the address, not
any privilege. This is by design.

**Q4 — Relationship between URLs and tickets:** An `httpi://` URL contains
only the base32-encoded public key (the peer's identity). A *ticket* contains
the public key **plus** addressing information (direct addresses, relay URLs).
The URL says *who*; the ticket says *who and how to reach them*. They are
complementary concepts, not overlapping.

The question of whether tickets should be embeddable in `httpi://` URLs (e.g.
as the domain, or via query parameters) was considered and deferred. The
current design keeps URLs clean (`httpi://<pubkey>/path`) and treats tickets
as an external resolution step: resolve a ticket to get pubkey + addresses,
then construct the URL. This avoids leaking infrastructure details (IP
addresses) into application-layer URLs.

## Implications

- Authentication and authorization are explicitly out of scope for iroh-http.
  This should be documented clearly in the README and specification.
- The capability-tokens recipe in `docs/recipes/` remains valid as a
  *user-level pattern*, not a library feature.
- URL leakage is not a security issue (unlike true capability URLs) — it only
  reveals a peer's public identity, which may already be public.

## Next steps

- [x] Review the tickets feature spec and capability-tokens recipe for overlap
  — they are complementary, not conflicting.
- [x] Decide whether to claim capability semantics — rejected.
- [x] Evaluate whether attenuatable URL tokens are in scope for v1 — no.
