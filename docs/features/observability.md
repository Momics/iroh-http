# Observability — Connection Stats and Path Info

Connection-level metrics and network path information exposed as async methods
on `IrohNode`. Includes whether traffic is flowing over a relay or a direct
connection, plus QUIC-level byte and RTT counters.

## API

```ts
// On IrohNode:
peerStats(nodeId: string): Promise<PeerStats | null>  // null if not connected
```

`PeerStats` fields:

| Field | Type | Source | When null |
|---|---|---|---|
| `relay` | `boolean` | iroh path info | never |
| `relayUrl` | `string \| null` | iroh path info | no active relay |
| `paths` | `PathInfo[]` | iroh path info | never |
| `rttMs` | `number \| null` | QUIC connection | no pooled connection |
| `bytesSent` | `number \| null` | QUIC connection | no pooled connection |
| `bytesReceived` | `number \| null` | QUIC connection | no pooled connection |
| `lostPackets` | `number \| null` | QUIC connection | no pooled connection |
| `sentPackets` | `number \| null` | QUIC connection | no pooled connection |
| `congestionWindow` | `number \| null` | QUIC connection | no pooled connection |

Connection-level stats (`rttMs`, `bytesSent`, etc.) are populated when
an active QUIC connection to the peer exists in the connection pool.  If you
call `peerStats` before any `fetch()` to that peer, these fields will be `null`
while the path fields will still reflect iroh's discovery state.

Path change events, for reactive use:

```ts
// Yields each time the active path to a peer changes:
node.pathChanges(nodeId: string): AsyncIterable<PathInfo>
```

## Notes

- `peerStats` returns `null` when there is no active connection to that peer,
  rather than throwing.
- `peerStats` for your **own** node ID returns `null` (it's a remote-peer metric).
- `pathChanges` is cancelled by breaking the `for await` loop.
- A top-level `stats()` method (node-wide aggregate metrics) is planned but
  not yet implemented. Use `peerStats` for per-peer information.
