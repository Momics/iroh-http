# Node Tickets

A `NodeTicket` is a compact, shareable string that encodes a node's full
`NodeAddr` — its public key, current relay URL, and any known direct socket
addresses. Sharing a ticket lets a peer attempt a direct connection immediately
rather than going through a relay round-trip first.

Tickets are the natural share format for iroh-http nodes: URL-safe, QR-codeable,
and copy-pasteable into any app.

## API

```ts
// Generate a ticket for this node:
const ticket: string = await node.ticket();

// Extract the node ID from a ticket (no I/O, purely encoding):
import { ticketNodeId } from 'iroh-http-shared';
const peerId = ticketNodeId(ticket);

// Fetch using a ticket — routing hints are used automatically:
await node.fetch(ticket.toURL('/api/data'));
```

`node.fetch` accepts any `NodeAddr`-compatible value wherever a peer is
expected: bare node ID string, `NodeAddr` object, or ticket string.

## Format

Tickets are JSON objects serialised to a URL-safe base64 string. They encode
the node's public key, relay URL, and known direct socket addresses. Example
decoded form:

```json
{
  "nodeId": "q4bsxi2...",
  "relayUrl": "https://relay.example.com",
  "addrs": ["192.168.1.5:12345"]
}
```

A ticket string is stable for as long as the node's relay URL and direct
addresses remain current; stale tickets still work by falling back to relay
discovery using the embedded public key.

## References

- [iroh QUIC transport](https://docs.iroh.computer/)
