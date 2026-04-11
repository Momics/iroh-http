---
status: pending
refs: features/tickets.md
---

# Patch 26 — Node Tickets

Expose `node.ticket()` and accept ticket strings wherever a peer address is
expected, as described in [tickets.md](../features/tickets.md).

## Problem

Nodes can only be addressed by bare node ID. Cold connections require a relay
round-trip to discover the peer's current addresses before the QUIC handshake
can begin. There is no share-friendly string format that encodes full routing
hints.

`iroh::NodeTicket` already implements the ticket format in Rust.
`IrohEndpoint::node_addr()` already exists in the bridge (Patch 17). The
remaining work is pure exposure: JS surface + ticket parsing in `fetch`.

## Changes

### 1. Rust — `crates/iroh-http-core/src/bridge.rs`

**Add `node_ticket`:**

```rust
/// Returns the current node ticket as a URL-safe string.
/// Encodes the node's public key, relay URL, and direct addresses.
pub fn node_ticket(handle: u32) -> String {
    let endpoint = get_endpoint(handle);
    let addr = endpoint.node_addr().await.unwrap();
    iroh::NodeTicket::new(addr).to_string()
}
```

**Extend `parse_node_id` (or replace with `parse_node_addr`):**

```rust
/// Parse a string as a NodeAddr.
/// Accepts: bare node ID string, full NodeAddr JSON, or ticket string.
pub fn parse_node_addr(s: &str) -> Result<NodeAddr, String> {
    // 1. Try parsing as a NodeTicket (bech32 / base32 ticket format)
    if let Ok(ticket) = iroh::NodeTicket::from_str(s) {
        return Ok(ticket.node_addr().clone());
    }
    // 2. Try parsing as a bare node ID
    if let Ok(key) = iroh::PublicKey::from_str(s) {
        return Ok(NodeAddr::new(key));
    }
    Err(format!("cannot parse '{}' as node address or ticket", s))
}
```

All places that currently call `parse_node_id` in `fetch` and bidi stream
setup should call `parse_node_addr` instead.

### 2. TypeScript — `packages/iroh-http-shared/src/index.ts`

Add to `IrohNode`:

```ts
/** Generate a ticket string encoding this node's current address. */
ticket(): Promise<string>;
```

Add helper export:

```ts
/**
 * Extract the node ID from a ticket string without network I/O.
 * Accepts a ticket string or a bare node ID string (returned unchanged).
 */
export function ticketNodeId(ticket: string): string;
```

Both `node.fetch(peer, ...)` and `node.connect(peer)` accept any
`NodeAddr`-compatible value wherever a peer is expected: bare node ID string,
`NodeAddr` object, or ticket string.

### 3. Platform adapters

Wire `node_ticket` through each adapter:

- **Node.js napi**: add `ticket()` async method to the node class.
- **Deno FFI**: add `node_ticket` to symbol declarations and the node wrapper.
- **Tauri**: add a `node_ticket` Tauri command.
- **Python**: add `ticket()` async method to the Python node class.

`ticketNodeId` is a pure TypeScript helper in `iroh-http-shared` — no native
component needed.

### 4. Tests

```rust
#[tokio::test]
async fn ticket_round_trip() {
    let node = create_test_node().await;
    let ticket_str = bridge::node_ticket(node.handle);
    // Parse back and verify the public key matches
    let ticket = iroh::NodeTicket::from_str(&ticket_str).unwrap();
    assert_eq!(ticket.node_addr().node_id, node.public_key());
}

#[test]
fn parse_node_addr_accepts_ticket() {
    // Create a ticket string for a known key + addr
    let key = iroh::SecretKey::generate(rand::rngs::OsRng).public();
    let addr = NodeAddr::new(key);
    let ticket = iroh::NodeTicket::new(addr.clone()).to_string();
    let parsed = bridge::parse_node_addr(&ticket).unwrap();
    assert_eq!(parsed.node_id, key);
}
```

## Files

- `crates/iroh-http-core/src/bridge.rs` — `node_ticket`, `parse_node_addr`
- `packages/iroh-http-shared/src/index.ts` — `IrohNode.ticket()`, `ticketNodeId`
- `packages/iroh-http-node/src/` — napi `ticket()` method
- `packages/iroh-http-deno/src/` — FFI `node_ticket` symbol
- `packages/iroh-http-tauri/src/` — Tauri command
- `packages/iroh-http-py/src/` — PyO3 `ticket()` method

## Notes

- `node_ticket` is async because `node_addr()` may involve a brief wait for
  the relay URL to be confirmed after node startup. In practice this resolves
  within milliseconds.
- Ticket strings become stale when a node's direct addresses change (e.g. after
  a network change). They always remain usable via the embedded public key +
  DNS fallback, but the direct path hint may be out of date. Applications that
  share tickets should regenerate them periodically.
