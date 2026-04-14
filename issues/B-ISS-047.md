---
id: "B-ISS-047"
title: "Connection pool stale-connection retry not described in architecture docs"
status: fixed
priority: P3
date: 2026-04-14
area: docs
package: iroh-http-core
tags: [docs, pool, correctness]
---

# [B-ISS-047] Connection pool stale-connection retry not described in architecture docs

## Summary

`docs/architecture.md` says the pool does a liveness check and "stale connections are evicted." The actual implementation in `pool.rs` goes further: on a stale cache hit, it invalidates the entry and retries the connect once, so the caller always gets a live connection without seeing the staleness. This retry guarantee is the important part of the contract and is not documented.

## Evidence

- `docs/architecture.md` — Connection Pool section: "Liveness check before returning a cached connection; stale connections are evicted"
- `crates/iroh-http-core/src/pool.rs` — after a stale hit, `cache.invalidate(&key).await` is called and `connect_fn` is invoked again; the retry path returns the new connection to the caller transparently

## Impact

Low — documentation only. A maintainer reading the docs might believe a stale connection surfaces as an error to the caller. The retry guarantee is actually an important reliability property worth stating.

## Remediation

1. Update the Connection Pool section in `docs/architecture.md` to describe the one-retry behaviour: "On a stale hit, the entry is invalidated and one reconnect attempt is made transparently — callers never observe a stale connection."

## Acceptance criteria

1. `docs/architecture.md` Connection Pool section accurately describes the stale-hit retry behaviour.
