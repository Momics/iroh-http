# Observability — Connection Stats and Path Info

Connection-level metrics and network path information exposed as async methods
on `IrohNode`. Includes whether traffic is flowing over a relay or a direct
connection.

## API

```ts
// On IrohNode:
peerStats(nodeId: string): Promise<PeerStats | null>  // null if not connected
```

```ts
interface PeerStats {
  /** Whether the active path goes through a relay server. */
  relay: boolean;
  /** Active relay URL, or null if using a direct path. */
  relayUrl: string | null;
  /** All known paths to this peer. */
  paths: PathInfo[];
}

interface PathInfo {
  /** Whether this path goes through a relay server. */
  relay: boolean;
  /** The relay URL in use, when relay is true. */
  relayUrl?: string;
  /** The remote socket address for this path. */
  addr: string;
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
- A top-level `stats()` method (node-wide aggregate metrics) is planned but
  not yet implemented. Use `peerStats` for per-peer information.
