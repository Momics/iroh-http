# The iroh-http Ecosystem

What happens when many iroh-http nodes exist and talk to each other? Individual
recipes solve individual problems. This document asks what *emerges* at the
network level — properties that no single node has alone.

---

## The node as a citizen

Every iroh-http node is simultaneously a client, a server, and a peer. It has
a stable cryptographic identity (its node ID), can be reached by anyone who
knows that ID, and can reach anyone it knows. There is no privileged tier —
no load balancer, no API gateway, no CDN that must exist for the system to
work.

From this flat topology, structure emerges bottom-up:

```
                        [node D]──────[node E]
                       /                     \
[node A]──────[node B]                       [node F]
              │         \                   /
              │          [node C]──────────
              │
         (LAN segment)
         [node G]──[node H]
```

Every edge is a direct iroh connection. Every edge is authenticated. Every
node is aware of its immediate neighbours. No node has a complete map of the
network — and none needs one.

---

## Emergent properties

### 1. Locality without configuration

LAN nodes find each other via mDNS. Traffic between co-located nodes never
leaves the network segment. No configuration required — the physics of the
network encode the topology.

**What this means at scale**: a neighbourhood mesh, a campus network, a ship's
intranet, a factory floor — all have sub-millisecond iroh connections between
their nodes without a single router advertisement or DHCP reservation.

### 2. Resilience without redundancy infrastructure

Each node holds whatever data it has fetched or been asked to hold. When a
node goes offline, its data is wherever it put copies. The network routes
around holes — not because it was engineered for it, but because every
connection is direct and every node that has data can serve it.

**What this means at scale**: with [content routing](content-routing.md) and
[cooperative backup](cooperative-backup.md), popular or critical data
automatically replicates toward the edges. The more a piece of content is
used, the more copies exist.

### 3. Trust as a graph property

Trust is not binary. [Peer exchange](peer-exchange.md) propagates introductions
one hop at a time, with trust decaying at each hop. [Proximity trust](proximity-trust.md)
anchors high trust to physical location. [Capability attenuation](capability-attenuation.md)
lets trust be delegated without ever being amplified.

**What this means at scale**: the trust graph is isomorphic to the social
graph. People who know each other well are close in the network. Strangers
require witnesses or token chains. This is how human trust works — iroh-http
just makes it cryptographically verifiable.

### 4. Identity without accounts

A node ID is a key, not a handle. [Multi-device identity](multi-device-identity.md)
lets one person own many devices. [Named nodes](named-nodes.md) let
human-readable names resolve to node IDs without a registrar. [Peer exchange](peer-exchange.md)
distributes those name bindings to the nodes that care.

**What this means at scale**: there is no "forgot my password" — the identity
key is the account. There is no "username taken" — names are scoped to trust
groups. There is no "account deleted" — the identity persists as long as at
least one peer remembers it.

### 5. Coordination without coordinators

[Witness receipts](witness-receipts.md), [append-only logs](append-only-log.md),
and [threshold custody](threshold-custody.md) allow two or more nodes to make
binding cryptographic commitments to each other without a notary, escrow
service, or smart contract platform. The signature is the record.

**What this means at scale**: small-group coordination (a team, a family, a
co-op) can have strong guarantees — audit trails, shared secrets, witnessed
exchanges — without institutional intermediaries.

---

## The full stack

Each layer is optional but each one unlocks the next:

```
┌─────────────────────────────────────────────────────────┐
│  Applications                                           │
│  (sync, messaging, file sharing, compute, media)        │
├─────────────────────────────────────────────────────────┤
│  Coordination                                           │
│  witness-receipts · threshold-custody · append-only-log │
├─────────────────────────────────────────────────────────┤
│  Communication                                          │
│  presence · sealed-messages · group-messaging           │
├─────────────────────────────────────────────────────────┤
│  Data                                                   │
│  content-routing · cooperative-backup · signed-caching  │
├─────────────────────────────────────────────────────────┤
│  Trust                                                  │
│  proximity-trust · capability-attenuation · peer-exchange│
├─────────────────────────────────────────────────────────┤
│  Identity                                               │
│  multi-device-identity · named-nodes                    │
├─────────────────────────────────────────────────────────┤
│  Transport (iroh-http core)                             │
│  QUIC · hole-punch · relay · TLS · node ID              │
└─────────────────────────────────────────────────────────┘
```

You don't have to build all layers. Many useful applications live one or two
layers above the transport. But each layer you add makes your application more
capable and more independent of centralised infrastructure.

---

## A worked example: the neighbourhood mesh

Imagine ten people on a street, each running an iroh-http node on their home
server or laptop. They start with nothing configured — just the library
running.

