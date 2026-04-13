---
id: "PARITY-006"
title: "Python addr(), ticket(), and home_relay() are synchronous; all JS platforms are async"
status: open
priority: P2
date: 2026-04-13
area: python
package: iroh-http-py
tags: [python, parity, api, async, sync]
---

# [PARITY-006] Python `addr()`, `ticket()`, `home_relay()` are sync; JS platforms are async

## Summary

`addr()`, `ticket()`, and `home_relay()` are synchronous in Python but return `Promise<...>` in all three JS platforms. This is the most surprising divergence when porting code between platforms.

## Evidence

From API surface parity:
- `addr()` — Node async, Deno async, Tauri async, Python **sync**
- `ticket()` — Node async, Deno async, Tauri async, Python **sync**
- `homeRelay()` — Node async, Deno async, Tauri async, Python **sync**

## Impact

Code written for JS cannot be translated to Python without changing the call sites from `await node.addr()` to `node.addr()`, and vice versa. This increases porting effort and cognitive overhead.

## Remediation

1. Convert `addr()`, `ticket()`, and `home_relay()` to async in Python, or document the intentional divergence clearly in both the Python API docs and the parity table.

## Acceptance criteria

1. Either all three methods are async in Python, or the divergence is explicitly called out in documentation with rationale.
