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
await node.fetch(ticket, '/api/data');
```

`node.fetch` accepts any `NodeAddr`-compatible value wherever a peer is
expected: bare node ID string, `NodeAddr` object, or ticket string.

## Format

Tickets encode to Iroh's standard URL-safe base32/bech32 representation and
are compatible with all Iroh tooling. A ticket string is stable for as long as
the node's relay URL and direct addresses remain current; stale tickets still
work via fallback DNS discovery using the embedded public key.

## References

- [Iroh ticket concepts](https://docs.iroh.computer/concepts/tickets)

→ [Patch 26](../patches/26_patch.md)
