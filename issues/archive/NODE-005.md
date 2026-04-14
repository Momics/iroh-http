---
id: "NODE-005"
title: "Node package uses unbounded @momics/iroh-http-shared dependency range"
status: fixed
priority: P1
date: 2026-04-13
area: node
package: iroh-http-node
tags: [node, dependencies, semver, packaging]
---

# [NODE-005] Node package uses unbounded `@momics/iroh-http-shared` dependency range

## Summary

The Node package depends on `@momics/iroh-http-shared` using `"*"`, which allows any future release to be installed without a coordinated `iroh-http-node` release.

## Evidence

- `packages/iroh-http-node/package.json:31` — dependency is declared as `"@momics/iroh-http-shared": "*"`

## Impact

`iroh-http-node` can break at install time or runtime when `iroh-http-shared` introduces incompatible API or behavior changes, even if users do not upgrade `iroh-http-node` itself.

## Remediation

1. Replace `"*"` with a bounded semver range (for example, `^0.1.0`).
2. Keep `iroh-http-node` and `iroh-http-shared` versioning aligned for breaking changes.
3. Add a release check that blocks publish if the shared dependency range is unbounded.

## Acceptance criteria

1. `packages/iroh-http-node/package.json` no longer uses `"*"` for `@momics/iroh-http-shared`.
2. Installing `@momics/iroh-http-node` cannot pull a future major-incompatible shared package by default.

