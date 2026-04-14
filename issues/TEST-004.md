---
id: "TEST-004"
title: "Rust core: add edge-case tests for zero-value configs, mid-stream cancellation, and pool exhaustion"
status: open
priority: P1
date: 2026-04-14
area: core
package: "iroh-http-core"
tags: [testing, unit, integration, edge-cases, regression]
---

# [TEST-004] Rust core: add edge-case tests for zero-value configs, mid-stream cancellation, and pool exhaustion

## Summary

`crates/iroh-http-core/tests/integration.rs` has ~30 scenarios (2039 lines)
covering happy paths and some limits. Missing: zero-value configuration
boundaries, mid-stream cancellation, connection pool exhaustion under
contention, and invalid handle use after close. These map directly to 8
config-default and architecture bugs (A-ISS-034, A-ISS-035, A-ISS-040,
ISS-001, ISS-022).

## Evidence

- A-ISS-034: `max_chunk_size = 0` can hang `send_chunk` — no test
- A-ISS-035: `channel_capacity = 0` can panic body channel — no test
- ISS-001: small `maxHeaderBytes` can panic server — test added but
  boundary at zero not covered
- Existing `integration.rs` tests connection pooling but not exhaustion
  under concurrent load

## Impact

Config-default bugs are the #3 root cause (13 of 106 issues). Rust core
edge-case tests are cheap (no FFI, no external process) and catch these
before they propagate to any adapter.

## Remediation

Add to `crates/iroh-http-core/tests/integration.rs`:

1. **Zero max_chunk_size:** `NodeOptions { max_chunk_size: 0, .. }` →
   either rejected at construction or `send_chunk` handles gracefully
2. **Zero channel_capacity:** `NodeOptions { channel_capacity: 0, .. }` →
   rejected at construction or defaults to 1
3. **Cancellation mid-stream:** client cancels fetch token while server is
   streaming body → server observes error, no panic, endpoint stays healthy
4. **Pool exhaustion:** `max_pooled_connections = 1`, open 10 concurrent
   connections → oldest evicted, all operations complete, no deadlock
5. **Timeout during body transfer:** `request_timeout = 100ms`, handler
   sleeps 500ms during body write → client receives 408 or timeout error
6. **Invalid handle after close:** call `get_endpoint(handle)` after
   `remove_endpoint(handle)` → returns `None`, no panic

## Acceptance criteria

1. `cargo test --workspace` passes with all new tests
2. No `#[ignore]` on new tests (unlike `diag_timeout.rs`)
3. No new crate dependencies
