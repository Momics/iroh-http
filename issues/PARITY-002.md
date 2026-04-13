---
id: "PARITY-002"
title: "Python missing publicKey / secretKey wrapper objects — only raw strings/bytes exposed"
status: open
priority: P2
date: 2026-04-13
area: python
package: iroh-http-py
tags: [python, parity, api, keys]
---

# [PARITY-002] Python missing `publicKey` / `secretKey` wrapper objects

## Summary

JS platforms expose structured `PublicKey` and `SecretKey` objects on the node. Python only exposes flat `node_id: str` and `keypair: bytes` fields, making cross-platform code that inspects key types impossible to write portably.

## Evidence

From API surface parity:
- `publicKey` — Node ✅, Deno ✅, Tauri ✅, Python ❌ (has `node_id: str` instead)
- `secretKey` — Node ✅, Deno ✅, Tauri ✅, Python ❌ (has `keypair: bytes` instead)

## Impact

Python users cannot write code that handles both Python nodes and JS nodes using the same key abstraction.

## Remediation

1. Add `publicKey` and `secretKey` properties to the Python `IrohNode` that return typed wrapper objects (or at minimum objects with a consistent `.toString()` / `__str__` representation).

## Acceptance criteria

1. `node.publicKey` returns an object in Python with the same logical value as `node.publicKey` in Node/Deno.
