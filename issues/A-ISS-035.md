---
id: "A-ISS-035"
title: "Zero channel capacity can panic body channel creation"
status: open
priority: P1
date: 2026-04-13
area: core
package: "iroh-http-core"
tags: [core, streaming, panic, validation]
---

# [A-ISS-035] Zero channel capacity can panic body channel creation

## Summary

`channel_capacity` can be set to `0` and is passed directly into `tokio::sync::mpsc::channel`, which panics for zero capacity.

## Evidence

- `crates/iroh-http-core/src/endpoint.rs:250` — bind forwards `opts.channel_capacity` into `configure_backpressure` without validation.
- `crates/iroh-http-core/src/stream.rs:159` — `make_body_channel` reads capacity from global config.
- `crates/iroh-http-core/src/stream.rs:161` — `mpsc::channel(cap)` is called with that value.

## Impact

Invalid runtime config can crash the process during body channel allocation, taking down fetch/serve/session operations and impacting availability.

## Remediation

1. Reject `channel_capacity: Some(0)` in endpoint option validation.
2. Optionally normalize `0` to default capacity if that behavior is preferred.
3. Add a unit test covering zero-capacity input behavior.

## Acceptance criteria

1. Setting `channel_capacity: Some(0)` no longer panics at runtime.
2. Behavior is explicit and tested: either bind fails with a clear error or value is normalized to default.
3. Existing channel/backpressure tests pass.

