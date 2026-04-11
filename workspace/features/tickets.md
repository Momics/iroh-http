---
status: not-implemented
scope: core
priority: high
---

# Feature: Node Tickets

## What

A `NodeTicket` is a compact, shareable string encoding of a full `NodeAddr` —
the node's public key, its current relay URL, and any known direct socket
addresses. Iroh's Rust layer (`iroh::NodeTicket`) already implements this
format; the feature is purely an exposure gap.

## Why

Currently a peer can only be addressed by node ID alone. The node ID is stable
but carries no routing hints, so the QUIC layer must contact the relay server
to discover the peer's current addresses before opening a connection. This adds
at least one relay round-trip to every cold connection.

A ticket embeds the full current address so the connecting peer can attempt a
direct connection immediately, falling back to the relay only if the direct
addresses are stale.

Tickets are also the natural share format: a user can copy a ticket string and
paste it into another app, email it, or encode it as a QR code. The public key
alone is unwieldy and gives no routing hint.

## Proposed API

```ts
// On IrohNode:
ticket(): Promise<string>
```

A ticket string encodes to a URL-safe base32 / bech32 representation that
Iroh's existing tooling can decode.

On the connect side, `createNode` (or a helper) accepts a ticket as an
alternative to a bare node ID:

```ts
// Helper to extract the node ID from a ticket (no I/O, purely encoding):
import { ticketNodeId } from 'iroh-http-shared';
const peerId = ticketNodeId(ticketStr);

// Fetch using the full ticket as routing hint:
await node.fetch(ticketStr, '/api/data');
// fetch() already accepts NodeAddr; tickets are one serialisation of NodeAddr.
```

## Rust side

`iroh::NodeTicket::new(node_addr)` and `NodeTicket::to_string()` are already
available. `IrohEndpoint::node_addr()` already exists in the Rust bridge as of
Patch 17. The only work is:

1. Add `node_ticket() -> String` to `IrohEndpoint` (one line).
2. Expose it via the napi / Tauri / Deno FFI bindings.
3. Accept a ticket string wherever a node ID string is accepted
   (`parse_node_id` extended to call `NodeTicket::from_str` as a fallback).

## References

- [Iroh ticket concepts](https://docs.iroh.computer/concepts/tickets)
- `iroh::NodeTicket` in the `iroh` crate
