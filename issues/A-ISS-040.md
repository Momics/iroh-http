---
id: "A-ISS-040"
title: "Process-global backpressure prevents independent multi-endpoint configuration"
status: fixed
priority: P1
date: 2026-04-14
area: core
package: "iroh-http-core"
tags: [architecture, backpressure, multi-tenancy]
---

# [A-ISS-040] Process-global backpressure prevents independent multi-endpoint configuration

## Summary

`configure_backpressure()` in `stream.rs` uses a `compare_exchange` guard so only the **first** endpoint's `channel_capacity`, `max_chunk_size_bytes`, and `drain_timeout_ms` take effect. All subsequent endpoints silently inherit the first endpoint's values, violating the principle that independent endpoints should not interfere with each other.

## Evidence

- `crates/iroh-http-core/src/stream.rs:49-62` — `configure_backpressure()` uses `BACKPRESSURE_CONFIGURED` `AtomicBool`; subsequent calls are no-ops.
- `crates/iroh-http-core/src/stream.rs:167-176` — `make_body_channel()` reads the global `CHANNEL_CAPACITY`, not an endpoint-local value.
- `crates/iroh-http-core/src/endpoint.rs:213-218` — `IrohEndpoint::bind()` calls `configure_backpressure()` unconditionally but the call is silently ignored for the second endpoint.

**Scenario:** Endpoint A binds with `channel_capacity: 16`. Endpoint B binds with `channel_capacity: 256`. Endpoint B's streams use capacity 16 without any warning. The developer sees no indication their config was ignored.

## Impact

- Multi-endpoint processes (e.g., an app with one endpoint for local mDNS and another for relay traffic) cannot have independent backpressure tuning.
- Integration tests that spin up two endpoints with different `channel_capacity` or `drain_timeout_ms` silently run with only the first endpoint's config.
- No error or warning is emitted — the second configuration call is a no-op.
- The constraint is not documented in `docs/architecture.md`, `docs/guidelines/rust.md`, or `docs/features/server-limits.md`.
- The silent discard violates Principle 6 ("No silent discards") and Principle 2 ("Never expose implementation details").

## Remediation

1. Move `channel_capacity`, `max_chunk_size_bytes`, and `drain_timeout_ms` into `EndpointInner` (per-endpoint).
2. Pass `ep_idx` or a config reference through `make_body_channel()` so each endpoint's channels use their own capacity.
3. Remove the process-global atomics (`CHANNEL_CAPACITY`, `MAX_CHUNK_SIZE`, `DRAIN_TIMEOUT_MS`, `BACKPRESSURE_CONFIGURED`).
4. If process-global is retained for simplicity, at minimum log a warning when a second endpoint's config is ignored.

## Acceptance criteria

1. Two endpoints in the same process can have different `channel_capacity` values, and each endpoint's body channels use their own capacity.
2. Or: a warning is logged when a subsequent `bind()` call provides different backpressure values than the active configuration, and the constraint is documented in `docs/architecture.md`.

## Absorbed

- **B-ISS-045** (duplicate) — same issue framed as "silently ignored for second endpoint."
