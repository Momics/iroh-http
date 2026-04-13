---
id: "PARITY-005"
title: "Python serve() returns None — no ServeHandle with finished, onListen, or signal-based stop"
status: closed
priority: P2
date: 2026-04-13
area: python
package: iroh-http-py
tags: [python, parity, api, serve-handle]
---

# [PARITY-005] Python `serve()` returns `None` — no `ServeHandle`

## Summary

JS platforms return a `ServeHandle` from `serve()` with `finished`, `onListen`, and `onError` hooks, plus signal-based stop. Python's `serve()` returns `None`; control is via the separate `stop_serve()` method.

## Evidence

From API surface parity:
- `serve()` — Node ✅ → `ServeHandle`, Deno ✅ → `ServeHandle`, Tauri ✅ → `ServeHandle`, Python ✅ → `None`

## Impact

Python code cannot use lifecycle hooks like `onListen` or await the `finished` promise to know when serving has fully stopped.

## Remediation

1. Return a Python object from `serve()` with at minimum a `wait()` awaitable and an error callback hook.

## Acceptance criteria

1. The returned object from `serve()` allows awaiting shutdown and receiving serve errors, consistent with JS `ServeHandle` semantics.
