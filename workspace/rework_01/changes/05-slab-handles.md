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

### FFI handle width: `u32` → `u64`

`slotmap::KeyData` is 64 bits (32-bit index + 32-bit generation). Its FFI
helpers (`KeyData::as_ffi() -> u64`, `KeyData::from_ffi(u64)`) operate on
`u64`. Attempting to truncate to `u32` would lose the generation bits and
defeat the purpose.

Since the package is unreleased, we move the FFI handle type from `u32` to
`u64`. This is a clean break with no backward-compatibility cost.

**Runtime support for `u64` handles:**

| Runtime | Mechanism | Notes |
|---|---|---|
| Node.js (napi-rs) | `BigInt` | napi-rs supports `BigInt` natively as a parameter and return type |
| Deno FFI | `u64` / `BigInt` | Deno's FFI layer handles `u64` as `BigInt` directly |
| Python (PyO3) | `u64` | Python integers have no size limit; PyO3 maps `u64` directly |
| Tauri | Serialize as string in JSON | Tauri's `invoke()` uses JSON; `u64` serializes as string to avoid precision loss |

Handles are opaque tokens — users never do arithmetic on them. The only code
that touches handle values is the `Bridge` interface in `iroh-http-shared`,
which is internal. The change surface is type annotations only.

### Why NOT a custom u32 generational allocator

A custom allocator packing index + generation into 32 bits (e.g. 22+10)
trades one structural weakness for another: 10-bit generation wraps after
1024 reuse cycles per slot. A long-running server at moderate load wraps a
slot's generation in ~100 seconds, re-opening the aliasing window. Increasing
generation bits reduces index capacity. This is a lateral move, not a fix.

`u64` with slotmap gives 32-bit generation (4 billion cycles before wrap per
slot) — aliasing is structurally eliminated for any practical lifetime.

### Why `slotmap`

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

`StreamHandle` wraps slotmap's `KeyData` (64 bits). The FFI boundary uses
`u64`:

```rust
impl StreamHandle {
    /// Encode as u64 for FFI. The adapter holds only this integer.
    pub fn to_ffi(self) -> u64 {
        self.0.as_ffi()
    }

    /// Decode from FFI u64. Returns None if the bits are invalid.
    pub fn from_ffi(raw: u64) -> Option<Self> {
        Some(StreamHandle(KeyData::from_ffi(raw)))
    }
}
```

### Handle composition

The current model composes endpoint index + local index into a single `u32`
(20 bits each). With slotmap, the endpoint index becomes a field on the
registry itself rather than encoded into every handle:

```rust
pub(crate) struct HandleRegistry<T> {
    ep_idx: u32,
    slots: SlotMap<StreamHandle, T>,
}
```

This eliminates the bit-packing and the associated overflow risk entirely.

### Adapter changes

All adapters change handle parameter types from `u32`/`number` to
`u64`/`bigint`:

**`iroh-http-shared` (TypeScript):**
```typescript
interface Bridge {
    nextChunk(handle: bigint): Promise<Uint8Array | null>;
    sendChunk(handle: bigint, chunk: Uint8Array): Promise<void>;
    finishBody(handle: bigint): Promise<void>;
}
```

**Node.js (napi-rs):**
```rust
#[napi]
pub async fn next_chunk(handle: BigInt) -> napi::Result<Option<Buffer>> {
    let h = handle.get_u64().1;  // extract u64 from BigInt
    // ...
}
```

**Python (PyO3):**
```rust
#[pyfunction]
fn next_chunk(py: Python, handle: u64) -> PyResult<...> {
    // u64 maps directly — no change in Python caller code
}
```

## Files changed

### Phase 1

| File | Change |
|---|---|
| `iroh-http-core/src/stream.rs` | Guardrails + regression tests |

### Phase 2

| File | Change |
|---|---|
| `iroh-http-core/Cargo.toml` | Add `slotmap = "1"` |
| `iroh-http-core/src/stream.rs` | Replace `HashMap<u32, T>` + `AtomicU32` with `SlotMap`; handle params `u32` → `u64` |
| `iroh-http-core/src/lib.rs` | Update handle encode/decode helpers; remove `compose_handle`/`decompose_handle` |
| `iroh-http-core/src/server.rs` | Handle parameter types `u32` → `u64` |
| `iroh-http-core/src/client.rs` | Handle parameter types `u32` → `u64` |
| `packages/iroh-http-shared/src/bridge.ts` | `number` → `bigint` for handle parameters |
| `packages/iroh-http-node/src/lib.rs` | `u32` → `BigInt` in napi function signatures |
| `packages/iroh-http-deno/src/lib.rs` | `u32` → `u64` in FFI function signatures |
| `packages/iroh-http-py/src/lib.rs` | `u32` → `u64` in PyO3 function signatures |
| `packages/iroh-http-tauri/src/lib.rs` | `u32` → `u64`, handle JSON serialization as string |

## Validation

```bash
cargo test -p iroh-http-core
cargo test --test integration --features compression
# All adapter test suites
cd packages/iroh-http-node && npm test
cd packages/iroh-http-deno && deno test
cd packages/iroh-http-py && pytest
```

Required tests (phase 1, carried into phase 2):

- `invalid_handle_after_remove`
- `no_handle_alias_under_reuse_pressure`
- `concurrent_alloc_remove_is_deterministic`

Additional phase 2 tests:

- `stale_handle_returns_none_after_generation_bump`
- `ffi_round_trip_preserves_handle_identity`
- `u64_handle_survives_bigint_round_trip` (Node.js specific)

## Exit criteria

- No stale-handle aliasing under stress (structural guarantee, not guardrail).
- All adapters pass with `u64`/`bigint` handles.
- Phase 2 lands after phase 1 (hyper migration) is stable and tested.
- Phase 2 completes before any public release.
