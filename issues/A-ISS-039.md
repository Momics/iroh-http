---
id: "A-ISS-039"
title: "NodeOptions default for dns_discovery_enabled conflicts with docs"
status: fixed
priority: P2
date: 2026-04-13
area: core
package: "iroh-http-core"
tags: [core, discovery, defaults, api]
---

# [A-ISS-039] NodeOptions default for dns_discovery_enabled conflicts with docs

## Summary

`NodeOptions` derives `Default`, making `dns_discovery_enabled` default to `false`, but the field documentation states the default is `true`.

## Evidence

- `crates/iroh-http-core/src/endpoint.rs:15` — `NodeOptions` derives `Default`.
- `crates/iroh-http-core/src/endpoint.rs:37` — docs state `dns_discovery_enabled` default is true.
- `crates/iroh-http-core/src/endpoint.rs:192` — DNS discovery is enabled only when this flag is true.
- `crates/iroh-http-core/README.md:15` — example uses `NodeOptions::default()`, which currently disables DNS discovery.

## Impact

Core users relying on `NodeOptions::default()` may silently run without DNS discovery, leading to connectivity/discovery surprises and mismatch with documented behavior.

## Remediation

1. Implement manual `Default` for `NodeOptions` with `dns_discovery_enabled: true`.
2. Audit other defaults to ensure comments match actual behavior.
3. Add a test asserting intended defaults.

## Acceptance criteria

1. `NodeOptions::default().dns_discovery_enabled` matches documented default.
2. Bind behavior for default options matches docs and examples.
3. Default-value regression test exists and passes.

