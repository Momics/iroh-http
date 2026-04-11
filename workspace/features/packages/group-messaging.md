# Group / Multicast Messaging

A named group abstraction on top of iroh-http's point-to-point transport.
Sending to a group fans out the message to all current members. Members join
and leave dynamically. The group is identified by a shared name rather than
a single node ID.

Backed by [iroh-gossip](https://github.com/n0-computer/iroh-gossip), a
gossip-based pub/sub protocol built on Iroh QUIC.

## API

```ts
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

## Notes

- Group membership is ephemeral: members who disconnect are removed by the
  gossip protocol's liveness detection. There is no persistent membership
  list.
- Message ordering is best-effort (gossip). Applications needing total order
  must implement their own sequence numbers.
- Message size is bounded by the gossip protocol's datagram limit
  (~1 KB usable payload with iroh-gossip's current encoding). Larger payloads
  require chunking at the application layer.
- Encryption of group messages is a future extension. The current design
  provides authentication (known sender) but not confidentiality beyond QUIC's
  transport encryption.

Part of the `iroh-http-group` package.
