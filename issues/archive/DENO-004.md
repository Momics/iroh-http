---
id: "DENO-004"
title: "Deno README example uses stale drainTimeout shape"
status: closed
priority: P3
date: 2026-04-13
area: deno
package: iroh-http-deno
tags: [deno, docs, readme, stale]
---

# [DENO-004] Deno README example uses stale `drainTimeout` shape

## Summary

The Deno README shows `drainTimeout` as a top-level `createNode` option. The actual API places it under `advanced.drainTimeout`. Copy-pasting the documented snippet silently does nothing.

## Evidence

- `packages/iroh-http-deno/README.md:38-42` — example uses top-level `drainTimeout`

## Impact

Developers following the README will configure `drainTimeout` at the wrong level and see no effect, leading to unexpected behavior on node shutdown.

## Remediation

1. Update the README example to use the correct `advanced.drainTimeout` path.

## Acceptance criteria

1. The README example matches the current `createNode` type signature.
