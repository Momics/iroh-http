---
id: "A-ISS-041"
title: "Endpoint slab management triplicated across Node, Deno, and Tauri adapters"
status: fixed
priority: P2
date: 2026-04-14
area: core
package: "iroh-http-core"
tags: [architecture, duplication, adapters]
---

# [A-ISS-041] Endpoint slab management triplicated across Node, Deno, and Tauri adapters

## Summary

Node, Deno, and Tauri each independently implement an identical `endpoint_slab()` pattern — a `Mutex<Slab<IrohEndpoint>>` with `insert_endpoint` / `get_endpoint` / `remove_endpoint` functions. This is ~30 lines of identical boilerplate per adapter (~100 lines total) that must be maintained in lockstep.

## Evidence

- `packages/iroh-http-node/src/lib.rs:35-45` — `fn endpoint_slab() -> &'static Mutex<Slab<IrohEndpoint>>`
- `packages/iroh-http-deno/src/dispatch.rs:34-52` — identical pattern
- `packages/iroh-http-tauri/src/state.rs:13-27` — identical pattern (but uses `u64` return type instead of `u32`)

All three use the same `OnceLock<Mutex<Slab<IrohEndpoint>>>` pattern with `insert`, `get`, and `remove` methods.

## Impact

- When endpoint lifecycle changes (e.g., adding cleanup logic on remove), all three adapters must be updated independently.
- The Tauri adapter already has a type divergence: it uses `u64` for the endpoint handle while Node and Deno use `u32`. Stream handles (from slotmap) are consistently `u64`, making the endpoint handle divergence an outlier. This means cross-platform code cannot assume a consistent handle type.
- The `as u32` truncation in Node/Deno is technically unsafe for processes with >4 billion endpoint allocations (unlikely but the type system should prevent it).
- Violates Principle 3 ("Leverage, Don't Reinvent") — this is a solved problem that should be centralized.

## Remediation

1. Add an `EndpointRegistry` (or similar) to `iroh-http-core` that owns the `Slab<IrohEndpoint>` and exposes `insert`, `get`, `remove`.
2. All adapters call into the shared registry instead of maintaining their own.
3. Standardize the endpoint handle type (u64 everywhere, or u32 everywhere).

## Acceptance criteria

1. A single endpoint registry implementation exists in `iroh-http-core`.
2. All three Rust adapters (Node, Deno, Tauri) use it.
3. Endpoint handle type is consistent across all adapters (`u64`, matching stream handles).

## Absorbed

- **A-ISS-045** (sub-issue) — endpoint handle type inconsistency (u32 vs u64) is a direct consequence of the triplication; centralizing the slab fixes it.
