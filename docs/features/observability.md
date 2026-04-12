# Observability — Connection Stats and Path Info

Connection-level metrics and network path information exposed as async methods
on `IrohNode`. Includes whether traffic is flowing over a relay or a direct
connection.

## API

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
// Yields each time the active path to a peer changes:
node.pathChanges(nodeId: string): AsyncIterable<PathInfo>
```

## Notes

- `peerStats` returns `null` when there is no active connection to that peer,
  rather than throwing.
- `pathChanges` is cancelled by breaking the `for await` loop.
- Stream-level stats (per-request `bytesSent`) are out of scope for the first
  version.

→ [Patch 23](../patches/23_patch.md)
