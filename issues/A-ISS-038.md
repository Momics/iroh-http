---
id: "A-ISS-038"
title: "mDNS browse/advertise sessions accumulate without unregister"
status: open
priority: P2
date: 2026-04-13
area: core
package: "iroh-http-discovery"
tags: [core, discovery, mdns, lifecycle]
---

# [A-ISS-038] mDNS browse/advertise sessions accumulate without unregister

## Summary

Discovery session startup adds mDNS lookup services to the endpoint, but dropping session objects does not unregister those services. Repeated start/stop cycles can accumulate lookups.

## Evidence

- `crates/iroh-http-discovery/src/lib.rs:35` — docs state "Drop to stop receiving events."
- `crates/iroh-http-discovery/src/lib.rs:91` — `start_browse` calls `ep.address_lookup().add(...)`.
- `crates/iroh-http-discovery/src/lib.rs:111` — docs state "Drop to stop advertising."
- `crates/iroh-http-discovery/src/lib.rs:132` — `start_advertise` also calls `ep.address_lookup().add(...)`.
- `crates/iroh-http-discovery/src/lib.rs` — no explicit unregister/remove path exists on session drop.

## Impact

Long-running processes that cycle discovery sessions may retain extra lookup services, causing duplicate work, memory growth, and behavior drift from documented lifecycle semantics.

## Remediation

1. Define explicit lifecycle semantics for browse/advertise sessions.
2. Implement teardown behavior (or document additive behavior explicitly if removal is impossible with current API).
3. Add tests (or integration checks) for repeated start/stop cycles and ensure no unbounded accumulation.

## Acceptance criteria

1. Session drop behavior is explicit and correct relative to docs.
2. Repeated browse/advertise start-stop does not create unbounded active lookup growth.
3. Discovery docs accurately reflect implemented lifecycle.

