# Presence

Know which of your peers are currently online. LAN presence is free via mDNS.
WAN presence uses lightweight heartbeat pings. No presence server required.

## The insight

In a conventional app, presence is a server problem: the server tracks
connections and broadcasts "Alice is online." In a P2P network, presence is
*local and graduated*: you are definitively online to your LAN peers the
moment mDNS fires. You are probably online to WAN peers if you responded to a
ping in the last 30 seconds. The distinction matters for UX.

```
Certainty   │  Source               │  Latency
────────────┼────────────────────────┼──────────────
High        │  mDNS (LAN only)      │  ~instant
Medium      │  Heartbeat ping (WAN) │  configurable
Low         │  Last-seen timestamp  │  best-effort
```

## Presence model

```ts
type PresenceState = 'online-lan' | 'online-wan' | 'away' | 'offline';

interface PeerPresence {
  nodeId: string;
  state: PresenceState;
  lastSeen: number;    // Unix ms
  latencyMs?: number;  // round-trip from last ping
}
```

## LAN presence via mDNS

```ts
const presence = new Map<string, PeerPresence>();

async function trackLanPresence(node: IrohNode, signal: AbortSignal) {
  for await (const event of node.browse({ signal })) {
    if (event.type === 'found') {
      presence.set(event.nodeId, {
        nodeId: event.nodeId,
        state: 'online-lan',
        lastSeen: Date.now(),
      });
      emit('presence-change', event.nodeId);
    }
    if (event.type === 'lost') {
      const current = presence.get(event.nodeId);
      if (current?.state === 'online-lan') {
        presence.set(event.nodeId, { ...current, state: 'offline' });
        emit('presence-change', event.nodeId);
      }
    }
  }
}
```

## WAN presence via heartbeat

Each node serves a tiny ping endpoint. A background loop pings known WAN
peers and updates their presence state.

```ts
// Both sides serve this
node.serve({}, async (req) => {
  if (req.method === 'GET' && new URL(req.url).pathname === '/__ping') {
    return new Response('pong', {
      headers: { 'x-server-time': String(Date.now()) },
    });
  }
  // ... other routes
});

async function pingPeer(
  node: IrohNode,
  nodeId: string,
  timeoutMs = 5000,
): Promise<number | null> {
  const start = Date.now();
  try {
    const res = await node.fetch(`iroh://${nodeId}/__ping`, {
      signal: AbortSignal.timeout(timeoutMs),
    });
    if (!res.ok) return null;
    return Date.now() - start; // RTT
  } catch {
    return null;
  }
}
```

## Presence loop

```ts
async function trackWanPresence(
  node: IrohNode,
  wanPeers: string[],
  signal: AbortSignal,
  intervalMs = 30_000,
) {
  while (!signal.aborted) {
    for (const nodeId of wanPeers) {
      // Skip if already detected on LAN
      if (presence.get(nodeId)?.state === 'online-lan') continue;

      const rtt = await pingPeer(node, nodeId);
      const prev = presence.get(nodeId);

      const next: PeerPresence = rtt !== null
        ? { nodeId, state: 'online-wan', lastSeen: Date.now(), latencyMs: rtt }
        : { ...prev ?? { nodeId }, state: 'offline', lastSeen: prev?.lastSeen ?? 0 };

      if (next.state !== prev?.state) {
        presence.set(nodeId, next);
        emit('presence-change', nodeId);
      }
    }
    await sleep(intervalMs, signal);
  }
}
```

## Away detection

Track when a peer was last seen and surface "away" if they haven't responded
recently but haven't explicitly gone offline:

```ts
function getPresence(nodeId: string): PeerPresence {
  const p = presence.get(nodeId);
  if (!p) return { nodeId, state: 'offline', lastSeen: 0 };

  if (p.state === 'online-wan') {
    const stale = Date.now() - p.lastSeen > 60_000; // 1 min
    if (stale) return { ...p, state: 'away' };
  }

  return p;
}
```

## Subscribing to changes

```ts
// Simple event emitter (or use a proper reactive library)
const listeners = new Map<string, Set<() => void>>();

function onPresenceChange(nodeId: string, fn: () => void): () => void {
  if (!listeners.has(nodeId)) listeners.set(nodeId, new Set());
  listeners.get(nodeId)!.add(fn);
  return () => listeners.get(nodeId)?.delete(fn);
}

function emit(event: string, nodeId: string) {
  if (event === 'presence-change') {
    listeners.get(nodeId)?.forEach((fn) => fn());
    listeners.get('*')?.forEach((fn) => fn()); // wildcard subscribers
  }
}
```

## Displaying presence in UI

```ts
function presenceIcon(nodeId: string): string {
  switch (getPresence(nodeId).state) {
    case 'online-lan': return '🟢'; // green — same network
    case 'online-wan': return '🔵'; // blue — reachable remotely
    case 'away':       return '🟡'; // yellow — seen recently, not responding
    case 'offline':    return '⚫'; // black — not seen
  }
}

function latencyLabel(nodeId: string): string {
  const { latencyMs, state } = getPresence(nodeId);
  if (state === 'online-lan') return 'LAN';
  if (latencyMs == null) return '';
  return `${latencyMs} ms`;
}
```

## Announcing your own presence

For WAN peers to track you, they need to ping you. Optionally, push
an online announcement when you start up, so peers don't have to wait for the
next heartbeat interval:

```ts
async function announceOnline(node: IrohNode, knownPeers: string[]) {
  await Promise.allSettled(
    knownPeers.map((peer) =>
      node.fetch(`iroh://${peer}/__presence`, {
        method: 'POST',
        body: JSON.stringify({ nodeId: node.nodeId(), state: 'online' }),
        headers: { 'Content-Type': 'application/json' },
      }),
    ),
  );
}
```

## Notes

- Presence is eventually consistent — a peer can appear online moments before
  they go offline. Design UI to treat it as a hint, not a guarantee.
- LAN presence via mDNS is significantly more reliable than WAN heartbeats.
  Consider surfacing the distinction visually (as above).
- The `__ping` endpoint adds ~40 bytes per peer per interval to your traffic.
  At 30-second intervals and 20 peers, that's under 100 KB/day.

## See also

- [Local-first sync](local-first-sync.md) — trigger a sync immediately when
  a peer comes online; combine with this presence loop
- [Offline-first](offline-first.md) — drain the outbox the moment a peer
  transitions from `offline` → `online-wan`
- [Proximity trust](proximity-trust.md) — `online-lan` peers get elevated
  trust; presence state maps directly to trust tier
