---
id: "PARITY-003"
title: "Python missing pathChanges() method"
status: closed
priority: P2
date: 2026-04-13
area: python
package: iroh-http-py
tags: [python, parity, api, path-changes]
---

# [PARITY-003] Python missing `pathChanges()` method

## Summary

`pathChanges()` is available on all JS platforms but is absent from the Python `IrohNode`.

## Evidence

From API surface parity:
- `pathChanges()` — Node ✅, Deno ✅, Tauri ✅, Python ❌

## Impact

Python applications cannot listen for QUIC path change events, limiting observability and path-optimization use cases.

## Remediation

1. Implement `path_changes()` in `iroh-http-py` as an async iterator or callback-based API.

## Acceptance criteria

1. `node.path_changes()` yields path-change events consistent with what the JS adapters produce.
