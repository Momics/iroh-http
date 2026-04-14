# Observability — Connection Stats and Path Info

Connection-level metrics and network path information exposed as async methods
on `IrohNode`. Includes whether traffic is flowing over a relay or a direct
connection.

## API

```ts
// On IrohNode:
peerStats(nodeId: string): Promise<PeerStats | null>  // null if not connected
```

See [`PeerStats` and `PathInfo` in the specification](../specification.md#supporting-types) for the type shapes.

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
