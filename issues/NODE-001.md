---
id: "NODE-001"
title: "disableNetworking option is ignored in Node adapter unless relayMode is disabled"
status: open
priority: P1
date: 2026-04-13
area: node
package: iroh-http-node
tags: [node, networking, options, bug]
---

# [NODE-001] `disableNetworking` is ignored in Node adapter

## Summary

`createNode({ disableNetworking: true })` has no effect. The Node adapter derives `disableNetworking` solely from `relayMode === "disabled"` and never reads `options.disableNetworking`.

## Evidence

- `packages/iroh-http-node/lib.ts:325` — `disableNetworking` computed from `relayMode` only
- `packages/iroh-http-node/lib.ts:350` — `options.disableNetworking` is not merged

## Impact

Callers who set `disableNetworking: true` explicitly get networking enabled anyway, making offline/test isolation impossible via this option.

## Remediation

1. Merge `options.disableNetworking` with the relay-mode-derived value using logical OR.

## Acceptance criteria

1. `createNode({ disableNetworking: true })` prevents outbound network connections regardless of `relayMode`.
