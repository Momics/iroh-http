---
id: "DRIFT-C"
title: "server-limits.md contains stale Status note about serve() not passing limits"
status: open
priority: P3
date: 2026-04-13
area: docs
package: ""
tags: [docs, server-limits, stale]
---

# [DRIFT-C] `server-limits.md` contains stale Status note

## Summary

The server-limits feature doc has a "Status" note saying that the TypeScript `serve()` function does not pass limits through to the core. In the current code, limits are configured at `createNode(...)` and wired into Rust.

## Evidence

- `docs/features/server-limits.md:56` — note says TS `serve()` does not pass limits through
- Current code: limits are provided at `createNode(...)` and passed to the Rust endpoint

## Impact

Readers get incorrect information about the current configuration architecture.

## Remediation

1. Replace the stale note with an accurate description of the current configuration path (`createNode` → Rust).

## Acceptance criteria

1. The status note is removed or updated to accurately reflect how limits reach the Rust core.
