---
id: "TAURI-002"
title: "maxPooledConnections and poolIdleTimeoutMs are silently ignored in Tauri"
status: open
priority: P1
date: 2026-04-13
area: tauri
package: iroh-http-tauri
tags: [tauri, pool, options, ignored]
---

# [TAURI-002] Pool tuning options silently ignored in Tauri

## Summary

Guest JS sends `maxPooledConnections` and `poolIdleTimeoutMs` to the Rust command, but the Rust command args type does not define those fields and `NodeOptions` hardcodes the pool values to `None`.

## Evidence

- `packages/iroh-http-tauri/guest-js/index.ts:485` — sends `maxPooledConnections`
- `packages/iroh-http-tauri/guest-js/index.ts:486` — sends `poolIdleTimeoutMs`
- `packages/iroh-http-tauri/src/commands.rs:23` — command args missing those fields
- `packages/iroh-http-tauri/src/commands.rs:88-89` — `NodeOptions` hardcodes pool to `None`

## Impact

Connection pool tuning is completely non-functional in the Tauri plugin. Callers who configure these options see no effect.

## Remediation

1. Add `maxPooledConnections` and `poolIdleTimeoutMs` to the Tauri command args struct.
2. Wire them through to `NodeOptions` in the command handler.

## Acceptance criteria

1. Setting `maxPooledConnections: 1` limits the pool to a single connection.