**Day 1:** mDNS fires. Nodes discover each other. They exchange introductions
via [peer exchange](peer-exchange.md). Each node learns its neighbours'
node IDs and stores them.

**Week 1:** They set up [cooperative backup](cooperative-backup.md). Each
person's important documents are stored on three neighbours' nodes. No
cloud subscription.

**Month 1:** They add [named nodes](named-nodes.md). "alice" resolves to
Alice's node ID within the group. They use [sealed messages](sealed-messages.md)
to send private notes to each other by name.

**Month 3:** They set up [content routing](content-routing.md). When one person
downloads a large software update, everyone else on the street gets it from
that node via LAN. Their collective internet bill drops.

**Month 6:** They add [threshold custody](threshold-custody.md) to their
shared document vault. Opening shared files requires three of the ten
neighbours to be online and consent. No single point of failure.

**Year 1:** The group has a coordination layer. [Witness receipts](witness-receipts.md)
handle informal agreements (borrowing equipment). [Append-only logs](append-only-log.md)
give each member an auditable history of shared resource usage. The network
is self-governing.

Nobody installed a server. Nobody created an account. Nobody paid a monthly
fee. The infrastructure is the community of devices.

---

## What makes this different from previous attempts

Federated social networks (ActivityPub, Matrix) require servers. Blockchain
systems require global consensus. BitTorrent requires trackers or a DHT.
Traditional VPNs require a gateway.

iroh-http is different because:

1. **HTTP semantics are universal.** Any developer already knows `fetch()` and
   `Response`. The learning curve is the transport layer, not the application
   model.
2. **Connection establishment is solved.** Hole-punching and relay fallback
   mean "behind CGNAT" is not a special case — it's just a peer.
3. **Identity is the transport.** The node ID is not separate from the
   connection. You don't verify identity after connecting — you connect *to*
   an identity.
4. **No DHT, no global state.** The network has no shared data structure that
   all nodes must maintain. Each node only tracks what it cares about.

---

## Open questions worth building toward

- **Name resolution without a root**: how do you bootstrap name discovery for
  a stranger? (Right now: out-of-band, like a QR code. Better: signed name
  records propagated by mutual friends.)
- **Peer reputation without a ledger**: how do you know which peers reliably
  serve what they promise? (Signed delivery receipts, stored locally.)
- **Incentivised relay**: relay nodes today are altruistic. A micropayment or
  reciprocity system could make the relay network self-sustaining without
  a company running it.
- **Network-partitioned consistency**: when the network splits (an island, a
  ship, a festival WiFi), each partition continues to operate. When it
  rejoins, conflicts are resolved. iroh-http handles the transport; the
  application layer needs CRDTs or append-only logs to handle the data.
- **Autonomous nodes**: nodes that aren't human devices but persistent
  services — a shared calendar, a build server, a community search index —
  operated collectively, with costs and benefits distributed across their
  operators.

---

## Where to start

If you're building toward this vision, the order that makes sense:

1. Start with [local-first sync](local-first-sync.md) — get two devices
   talking without a server.
2. Add [multi-device identity](multi-device-identity.md) — make your identity
   portable.
3. Add [peer exchange](peer-exchange.md) — grow the network organically.
4. Add [presence](presence.md) — know when to sync vs. queue.
5. Add [cooperative backup](cooperative-backup.md) — make data durable.
6. Add [capability attenuation](capability-attenuation.md) — control what
   each peer can do.
7. Add [append-only logs](append-only-log.md) — make the history auditable.

Each step is useful in isolation. Together they compose into something that
has no central point of failure, no company that can shut it down, and no
account to lose access to.

---

## When this ecosystem is not the right answer

The P2P model is not universally better — it's better for specific tradeoffs.

**Use a conventional server when:**
- You need a global public namespace (search engines, public APIs, social
  media feeds that strangers discover). iroh names are scoped to groups.
- You need globally consistent state that all participants must agree on
  simultaneously (financial ledgers, auction systems). P2P with CRDTs
  gives eventual consistency, not immediate consistency.
- Your users control nothing — they're end consumers of content from a
  single authoritative source. One server is simpler.
- Your team controls all nodes and updates them in lockstep on a reliable
  internal network. Just use HTTPS.

**The P2P ecosystem pays off when:**
- Data must not leave users' devices under any circumstances
- Devices span multiple networks without static IPs or port forwarding
- The peer set grows through social trust rather than central onboarding
- Resilience to network partitions matters more than consistency
- You are building for communities, not for consumers
- No company should be in a position to shut it down

The recipes in this collection reflect that second set of constraints. Every
recipe that asks "why iroh?" has an answer that only makes sense if you're
optimising for those tradeoffs. If you're not, a REST API over HTTPS is the
right tool.
