---
id: "DRIFT-B"
title: "architecture.md security defaults table uses stale request_timeout_secs naming"
status: closed
priority: P3
date: 2026-04-13
area: docs
package: ""
tags: [docs, architecture, naming, units]
---

# [DRIFT-B] `architecture.md` security defaults table uses stale timeout field name

## Summary

The security defaults table in `architecture.md` references `request_timeout_secs`, but the runtime API and Rust code use millisecond-based names (`request_timeout_ms` / `requestTimeout`).

## Evidence

- `docs/architecture.md:168` — table uses `ServeOptions::request_timeout_secs`
- `crates/iroh-http-core/src/server.rs:432` — code uses `request_timeout_ms`
- `packages/iroh-http-node/src/lib.rs:100` — Node FFI uses millisecond naming

## Impact

Documentation gives incorrect field names and units, leading developers to configure timeouts incorrectly.

## Remediation

1. Normalize the naming and units in `architecture.md` to match the runtime API.

## Acceptance criteria

1. The architecture docs use the correct field names and correct units (milliseconds).
