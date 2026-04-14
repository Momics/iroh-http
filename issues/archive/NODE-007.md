---
id: "NODE-007"
title: "FFI numeric options are lossy-cast without validation"
status: fixed
priority: P2
date: 2026-04-13
area: node
package: iroh-http-node
tags: [node, ffi, validation, options]
---

# [NODE-007] FFI numeric options are lossy-cast without validation

## Summary

Several numeric options cross the Node FFI as `f64` and are cast to integer types with `as`, without validating bounds, finiteness, or sign.

## Evidence

- `packages/iroh-http-node/src/lib.rs:140` — `idle_timeout` cast with `as u64`
- `packages/iroh-http-node/src/lib.rs:151` — `drain_timeout` cast with `as u64`
- `packages/iroh-http-node/src/lib.rs:161` — `request_timeout` cast with `as u64`
- `packages/iroh-http-node/src/lib.rs:162` — `max_request_body_bytes` cast with `as usize`

## Impact

Negative, `NaN`, or very large values can be silently coerced into surprising integers instead of being rejected as invalid input, leading to hard-to-debug behavior.

## Remediation

1. Validate each numeric option before conversion (finite, non-negative, and within target type range).
2. Return `Status::InvalidArg` for invalid values with clear field-specific messages.
3. Add tests that assert invalid numeric inputs are rejected.

## Acceptance criteria

1. Invalid numeric option values produce explicit `InvalidArg` errors.
2. Unit/integration tests cover `NaN`, negative, and overflow-like values for affected fields.

