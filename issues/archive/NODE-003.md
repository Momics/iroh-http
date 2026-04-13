---
id: "NODE-003"
title: "Compression options are partially wired — compressionLevel is accepted but unused"
status: closed
priority: P2
date: 2026-04-13
area: node
package: iroh-http-node
tags: [node, compression, options, ignored]
---

# [NODE-003] Compression options are partially wired in Node adapter

## Summary

`compressionLevel` is accepted in the FFI options type but the value is never used. Compression is only enabled when `compressionMinBodyBytes` is set, meaning `compression: true` or `{ level: N }` alone does not enable compression.

## Evidence

- `packages/iroh-http-node/src/lib.rs:93` — FFI struct carries `compression_level`
- `packages/iroh-http-node/src/lib.rs:165` — `compression_level` is not applied
- `packages/iroh-http-node/lib.ts:354` — `compressionLevel` accepted in TypeScript

## Impact

Callers who set `compression: true` or `compression: { level: 3 }` without also setting `compressionMinBodyBytes` receive no compression.

## Remediation

1. Wire `compressionLevel` through to core, or update the TypeScript API to only accept the `{ minBodyBytes }` shape that actually works.

## Acceptance criteria

1. All documented compression configuration shapes either work or are removed from the public API.
