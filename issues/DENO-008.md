---
id: "DENO-008"
title: "Deno dispatch.rs is a 700-line business logic layer, not a thin FFI shim"
status: fixed
priority: P1
date: 2026-04-14
area: deno
package: "iroh-http-deno"
tags: [architecture, layer-violation, adapter]
---

# [DENO-008] Deno dispatch.rs is a 700-line business logic layer, not a thin FFI shim

## Summary

`dispatch.rs` in the Deno adapter contains ~700 lines implementing a full JSON dispatch router, endpoint slab management, serve registry, and option deserialization — far beyond the "thin shim, no logic, no state" contract described in the architecture doc.

## Evidence

- `packages/iroh-http-deno/src/dispatch.rs:34-52` — endpoint slab management (duplicated from Node/Tauri)
- `packages/iroh-http-deno/src/dispatch.rs` — `dispatch()` function has 30+ match arms, each implementing JSON deserialization, argument extraction, and response serialization
- `packages/iroh-http-deno/src/serve_registry.rs:1-55` — a full stateful serve queue (`ServeQueue`) with `mpsc` channels for request polling
- `docs/architecture.md:37-40` — "Thin shims. Translate platform types ↔ u64 handles. No logic. No state."

**Contrast with Node:** Node uses `napi-rs` macros that auto-generate the binding layer, resulting in much thinner per-function wrappers.

## Impact

- Deno adapter is the most complex adapter by ~2× (700 lines vs. ~400 for Tauri, ~500 for Node), making it the hardest to maintain.
- The JSON dispatch pattern means every new core API or parameter change requires updating the match table, the JSON deserialization, and the JSON serialization — all manually.
- The serve registry introduces Deno-specific state management not present in other adapters, creating behavioral divergence.
- This violates the stated architecture invariant that adapters are "no logic, no state."

## Remediation

1. **Short-term:** Factor out the endpoint slab and option deserialization into shared Rust code (see A-ISS-041).
2. **Medium-term:** Evaluate whether Deno can use a code-generated binding approach (similar to napi-rs macros) rather than manual JSON dispatch. Deno FFI supports `Deno.dlopen` with typed symbols — direct FFI functions would eliminate the JSON dispatch layer entirely.
3. **Long-term:** If JSON dispatch must be retained (Deno FFI constraints), generate the dispatch table from a shared schema so new core APIs are automatically reflected.

## Acceptance criteria

1. `dispatch.rs` is reduced to ≤200 lines, or the dispatch layer is generated rather than hand-maintained.
2. `serve_registry.rs` state management is either moved to core or justified as platform-necessary in a doc comment.
