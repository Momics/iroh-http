---
id: "A-ISS-036"
title: "session_ready ignores invalid or stale session handles"
status: fixed
priority: P2
date: 2026-04-13
area: core
package: "iroh-http-core"
tags: [core, session, api-consistency, validation]
---

# [A-ISS-036] session_ready ignores invalid or stale session handles

## Summary

`session_ready` currently returns `Ok(())` unconditionally and does not validate that the provided session handle exists.

## Evidence

- `crates/iroh-http-core/src/session.rs:156` — `session_ready` accepts `_session_handle` and does not use it.
- `crates/iroh-http-core/src/session.rs:159` — function always returns `Ok(())`.
- `crates/iroh-http-core/src/session.rs:39` — other session APIs route through `get_conn` and return `InvalidInput` for unknown handles.

## Impact

Callers can receive a false success for invalid handles, masking lifecycle bugs and making API behavior inconsistent across session methods.

## Remediation

1. Validate handle existence in `session_ready` via `get_conn(session_handle)?`.
2. Keep no-op readiness semantics after validation for compatibility.
3. Add a test asserting invalid-handle behavior returns `ErrorCode::InvalidInput`.

## Acceptance criteria

1. `session_ready` returns an error for unknown session handles.
2. `session_ready` still returns `Ok(())` for valid established sessions.
3. Session API behavior is consistent across methods for invalid handles.

