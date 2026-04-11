# Recipes

Practical patterns built on top of iroh-http. Each recipe is self-contained:
a problem, the iroh-specific insight, and working code.

These are **not** part of the core library — they are illustrations of what
the primitives enable. Copy, adapt, and build.

The most interesting recipes are the ones where **removing the server is the
whole point**: devices that communicate directly, trust derived from physical
proximity, state that lives on the edge and syncs peer-to-peer.

---

## Decentralized patterns

These are patterns that only make sense without a central server. The P2P
transport is the feature, not just the delivery mechanism.

- [Local-first sync](local-first-sync.md) — two devices on the same LAN
  exchange changes directly via mDNS discovery and `GET`/`PUT`; no cloud
  required, cloud optional
- [Device handoff](device-handoff.md) — encode a node ID in a QR code or
  deep link; transfer state (clipboard, file, session token) device-to-device
  with no intermediary; scan → connect → done
- [Proximity trust](proximity-trust.md) — a peer discovered via mDNS gets a
  wider permission scope than one reached through a relay; "on my LAN" is
  meaningful signal; no VPN, no allowlist
- [Cooperative backup](cooperative-backup.md) — ask three trusted peers to
  each hold a copy of a blob; verify with a signed manifest; reconstruct from
  any two; no cloud storage account needed
- [Offline-first with peer sync](offline-first.md) — buffer writes locally
  while peers are unreachable; when they reappear (mDNS or reconnect), replay
  the queue; merge conflicts with a last-write-wins or CRDT strategy
- [Reverse ingress](reverse-ingress.md) — a device behind CGNAT (Raspberry
  Pi, home server, ESP32 with a companion) serves content to the internet
  without port forwarding, a static IP, or a tunneling service

---

## Identity and social graph

These patterns deal with the fundamental decentralized identity problem: who
are you, how do I know it's really you, and how do I find your other devices?

- [Multi-device identity](multi-device-identity.md) — one identity key, many
  device node IDs; cryptographically link all your devices so peers recognise
  you on any of them
- [Peer exchange](peer-exchange.md) — signed introductions; when Alice knows
  both you and Bob, she can introduce you cryptographically — Bob knows Alice
  vouched for you
- [Presence](presence.md) — know which peers are online right now; LAN
  presence via mDNS is near-instant; WAN presence via heartbeat; surface
  latency tier in the UI

---

## Messaging

- [Sealed messages](sealed-messages.md) — encrypt a message to a peer's
  public key; they decrypt it later, even if offline; inbox nodes relay
  ciphertexts without reading them; the P2P equivalent of email encryption
- [Group messaging](group-messaging.md) — fan out messages to multiple peers;
  chat, pub/sub, collaborative sync

---

## Data and distribution

- [Content routing](content-routing.md) — fetch from the nearest peer that
  has a blob; peers that have already downloaded re-serve automatically;
  origin load stays constant regardless of audience size
- [Cooperative backup](cooperative-backup.md) — ask three trusted peers to
  each hold a copy of a blob; verify with a signed manifest; reconstruct from
  any two; no cloud storage account needed
- [Signed caching](signed-caching.md) — cache responses with unforgeable
  Ed25519 ETags; revalidate with cryptographic certainty; tamper-evident
  intermediate caches

---

## Infrastructure patterns

These use iroh-http as a transport layer where the P2P identity and
hole-punching still add something conventional HTTPS cannot.

- [HTTP gateway](http-gateway.md) — expose any local HTTP service to the iroh
  network without port forwarding; includes the IoT/ESP32 pattern
- [Peer fallback](peer-fallback.md) — try a primary peer, fall back to
  secondaries; build resilient multi-peer fetch with `Promise.any` racing
- [Offline-first with peer sync](offline-first.md) — buffer writes locally
  while peers are unreachable; when they reappear, replay the queue; merge
  with last-write-wins or CRDTs

---

## Security patterns

- [Capability tokens](capability-tokens.md) — issue and verify signed access
  tokens using iroh-http's Ed25519 key primitives; zero-round-trip
  verification, no token database
- [Middleware](middleware.md) — compose rate limiting, auth, and logging into
  a serve handler using a two-line `compose()` helper

---

## What belongs here vs. in core

**Recipes** — logic that lives in a handler, middleware, or a thin wrapper
package. Could be written by any user given iroh-http's primitives.

**Core** — features that require Rust-level stream interception or protocol
negotiation (compression, framing, trailers, identity injection). See
[guidelines.md](../guidelines.md#3-primitives-not-policies).
