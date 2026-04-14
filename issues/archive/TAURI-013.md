---
id: "TAURI-013"
title: "Mobile mdns_next_event drops buffered discovery events"
status: fixed
priority: P1
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["mobile", "discovery", "data-loss"]
---

# [TAURI-013] Mobile mdns_next_event drops buffered discovery events

## Summary

On mobile, `mdns_next_event` calls the native poll command, which drains all pending events on the platform side, but Rust returns only the first event (`.next()`). The rest are discarded and can never be observed by the JS caller.

## Evidence

- `packages/iroh-http-tauri/src/commands.rs:732-739` — mobile `mdns_next_event` calls `browse_poll(...)` and returns only `events.into_iter().next()`.
- `packages/iroh-http-tauri/android/src/main/java/com/iroh/http/IrohHttpPlugin.kt:149-153` — `mdns_browse_poll` copies all pending events and clears the queue.
- `packages/iroh-http-tauri/ios/Sources/IrohHttpPlugin.swift:191-193` — `mdns_browse_poll` returns all pending events and clears the queue.

## Impact

Discovery event streams can desynchronize under bursty updates because only one event survives each poll. Peer appearance/expiry transitions can be missed, causing stale peer state and unreliable local discovery behavior.

## Remediation

1. Preserve event queue semantics end-to-end on mobile:
2. Either change Rust mobile `mdns_next_event` to buffer all polled events internally and return one per call, or change native poll to return only one event without draining the full queue.
3. Keep desktop and mobile semantics aligned: one command call should consume exactly one event from the underlying queue.

## Acceptance criteria

1. When native side has N pending discovery events (N > 1), N successive `mdns_next_event` calls return all N events in order.
2. No events are lost when multiple discovered/expired events arrive between polls.
3. Desktop behavior remains unchanged.
