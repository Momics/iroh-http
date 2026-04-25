---
id: "003"
title: "Surfacing partial connectivity and hole-punching in the API"
status: accepted
date: 2026-04-13
resolved: 2026-04-25
area: api | transport
tags: [nat-traversal, hole-punching, connectivity, relay, fetch]
---

# [003] Surfacing partial connectivity and hole-punching in the API

## Context

Standard HTTP assumes binary connectivity: a request either reaches the server
or it doesn't. The `fetch` API reflects this — you get a response or a network
error.

Iroh's transport is different. Connections can take time to establish via NAT
traversal, may succeed only after hole-punching completes, or may degrade to
relayed routing when direct paths fail. These states exist between "connected"
and "failed" and can take meaningful real-world time (hundreds of milliseconds
to seconds).

A JS developer using `fetch` has no mental model for this. If `fetch` is called
while a hole-punch is in progress, it will either block silently, time out, or
fail — with no indication of why.

> **Resolved.** Connectivity state is observable at the node level via events
> and dedicated APIs. See [Decisions](#decisions).

## Questions

1. Should callers be able to observe connectivity state (e.g.
   "hole-punching in progress", "using relay", "direct connection established")?
2. Should `fetch` expose a way to receive a connection-established event before
   the response arrives, so callers can distinguish transport delay from server
   delay?
3. Should connectivity state be observable at the node level (connection pool
   events) rather than per-request?
4. What is the right default timeout behaviour when a hole-punch is in
   progress?

## What we know

- Iroh exposes connection events internally; the Rust core has visibility into
  whether a connection is direct, relayed, or being established.
- The WebTransport feature (in scope per roadmap) may expose connection
  lifecycle events and could inform how lower-level events are surfaced.
- Node-level observability is described in the observability feature spec;
  it may be the right place to surface this rather than the per-request API.
- **Shipped:** `node.pathChanges(nodeId)` returns `AsyncIterable<PathInfo>`,
  streaming QUIC path changes (relay ↔ direct, network interface switches).
- **Shipped:** `node.peerStats(nodeId)` returns per-peer QUIC stats including
  RTT, bytes sent/received, and current path information.
- **Shipped:** `IrohNode` emits `EventTarget` events: `"pathchange"`,
  `"peerconnect"`, `"peerdisconnect"`, and diagnostic events (`"pool:hit"`,
  `"pool:miss"`, `"pool:evict"`, `"handle:sweep"`).
- **Shipped:** `node.stats()` returns endpoint-wide stats (active connections,
  requests, handles, pool size).

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Expose connectivity events on the node object | Single place, not coupled to individual requests | Callers must wire up listeners separately |
| Add `onConnecting` / `onRelay` hooks to fetch options | Per-request visibility | Non-standard, verbose |
| Surface via response metadata (e.g. timing headers) | Compatible with existing response model | After the fact; no visibility during establishment |
| Document and do nothing for now | No API complexity | Silent failure modes are confusing |

## Decisions

**Q1 — Should callers observe connectivity state?** Yes — at the node level,
not per-request. `node.pathChanges(nodeId)` and `node.peerStats(nodeId)`
provide full visibility into connection state. The `"pathchange"` event fires
when a path switches between direct and relayed.

**Q2 — Connection-established event before response?** Not needed as a
separate API. `peerStats()` can be polled, and `"peerconnect"` fires when a
QUIC connection is established. Per-request hooks were rejected as too verbose
and non-standard.

**Q3 — Node-level vs. per-request?** Node-level. All connectivity observability
lives on `IrohNode` as methods and `EventTarget` events. This is the right
granularity — connection state is a node concern, not a request concern.

**Q4 — Timeout behaviour during hole-punch?** The connection pool handles
this transparently. `fetch()` waits for a connection (via pool single-flight)
and applies the configured `requestTimeout`. If hole-punching takes too long,
the request times out with a clear error. No special timeout mode is needed.

## Implications

- Affects the mental model documented for JS/TS callers — now documented via
  the observability feature spec and the `EventTarget` API.
- Timeout defaults are configurable via `NodeOptions`.
- Path change events enable applications to build reconnect UI, connection
  quality indicators, or relay-fallback notifications.

## Next steps

- [x] Survey how other P2P libraries expose connectivity state.
- [x] Audit the observability feature spec — connection-state events are
  implemented as `pathChanges()`, `peerStats()`, and `EventTarget` events.
- [x] Decide scope — resolved and shipped before v1.
