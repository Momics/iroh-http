# Recipes

Practical patterns built on top of iroh-http. Each recipe is self-contained:
a problem, the iroh-specific insight, and working code.

These are **not** part of the core library — they are illustrations of what
the primitives enable. Copy, adapt, and build.

The most interesting recipes are the ones where **removing the server is the
whole point**: devices that communicate directly, trust derived from physical
proximity, state that lives on the edge and syncs peer-to-peer.

For the big picture — what emerges when many nodes are connected — start with
the [Ecosystem overview](ecosystem.md).

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

## Identity lifecycle

- [Key rotation and recovery](key-rotation.md) — planned device rotation,
  emergency revocation of compromised keys, catastrophic recovery from
  threshold custody; the part of identity management that's hardest to
  improvise in a crisis

---

## Coordination patterns

These patterns emerge at the network level — what nodes can do *together* that
none can do alone.

- [Append-only log](append-only-log.md) — each node maintains a signed,
  tamper-evident history; subscribers replay it to derive state; the
  foundation for audit trails, collaborative docs, and distributed databases
- [Witness receipts](witness-receipts.md) — a third node counter-signs a
  two-party exchange; both parties receive cryptographic proof; disputes
  resolved without institutions
- [Threshold custody](threshold-custody.md) — split a secret key across N
  peers (k-of-n Shamir); no single peer can act alone; reconstruct from any k;
  applied to key recovery, shared vaults, dead man's switches
- [Capability attenuation](capability-attenuation.md) — delegate a *subset*
  of your permissions; each hop can only restrict, never expand; chains are
  verifiable without contacting the root issuer (object-capability model)
- [Named nodes](named-nodes.md) — claim a human-readable name by signing it;
  peers store and relay the mapping; scoped to a group, no registrar required
- [Schema and version negotiation](schema-negotiation.md) — peers on
  different versions of your protocol coexist; new nodes speak the richer
  protocol with each other and the older protocol with unupgraded peers;
  rolling upgrades with no coordination required

---

## Compute and distribution

- [Job dispatch](job-dispatch.md) — peers advertise spare CPU/GPU capacity;
  clients submit render, transcode, index, or inference jobs; results stream
  back; no job queue server, no cloud function platform
- [Release channels](release-channels.md) — sign a release with your node key;
  subscribers follow the append-only release log; peers propagate the archive
  so the origin barely has to serve anyone; no CDN, no package registry
- [Capability advertisement](capability-advertisement.md) — peers announce
  not just presence but *what they offer*: storage, compute, inbox, gateway;
  others discover matching peers dynamically; the service-discovery layer
  that makes the full ecosystem self-organising

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

- [Capability tokens](capability-tokens.md) — single-hop signed access tokens;
  start here before building attenuation chains
- [Middleware](middleware.md) — compose rate limiting, auth, and logging into
  a serve handler using a two-line `compose()` helper

---

## When not to use any of this

iroh-http adds value when the absence of a central server is the point. It
adds complexity when a simple server would do.

**Reach for a conventional server when:**
- Your data is public and you want search engine indexing
- You need a global namespace that strangers can discover without out-of-band
  setup (iroh names are scoped to groups, not the internet)
- Your peer set is entirely controlled by one organisation on a reliable
  network (just use HTTPS)
- You need sub-10ms latency and both sides are in the same data centre

**The P2P approach pays off when:**
- Data must not leave the user's devices
- Devices are behind CGNAT with no static IP or port forwarding
- The peer set spans personal devices across different networks
- Resilience matters more than peak performance
- You don't want to run, pay for, or trust a central server

---

## What belongs here vs. in core

**Recipes** — logic that lives in a handler, middleware, or a thin wrapper
package. Could be written by any user given iroh-http's primitives.

**Core** — features that require Rust-level stream interception or protocol
negotiation (compression, framing, trailers, identity injection). See
[principles.md](../principles.md#4-primitives-not-policies).
