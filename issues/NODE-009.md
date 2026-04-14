---
id: "NODE-009"
title: "Node README options example does not match current createNode API"
status: open
priority: P3
date: 2026-04-13
area: node
package: iroh-http-node
tags: [node, docs, readme, api]
---

# [NODE-009] Node README options example does not match current `createNode` API

## Summary

The Node README still shows stale option keys/shapes (`relays`, and `discovery` with top-level `serviceName`) that differ from the current typed API (`relayMode`, nested `discovery.mdns` shape).

## Evidence

- `packages/iroh-http-node/README.md:44` — example uses `relays`
- `packages/iroh-http-node/README.md:45` — example uses `discovery: { mdns: true, serviceName: ... }`
- `packages/iroh-http-shared/src/bridge.ts:168` — typed API uses `relayMode`
- `packages/iroh-http-shared/src/bridge.ts:217` — `serviceName` is nested under `discovery.mdns`

## Impact

Developers copying the README can pass options that are ignored or misapplied, leading to confusion and misconfiguration.

## Remediation

1. Update README examples to match current `NodeOptions`.
2. Add a docs check (or small TS compile snippet) that validates README option examples.

## Acceptance criteria

1. README examples compile against current TypeScript types.
2. README option keys and nesting match the public API.

