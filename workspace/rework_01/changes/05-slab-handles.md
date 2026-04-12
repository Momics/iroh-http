# Change 05 — Handle management strategy (`u32` FFI contract)

## Risk: High if rewritten naively

## Problem

The existing `u32` handle contract is part of all adapters (Node/Deno/Tauri/
Python). A simple switch to `slab::Slab` with slot reuse can introduce stale-
handle aliasing unless generation is encoded.

For this rework, API stability and correctness matter more than deleting lines.

## Decision

Do **not** do a broad mechanical `HashMap -> Slab` rewrite in the same phase as
Hyper migration.

Instead:

1. Keep current handle architecture for first Hyper cut.
2. Add explicit overflow/alias guards and stronger tests now.
3. Plan a dedicated follow-up to adopt a generational-key model if/when we can
   preserve `u32` semantics cleanly.

## Ecosystem usage

- Keep using `slab` where safe and local.
- For globally exposed FFI handles, only adopt a crate-backed generational
  model when it proves strict no-alias guarantees under reuse.

## Immediate hardening tasks

1. Add upper-bound checks before composing `u32` handles.
2. Add stale-handle regression tests (cancel/remove/reallocate sequences).
3. Add deterministic stress tests for concurrent handle allocation/removal.

## Files changed (phase 1)

| File | Change |
|---|---|
| `iroh-http-core/src/stream.rs` | Guardrails + tests, no broad storage rewrite |

## Validation

```bash
cargo test -p iroh-http-core
cargo test --test integration --features compression
```

Required tests:

- `invalid_handle_after_remove`
- `no_handle_alias_under_reuse_pressure`
- `concurrent_alloc_remove_is_deterministic`

## Exit criteria

- No stale-handle aliasing under stress.
- Existing adapter contracts remain unchanged.
- Any later generational rewrite happens as an isolated change.
