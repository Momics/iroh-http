---
id: "DRIFT-A"
title: "sign-verify.md has broken relative doc links in See Also section"
status: open
priority: P3
date: 2026-04-13
area: docs
package: ""
tags: [docs, links, sign-verify]
---

# [DRIFT-A] `sign-verify.md` has broken relative doc links

## Summary

The "See also" section of the sign/verify feature docs links to relative paths that do not exist in the repository.

## Evidence

- `docs/features/sign-verify.md:29` — broken `See also` links to non-existent relative paths

## Impact

Navigating doc links leads to 404s and creates a confusing reader experience.

## Remediation

1. Fix the broken links to point to existing doc files, or remove the stale references.

## Acceptance criteria

1. All links in `sign-verify.md` resolve to existing files.
