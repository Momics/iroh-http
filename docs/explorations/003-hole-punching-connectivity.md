---
id: "003"
title: "Surfacing partial connectivity and hole-punching in the API"
status: open
date: 2026-04-13
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

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Expose connectivity events on the node object | Single place, not coupled to individual requests | Callers must wire up listeners separately |
| Add `onConnecting` / `onRelay` hooks to fetch options | Per-request visibility | Non-standard, verbose |
| Surface via response metadata (e.g. timing headers) | Compatible with existing response model | After the fact; no visibility during establishment |
| Document and do nothing for now | No API complexity | Silent failure modes are confusing |

## Implications

- Affects the mental model documented for JS/Python callers.
- Timeout defaults have user-facing consequences; wrong defaults will cause
  silent hangs in NAT-heavy environments.
- Relevant to the offline/intermittent connectivity exploration (004).

## Next steps

- [ ] Survey how other P2P libraries (libp2p, WebRTC APIs) expose
  connectivity state to application code.
- [ ] Audit the observability feature spec to see if connection-state events
  are already planned.
- [ ] Decide whether this is in scope before v1 or deferred.
