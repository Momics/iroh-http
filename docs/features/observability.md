# Observability — Connection Stats and Path Info

Connection-level metrics and network path information exposed on `IrohNode`.
Includes node-wide aggregate counters, per-peer path and QUIC stats, and a
stream of transport events for reactive monitoring.

## API

```ts
// Node-wide aggregate stats (synchronous snapshot, never null):
node.stats(): Promise<EndpointStats>

// Per-peer stats (null if no connection exists yet):
node.peerStats(nodeId: string): Promise<PeerStats | null>
```

`EndpointStats` fields:

| Field | Type | Description |
|---|---|---|
| `activeConnections` | `number` | Currently open QUIC connections |
| `activeRequests` | `number` | In-flight HTTP requests across all peers |
| `activeReaders` | `number` | Open response body reader handles |
| `activeWriters` | `number` | Open request body writer handles |
| `activeSessions` | `number` | Open `IrohSession` handles |
| `totalHandles` | `number` | Total live handle-store entries |
| `poolSize` | `number` | Cached connections in the pool |

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

## Transport events

Opt-in stream of internal transport events, emitted as `CustomEvent('transport', { detail })`
on the `IrohNode` instance. Enable at construction time:

```ts
const node = await createNode({ observability: { transportEvents: true } });
node.addEventListener('transport', (e) => console.log(e.detail));
```

| Event type | Fields | When emitted |
|---|---|---|
| `pool:hit` | `peerId`, `timestamp` | Fetch reused an existing pooled connection |
| `pool:miss` | `peerId`, `timestamp` | Fetch established a new connection |
| `pool:evict` | `peerId`, `timestamp` | An idle connection was evicted from the pool |
| `path:change` | `peerId`, `addr`, `relay`, `timestamp` | Active path to a peer changed |
| `handle:sweep` | `evicted`, `timestamp` | TTL sweep removed stale handles |

Transport events are not emitted unless `transportEvents: true` is set — the
background polling task does not start otherwise.

## Notes

- `peerStats` returns `null` when there is no active connection to that peer,
  rather than throwing.
- `peerStats` for your **own** node ID returns `null` (it's a remote-peer metric).
- `pathChanges` is cancelled by breaking the `for await` loop.
- `node.stats()` returns an `EndpointStats` snapshot with node-wide aggregate
  counters. It does not require a prior `fetch()` and never returns `null`.
- The `iroh-path-type` response header (indicating direct vs relay path) is
  planned but not yet injected by the current server. It will be added once
  iroh exposes stable per-connection path metadata in its public API.
