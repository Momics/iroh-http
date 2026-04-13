---
id: "004"
title: "Offline and intermittent connectivity resilience"
status: open
date: 2026-04-13
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

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| No built-in retry; callers handle it | Predictable, explicit | Boilerplate for every Tauri app |
| Opt-in `retry` option on `fetch` (like `keepalive`) | Caller-controlled, familiar pattern | Limited to retrying the same request |
| Node-level outbox queue with delivery events | Enables richer offline-first patterns | Large scope; hard delivery guarantees required |
| Document as out-of-scope; recommend recipe patterns | Keeps core simple | Users rebuild the same logic repeatedly |

## Implications

- Directly relevant to the Tauri adapter and desktop-app use cases.
- Any retry or queueing behaviour must be clearly documented — silent retry
  violates the principle of least surprise.
- Interacts with the hole-punching connectivity exploration (003).
- Would affect the offline-first and peer-fallback recipes.

## Next steps

- [ ] Survey what Tauri app developers currently do for offline HTTP resilience.
- [ ] Decide whether any retry behaviour belongs in the core or only in
  higher-level wrappers.
- [ ] Check whether the offline-first recipe already covers enough ground.
