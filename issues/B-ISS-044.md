---
id: "B-ISS-044"
title: "Python adapter missing session API, key operations, and mDNS — docs imply complete"
status: open
priority: P1
date: 2026-04-14
area: python
package: iroh-http-py
tags: [python, parity, sessions, discovery, sign-verify]
---

# [B-ISS-044] Python adapter missing session API, key operations, and mDNS — docs imply complete

## Summary

The Python guidelines and feature docs describe `IrohNode` with `serve()`, session connections, `SecretKey`/`PublicKey` sign/verify, and mDNS advertise/browse. The actual `iroh-http-py/src/lib.rs` only implements `fetch()`, body consumption (`bytes()`, `text()`, `json()`), and a stub `serve()`. There is no session API, no key operations, and no mDNS. The documentation makes the Python adapter look feature-complete when it is not.

## Evidence

- `docs/guidelines/python.md` — describes `create_node`, `IrohNode.serve()`, session lifecycle
- `docs/features/sign-verify.md` — documents `SecretKey.sign()` / `PublicKey.verify()` as part of the API (no platform exclusion noted)
- `docs/features/discovery.md` — documents `node.advertise()` / `node.browse()` (no platform exclusion noted)
- `packages/iroh-http-py/src/lib.rs` — only `IrohNode.fetch()`, `IrohResponse`, and a stub `serve()` are implemented; no `SecretKey`, `PublicKey`, `IrohSession`, or mDNS methods
- `packages/iroh-http-py/iroh_http/__init__.pyi` — stub file will show the missing surface to IDE users

## Impact

Contributors implementing features against the Python guidelines will build against a non-existent surface. Users installing the Python package will find that documented features silently don't exist. The `.pyi` stub file likely exposes the gap directly to IDE tooling.

## Remediation

Two valid paths:

**Option A — Add platform caveats to docs:**
1. Add a "Platform support" table to each feature doc noting which adapters implement it.
2. Note in `docs/guidelines/python.md` which sections are aspirational vs. implemented.
3. Track each missing feature as a child issue.

**Option B — Implement the missing surface:**
1. Add `SecretKey` / `PublicKey` PyO3 classes with `sign` / `verify` async methods.
2. Implement `IrohSession` via `session_connect` / bidi stream calls.
3. Implement `advertise` / `browse` via `iroh-http-discovery` if the feature flag is enabled.
4. Update `__init__.pyi` to match.

## Acceptance criteria

1. Either: platform support tables exist in relevant feature docs and are accurate.
2. Or: the Python adapter implements and tests sign/verify, sessions, and mDNS.
