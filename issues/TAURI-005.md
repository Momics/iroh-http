---
id: "TAURI-005"
title: "Lifecycle listener cleanup unsubscribe function is never stored or called"
status: closed
priority: P3
date: 2026-04-13
area: tauri
package: iroh-http-tauri
tags: [tauri, lifecycle, cleanup, memory-leak]
---

# [TAURI-005] Lifecycle listener cleanup is never used

## Summary

`installLifecycleListener` returns an unsubscribe function, but `createNode` neither stores it nor calls it. Stale listeners can persist after node shutdown and trigger redundant ping attempts.

## Evidence

- `packages/iroh-http-tauri/guest-js/index.ts:317` — `installLifecycleListener` returns unsubscribe
- `packages/iroh-http-tauri/guest-js/index.ts:345` — return value not stored
- `packages/iroh-http-tauri/guest-js/index.ts:544` — listener never unsubscribed on `close()`

## Impact

After closing a node, stale lifecycle listeners may fire and trigger operations on a dead node object.

## Remediation

1. Store the returned unsubscribe function and call it in the node's `close()` method.

## Acceptance criteria

1. After `node.close()`, no further lifecycle listener callbacks fire for that node.
