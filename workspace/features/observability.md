---
status: not-implemented
scope: core
priority: high
---

# Feature: Observability — Connection Stats and Path Info

## What

Expose connection-level metrics and network path information (including whether
traffic is flowing over a relay or a direct connection) through async methods
on `IrohNode`.

## Why

The old `iroh` package had full observability: `connectionStats()` (RTT, bytes
sent/received, datagrams), `streamSendStats()`, `streamReceiveStats()`, and
`waitPathChange()` (which indicated relay vs direct). iroh-http lost all of
this.

In a P2P network, observability is especially important:

- Developers need to know whether they are relaying (higher latency, metered
  traffic) or direct (optimal path).
- Debugging connection issues requires RTT and byte-transfer data.
- Applications may want to degrade gracefully or warn the user when relay
  traffic is detected.

Iroh exposes all of this natively. The gap is purely in the JS/TS surface.

## Proposed API

```ts
// On IrohNode:
stats(): Promise<NodeStats>
peerStats(nodeId: string): Promise<PeerStats | null>  // null if not connected
```

```ts
interface NodeStats {
  /** Number of active connections. */
  connections: number;
  /** Total bytes sent across all connections since node start. */
  bytesSent: number;
  /** Total bytes received across all connections since node start. */
  bytesReceived: number;
}

interface PeerStats {
  /** Round-trip time to this peer in milliseconds. */
  rttMs: number;
  /** Active network path to this peer. */
  path: PathInfo;
  /** All known paths to this peer. */
  paths: PathInfo[];
  /** Total bytes sent to this peer. */
  bytesSent: number;
  /** Total bytes received from this peer. */
  bytesReceived: number;
}

interface PathInfo {
  /** Whether this path goes through a relay server. */
  relay: boolean;
  /** The relay URL in use, when relay is true. */
  relayUrl?: string;
  /** The remote socket address for this path. */
  addr: string;
  /** Round-trip time for this specific path in milliseconds, if known. */
  rttMs: number | null;
  /** Whether this is the currently selected (active) path. */
  selected: boolean;
}
```

Path change events, for reactive use:

```ts
// AsyncIterable that yields each time the active path to a peer changes:
node.pathChanges(nodeId: string): AsyncIterable<PathInfo>
```

This mirrors the `waitPathChange` loop from the old adapter, but exposed as an
async iterable rather than a callback — idiomatic modern JS.

## Rust side

`iroh::endpoint::ConnectionInfo` provides `rtt`, `paths`, `bytes_sent`,
`bytes_recvd`. `iroh::Endpoint::connection_info(node_id)` is the entry point.
`iroh::endpoint::PathInfo` has `is_relay`, `addr`, and `latency`.

## Notes

- `peerStats` returns `null` when there is no active connection to that peer,
  rather than throwing. Developers will commonly call this speculatively.
- `pathChanges` should be cancellable by breaking the `for await` loop (which
  calls `return()` on the iterator, dropping the Rust task).
- Stream-level stats (`bytesSent` on a per-request basis) are out of scope
  for the first version. Node-level and peer-level are the useful granularity.
