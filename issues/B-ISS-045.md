---
id: "B-ISS-045"
title: "Process-global backpressure config silently ignored for second endpoint"
status: open
priority: P2
date: 2026-04-14
area: core
package: iroh-http-core
tags: [correctness, concurrency, testing, backpressure]
---

# [B-ISS-045] Process-global backpressure config silently ignored for second endpoint

## Summary

`configure_backpressure()` in `stream.rs` is a one-shot process-global initialiser protected by an `AtomicBool`. The first `IrohEndpoint::bind` call sets the channel capacity, max chunk size, and drain timeout for the entire process lifetime. Any subsequent endpoint bind — including during testing — silently uses the first endpoint's values. This is undocumented and will cause confusing behaviour in any multi-endpoint scenario.

## Evidence

- `crates/iroh-http-core/src/stream.rs` — `BACKPRESSURE_CONFIGURED` `AtomicBool`; `configure_backpressure()` comment: "Only the **first** call takes effect; subsequent calls are silently ignored"
- `crates/iroh-http-core/src/stream.rs` — `CHANNEL_CAPACITY`, `MAX_CHUNK_SIZE`, `DRAIN_TIMEOUT_MS` are `static` atomics with no per-endpoint scoping
- No mention of this constraint in `docs/architecture.md`, `docs/guidelines/rust.md`, or `docs/features/server-limits.md`

## Impact

- Integration tests that spin up two endpoints with different `channel_capacity` or `drain_timeout_ms` silently run with only the first endpoint's config.
- An application running a "fast" internal node alongside a "slow" external node cannot configure them independently.
- No error or warning is emitted — the second configuration call is a no-op.

## Remediation

1. Move backpressure config into per-endpoint state (e.g. `Arc<BackpressureConfig>` passed into `make_body_channel` and related functions) rather than process-global atomics.
2. If a full refactor is deferred, document the constraint prominently in `docs/architecture.md` under the Handle System or Concurrency Model sections, and add a `tracing::warn!` on the ignored second call.

## Acceptance criteria

1. Either: two endpoints with different `channel_capacity` values each use their own config, verified by test.
2. Or: the constraint is documented in architecture.md and a `tracing::warn!` fires when the second call is silently dropped.
