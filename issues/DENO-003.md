---
id: "DENO-003"
title: "Compression options effectively ignored in Deno adapter unless minBodyBytes is set"
status: open
priority: P2
date: 2026-04-13
area: deno
package: iroh-http-deno
tags: [deno, compression, options, ignored]
---

# [DENO-003] Compression options effectively ignored in Deno adapter

## Summary

The Deno adapter sends both `compressionLevel` and `compressionMinBodyBytes`, but dispatch only enables compression when `compression_min_body_bytes.is_some()`. `compression_level` is never applied, so `compression: true` and `compression: { level: N }` are both no-ops.

## Evidence

- `packages/iroh-http-deno/src/dispatch.rs:212-218` — compression enabled only when `min_body_bytes` is `Some`

## Impact

Callers who set `compression: true` or configure only a compression level see no effect. This diverges silently from the documented API contract.

## Remediation

1. Enable compression when `compressionLevel` is provided and non-default, in addition to the `minBodyBytes` path.
2. Or align the public Deno API to only expose the `minBodyBytes` shape that is actually wired.

## Acceptance criteria

1. `compression: true` enables compression with a default level.
2. `compression.level` produces an observable effect on the negotiated encoding.
