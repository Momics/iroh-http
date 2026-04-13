---
id: "PARITY-004"
title: "Python missing closed property / node lifecycle signal"
status: open
priority: P2
date: 2026-04-13
area: python
package: iroh-http-py
tags: [python, parity, api, lifecycle]
---

# [PARITY-004] Python missing `closed` property / node lifecycle signal

## Summary

JS platforms expose a `closed` promise that resolves when the node shuts down. Python has no equivalent lifecycle signal; there is no way to be notified when a node dies.

## Evidence

From API surface parity:
- `closed` (property) — Node ✅, Deno ✅, Tauri ✅, Python ❌

## Impact

Python applications cannot react to unexpected node shutdown or coordinate shutdown sequencing.

## Remediation

1. Add a `closed` awaitable (or equivalent async event) to the Python `IrohNode`.

## Acceptance criteria

1. `await node.closed` completes when the node shuts down, consistent with JS behavior.
