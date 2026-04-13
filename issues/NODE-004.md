---
id: "NODE-004"
title: "Node README options example is out of sync with actual API shape"
status: closed
priority: P3
date: 2026-04-13
area: node
package: iroh-http-node
tags: [node, docs, readme, stale]
---

# [NODE-004] Node README options example is out of sync with actual API

## Summary

The README shows `relays` and `discovery: { mdns: true, serviceName: ... }` in the `createNode` options example. The actual typed options use `relayMode` and a `discovery.mdns` object shape.

## Evidence

- `packages/iroh-http-node/README.md:45` — example uses stale option shapes

## Impact

Developers copy-pasting from the README will use incorrect option keys that are silently ignored.

## Remediation

1. Update the README example to match the current `createNode` TypeScript types.

## Acceptance criteria

1. The README example compiles without TypeScript errors against the current types.
