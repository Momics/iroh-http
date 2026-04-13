---
id: "DENO-002"
title: "ServeQueue cannot naturally terminate — stuck nextRequest awaits after shutdown"
status: open
priority: P1
date: 2026-04-13
area: deno
package: iroh-http-deno
tags: [deno, serve, shutdown, channel, hang]
---

# [DENO-002] `ServeQueue` cannot naturally terminate

## Summary

`ServeQueue` stores both a `Sender` and `Receiver` in the same shared object. Because the queue always holds a live sender, the channel never closes and `recv()` in `next_request` can never return `None`. `stopServe` also does not remove or close the queue. This leaves Rust futures parked indefinitely after shutdown.

## Evidence

- `packages/iroh-http-deno/src/serve_registry.rs:20-23` — `ServeQueue` owns both `Sender` and `Receiver`

## Impact

After calling `stopServe`, the Deno server may not fully shut down. Async tasks can remain parked, causing resource leaks and preventing clean process exit.

## Remediation

1. Separate ownership of `Sender` and `Receiver` so the channel can close when serving stops.
2. Ensure `stopServe` drops or closes the sender to unblock any parked `recv()` futures.

## Acceptance criteria

1. `stopServe` causes all pending `next_request` awaits to resolve and the Rust tasks to clean up.
