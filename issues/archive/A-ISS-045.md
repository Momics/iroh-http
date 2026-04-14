---
id: "A-ISS-045"
title: "Endpoint handle type inconsistency across adapters (u32 vs u64)"
status: duplicate
priority: P2
duplicate_of: "A-ISS-041"
date: 2026-04-14
area: core
package: ""
tags: [architecture, consistency, adapters]
---

# [A-ISS-045] Endpoint handle type inconsistency across adapters (u32 vs u64)

## Summary

The type used for endpoint handles varies across platform adapters: Node and Deno use `u32`, while Tauri uses `u64`. This inconsistency means the endpoint handle space and overflow behavior differ by platform. Stream handles (from slotmap) are consistently `u64` across all adapters, making the endpoint handle divergence an outlier.

## Evidence

- `packages/iroh-http-node/src/lib.rs:41` — `insert_endpoint(ep: IrohEndpoint) -> u32` (returns `slab.insert(ep) as u32`)
- `packages/iroh-http-deno/src/dispatch.rs:40` — `insert_endpoint(ep: IrohEndpoint) -> u32` (returns `slab.insert(ep) as u32`)
- `packages/iroh-http-tauri/src/state.rs:19` — `insert_endpoint(ep: IrohEndpoint) -> u64` (returns `slab.insert(ep) as u64`)
- All three use `slab::Slab` which returns `usize` — the cast targets differ.

## Impact

- A developer building a cross-platform library on top of iroh-http cannot write handle-generic code.
- The `as u32` truncation in Node/Deno is technically unsafe for processes with >4 billion endpoint allocations (unlikely but the type system should prevent it).
- Inconsistency creates friction for contributors maintaining multiple adapters.

## Remediation

1. Standardize on `u64` for endpoint handles in all adapters (matching stream handles).
2. Or: move the endpoint slab into `iroh-http-core` with a consistent `u64` return type (see A-ISS-041).

## Acceptance criteria

1. All adapters use the same integer type for endpoint handles.
2. The endpoint handle type matches the stream handle type (`u64`).
