---
id: "002"
title: "URLs as capability references"
status: open
date: 2026-04-13
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

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Treat URLs as pure addresses, add auth separately | Familiar HTTP mental model | Misses the unique property of peer-key addressing |
| Embrace capability semantics, document explicitly | Principled access-control story | Requires users to think in capability terms |
| Build attenuation via signed URL tokens | Enables delegation without extra infra | Increases protocol complexity |

## Implications

- Directly affects how authentication and authorization are documented and
  built on top of iroh-http.
- Overlaps with the tickets feature, sign/verify feature, and
  capability-tokens recipe.
- If URLs are capabilities, then URL leakage is a security issue — worth
  noting in threat model docs.

## Next steps

- [ ] Review the tickets feature spec and capability-tokens recipe for overlap.
- [ ] Decide whether to explicitly claim capability semantics in the protocol
  docs or leave it implicit.
- [ ] Evaluate whether signed, attenuatable URL tokens are in scope for v1.
