---
id: "A-ISS-049"
title: "Serve callback model diverges across adapters — no shared abstraction"
status: open
priority: P1
date: 2026-04-14
area: core
package: ""
tags: [architecture, consistency, adapters, serve]
---

# [A-ISS-049] Serve callback model diverges across adapters — no shared abstraction

## Summary

Each platform adapter implements a fundamentally different mechanism for delivering incoming requests from Rust to the platform runtime:

- **Node**: `ThreadsafeFunction` callback (push model)
- **Deno**: `mpsc` polling queue via `nextRequest()` (pull model)
- **Tauri**: Tauri `Channel` event emission (push model)
- **Python**: Direct PyO3 callback into Python handler (push model)

These are four independent implementations of "deliver a request to the handler," each with different error propagation, cancellation, and backpressure semantics.

## Evidence

- `packages/iroh-http-node/src/lib.rs` — uses `napi::threadsafe_function::ThreadsafeFunction`
- `packages/iroh-http-deno/src/serve_registry.rs` — implements `ServeQueue` with `mpsc::channel`
- `packages/iroh-http-deno/src/dispatch.rs` — `serveNextRequest` pops from the queue
- `packages/iroh-http-tauri/src/commands.rs` — uses `tauri::ipc::Channel` to send request events
- `packages/iroh-http-py/src/lib.rs` — calls Python handler directly via `pyo3_async_runtimes`

## Impact

- **Error propagation**: Node's `ThreadsafeFunction` errors are logged and swallowed. Deno's poll model returns errors to the caller. Tauri's channel can silently drop events. Python raises exceptions.
- **Backpressure**: Node pushes all requests immediately. Deno pulls one at a time. These create different load profiles.
- **Timeouts**: Request timeout behavior under load depends on which callback model is used, creating platform-dependent behavior for the same logical operation.
- This is the deepest architectural inconsistency in the project and is difficult to address without significant rework.

## Remediation

This is a known consequence of the multi-platform architecture. The core `serve()` function accepts an `on_request: Arc<dyn Fn(RequestPayload)>` callback, and each adapter implements this differently. Possible improvements:

1. **Document the behavioral contract**: Add a section to `docs/architecture.md` specifying exactly what guarantee `on_request` provides (e.g., "will be called once per request, may block the serve loop if slow, must be `Send + Sync`").
2. **Add integration tests**: Per-adapter tests that verify request delivery under load, timeout behavior, and error propagation.
3. **Consider a shared Rust trait**: Define a `RequestDispatcher` trait in core with `send_request()` and `poll_request()` methods, so the behavioral contract is enforced at the type level.

## Acceptance criteria

1. The behavioral contract for `on_request` is documented in `docs/architecture.md`.
2. Each adapter's deviation from the contract (if any) is documented in its own guideline file.
3. Integration tests exist that verify request delivery and timeout behavior for at least two adapters.
