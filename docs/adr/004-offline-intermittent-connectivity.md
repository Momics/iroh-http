---
id: "004"
title: "Offline and intermittent connectivity resilience"
status: accepted
date: 2026-04-13
resolved: 2026-04-25
area: transport | api
tags: [offline, resilience, retry, queuing, tauri, desktop]
---

# [004] Offline and intermittent connectivity resilience

## Context

Standard `fetch` fails immediately and loudly when there is no network
connectivity. That failure model is appropriate for a traditional client-server
HTTP request — if the server is unreachable there is nothing to do.

In iroh-http the transport layer has properties that could change this:
connections are peer-to-peer, Iroh has relay infrastructure, and the system
targets desktop apps via Tauri where intermittent connectivity is a common and
expected condition. The transport layer could, in principle, queue outbound
requests or retry connections in ways that are invisible to the JS caller.

Whether that is desirable — or dangerous — is an open question.

> **Resolved.** Offline resilience (queuing, retry) is explicitly out of scope
> for iroh-http. Connection-level recovery exists in the pool. Application-level
> patterns belong in recipes. See [Decisions](#decisions).

## Questions

1. Should the transport layer ever silently retry a failed connection, or
   should all retries be explicit and caller-controlled?
2. Should there be an opt-in request queue that holds requests while a peer
   is temporarily unreachable and delivers them when connectivity resumes?
3. If queuing is supported, what are the delivery guarantees (at-most-once,
   at-least-once)? How is ordering handled?
4. Is offline resilience in scope for iroh-http itself, or should it be built
   as a higher-level layer on top of it?

## What we know

- The offline-first recipe (`docs/recipes/offline-first.md`) describes
  patterns built on iroh-http primitives, suggesting offline resilience is
  an expected use case.
- Tauri is an explicit target platform; desktop apps routinely deal with
  network interruptions that a web app can ignore.
- Silent retry at the transport layer can create confusing behaviour: a caller
  that expects a time-bound response may be surprised by delayed delivery.
- HTTP is a request-response protocol; queueing without acknowledgement
  risks duplicate delivery.
- **Connection-level recovery exists:** The connection pool in `pool.rs`
  performs a single transparent reconnect when a cached QUIC connection is
  found to be dead. This handles transient network blips without caller
  awareness. It is not a configurable retry mechanism — it is connection
  liveness recovery.
- **No request-level retry middleware:** The tower stack
  (`LoadShed → ConcurrencyLimit → Timeout → RequestService`) does not
  include a retry layer. This is intentional.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| No built-in retry; callers handle it | Predictable, explicit | Boilerplate for every Tauri app |
| Opt-in `retry` option on `fetch` (like `keepalive`) | Caller-controlled, familiar pattern | Limited to retrying the same request |
| Node-level outbox queue with delivery events | Enables richer offline-first patterns | Large scope; hard delivery guarantees required |
| Document as out-of-scope; recommend recipe patterns | Keeps core simple | Users rebuild the same logic repeatedly |

## Decisions

**Q1 — Should the transport silently retry?** No, not at the HTTP request
level. Connection-level recovery (one reconnect attempt on a dead pooled
connection) is built-in and transparent. Request-level retry is the caller's
responsibility.

**Q2 — Should there be a request queue?** No. Queuing is an application-level
concern with complex delivery guarantees (at-most-once, ordering, persistence).
iroh-http should not take opinions on this.

**Q3 — Delivery guarantees?** Not applicable — no built-in queue.

**Q4 — In scope or higher-level?** Higher-level. The offline-first recipe
documents patterns for building resilience on top of iroh-http. The library
provides the transport; the application provides the retry/queue strategy.

## Implications

- Directly relevant to the Tauri adapter and desktop-app use cases. Tauri
  developers should implement retry/queue in their application logic.
- The offline-first recipe covers the recommended patterns.
- Connection-level recovery is transparent and undocumented to callers — it
  just works. This could be noted in the troubleshooting docs.

## Next steps

- [x] Survey what Tauri app developers do for offline resilience — answered:
  application-level retry, not transport-level.
- [x] Decide whether retry belongs in core — no.
- [x] Check whether the offline-first recipe covers enough — yes.
