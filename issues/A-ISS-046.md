---
id: "A-ISS-046"
title: "NodeOptions and ServeOptions contain 6 duplicated fields with no enforcement of consistency"
status: open
priority: P3
date: 2026-04-14
area: core
package: "iroh-http-core"
tags: [architecture, api-design, duplication]
---

# [A-ISS-046] NodeOptions and ServeOptions contain 6 duplicated fields with no enforcement of consistency

## Summary

`NodeOptions` (in `endpoint.rs`) and `ServeOptions` (in `server.rs`) both declare the same 6 server-limit fields: `max_concurrency`, `max_connections_per_peer`, `request_timeout_ms`, `max_request_body_bytes`, `drain_timeout_secs`, and `max_consecutive_errors`. While `IrohEndpoint::serve_options()` bridges between them, there is no compile-time enforcement that the two structs stay in sync.

## Evidence

- `crates/iroh-http-core/src/endpoint.rs:82-87` — `NodeOptions` server limit fields
- `crates/iroh-http-core/src/server.rs:51-57` — `ServeOptions` fields (identical set)
- `crates/iroh-http-core/src/endpoint.rs:296-304` — `serve_options()` manually copies 6 fields

## Impact

- Adding a new server limit to `NodeOptions` without adding it to `ServeOptions` (or vice versa) produces no compiler error.
- The manual field-by-field copy in `serve_options()` is fragile — a new field can be added to both structs but forgotten in the bridge method.
- Violates Principle 2 ("duplicate concepts are a bug").

## Remediation

Option A: Make `serve()` accept `&IrohEndpoint` instead of `ServeOptions`, reading limits directly from the endpoint. Remove `ServeOptions` as a public type.

Option B: If per-call override flexibility is needed (which is the current design intent), extract the common 6 fields into a shared `ServerLimits` struct that both `NodeOptions` and `ServeOptions` embed. The bridge method becomes a single field copy.

Option C: Add a compile-time test that asserts field parity between `NodeOptions` and `ServeOptions` (e.g., via a macro or by making `ServeOptions` derive from `NodeOptions`).

## Acceptance criteria

1. Adding a new server-limit field in one struct but not the other produces a compile error.
2. The manual 6-field copy in `serve_options()` is replaced with a structural operation.
