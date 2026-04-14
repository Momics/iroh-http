---
id: "B-ISS-048"
title: "compression.md contains internal patch tracking artifact"
status: open
priority: P3
date: 2026-04-14
area: docs
package: iroh-http-core
tags: [docs, cleanup]
---

# [B-ISS-048] compression.md contains internal patch tracking artifact

## Summary

`docs/features/compression.md` ends with `→ [Patch 19](../patches/19_patch.md)` — a leftover internal patch tracking reference that should not appear in a published feature document. The link target does not exist in the repository.

## Evidence

- `docs/features/compression.md` — final line: `→ [Patch 19](../patches/19_patch.md)`
- No `docs/patches/` directory exists in the repository

## Impact

Low — cosmetic. But it will render as a broken link in any documentation site and signals unreviewed internal scaffolding in the public docs.

## Remediation

1. Remove the `→ [Patch 19](../patches/19_patch.md)` line from `docs/features/compression.md`.

## Acceptance criteria

1. `docs/features/compression.md` contains no reference to "Patch 19" or `../patches/`.
