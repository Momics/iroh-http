# Change 05 — Handle management: generational keys (phase 2)

## Risk: Medium — isolated from hyper migration, executed as phase 2

## Problem

The existing `u32` handle contract uses `HashMap<u32, T>` + `AtomicU32`
counters. This model has a structural weakness: slot reuse after removal can
cause stale-handle aliasing — a cancelled handle ID gets reassigned to a new
stream, and a late FFI call operates on the wrong resource.

The current mitigations (TTL sweep, manual overflow checks) are guardrails
around a fundamentally unsafe model. Since the package is unreleased, there
is no backward compatibility constraint. We should fix the model, not patch
around it.

## Decision

Adopt generational keys via the `slotmap` crate as **phase 2** of this rework,
after the hyper migration (changes 01-04, 06-07) has landed and stabilized.

Phase 2 is part of this rework plan and must complete before any release.

### Why `slotmap`

- `slotmap::SlotMap` uses `u32`-sized keys by default (`KeyData` is 32 bits:
  index + generation packed together). The FFI boundary stays `u32`.
- Stale-handle aliasing is eliminated structurally: a removed key's generation
  is bumped, so any stale handle pointing to the old generation returns `None`.
- The crate is well-maintained, `no_std`-compatible, and widely used.
- `SecondaryMap` can hold associated data (trailer channels, body writers)
  keyed to the same handle without separate slab management.

### Why phase 2, not simultaneous

The hyper migration (phase 1) changes how body/trailer data flows through the
system. The handle model determines how that data is stored and addressed.
Changing both simultaneously doubles the surface area for bugs. By landing
hyper first, the new data flow is stable and tested before the storage model
changes underneath it.

## Phase 1 — immediate hardening (during hyper migration)

1. Add upper-bound checks before composing `u32` handles.
2. Add stale-handle regression tests (cancel/remove/reallocate sequences).
3. Add deterministic stress tests for concurrent handle allocation/removal.

These tests become the regression suite that phase 2 must also pass.

## Phase 2 — generational rewrite

### Storage change

```rust
// Before:
pub(crate) struct HandleRegistry<T> {
    map: HashMap<u32, T>,
    next: AtomicU32,
}

// After:
use slotmap::{SlotMap, new_key_type};

new_key_type! { pub(crate) struct StreamHandle; }

pub(crate) struct HandleRegistry<T> {
    slots: SlotMap<StreamHandle, T>,
}
```

### FFI encoding

`StreamHandle` is 32 bits internally. The FFI boundary converts:

```rust
impl StreamHandle {
    /// Encode as u32 for FFI. The adapter holds only this integer.
    pub fn to_ffi(self) -> u32 {
        self.0.as_ffi()  // slotmap provides this
    }

    /// Decode from FFI u32. Returns None if the bits are malformed.
    pub fn from_ffi(raw: u32) -> Option<Self> {
        KeyData::from_ffi(raw).map(StreamHandle)
    }
}
```

Adapter code continues to pass `u32` handles — no JS/Python/Deno changes.

### Handle composition

The current model composes endpoint index + local index into a single `u32`
(20 bits each). With slotmap, the endpoint index can be a field on the
registry itself rather than encoded into every handle:

```rust
pub(crate) struct HandleRegistry<T> {
    ep_idx: u32,
    slots: SlotMap<StreamHandle, T>,
}
```

This eliminates the bit-packing and the associated overflow risk entirely.

## Files changed

### Phase 1

| File | Change |
|---|---|
| `iroh-http-core/src/stream.rs` | Guardrails + regression tests |

### Phase 2

| File | Change |
|---|---|
| `iroh-http-core/Cargo.toml` | Add `slotmap = "1"` |
| `iroh-http-core/src/stream.rs` | Replace `HashMap<u32, T>` + `AtomicU32` with `SlotMap` |
| `iroh-http-core/src/lib.rs` | Update handle encode/decode helpers |
| Platform adapters | No changes — `u32` FFI contract preserved |

## Validation

```bash
cargo test -p iroh-http-core
cargo test --test integration --features compression
```

Required tests (phase 1, carried into phase 2):

- `invalid_handle_after_remove`
- `no_handle_alias_under_reuse_pressure`
- `concurrent_alloc_remove_is_deterministic`

Additional phase 2 tests:

- `stale_handle_returns_none_after_generation_bump`
- `ffi_round_trip_preserves_handle_identity`

## Exit criteria

- No stale-handle aliasing under stress (structural guarantee, not guardrail).
- `u32` FFI contract preserved — all adapters pass without modification.
- Phase 2 lands after phase 1 (hyper migration) is stable and tested.
- Phase 2 completes before any public release.
