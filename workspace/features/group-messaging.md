---
status: not-implemented
scope: separate package — iroh-http-group
priority: low
---

# Feature: Group / Multicast Messaging

## What

A named group abstraction on top of iroh-http's point-to-point transport.
Sending to a group fans out the message to all current members. Members join
and leave dynamically. The group is identified by a shared name (or key) rather
than a single node ID.

## Why

iroh-http handles node-to-node request/response well. Many real applications
need one-to-many or many-to-many communication: chat rooms, collaborative
editors, sensor networks, distributed event streams. Doing this today requires
the application to maintain its own member list and fan out manually.

n0 already publishes `iroh-gossip`, a gossip-based publish/subscribe protocol
built on Iroh QUIC. The right approach is to expose a clean HTTP-flavoured
interface on top of it rather than reimplementing the gossip layer.

## Design

The group is modelled as a named channel. Any node can publish to the channel;
any member receives all messages published while it is subscribed.

```ts
// iroh-http-group (separate package)
import { joinGroup } from 'iroh-http-group';

const group = await joinGroup(node, {
  name: 'my-chat-room',
  // Optional: list of seed peers to bootstrap from.
  // Without seeds, discovery falls back to mDNS / DNS.
  seeds: [knownPeerId],
});

// Publish
await group.send(new TextEncoder().encode('hello everyone'));

// Subscribe
for await (const msg of group.messages()) {
  const { from, data } = msg;
  console.log(`${from}: ${new TextDecoder().decode(data)}`);
}

// Leave
await group.leave();
```

The `from` field is the sender's verified public key — unforgeable, same
guarantee as the rest of iroh-http.

## Dependencies

- `iroh-gossip` Rust crate for the gossip transport.
- `iroh-http-group` is a new package that wraps `iroh-gossip` and exposes the
  JS API above. It depends on `iroh-http-shared` for key types.
- A new Rust crate `iroh-http-group-core` would hold the Rust side of the
  napi / Deno FFI bindings.

## Notes

- This is a **substantial feature** — roughly the same scope as adding a new
  platform adapter. Do not fold it into the core library.
- Group membership is ephemeral: members who disconnect are removed by the
  gossip protocol's liveness detection. There is no persistent membership
  list.
- Message ordering is best-effort (gossip). Applications needing total order
  must implement their own sequence numbers.
- Message size is bounded by the gossip protocol's datagram limit
  (~1 KB usable payload with iroh-gossip's current encoding). Larger payloads
  require chunking at the application layer.
- Encryption of group messages (so only members can read) is a future
  extension. The current design provides authentication (known sender) but
  not confidentiality beyond QUIC's transport encryption.
