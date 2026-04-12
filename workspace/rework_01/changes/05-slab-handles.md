# Change 05 — Handle slab: replace HashMap + AtomicU32 with slab::Slab

## Risk: Medium — touches all slab call sites, mechanical but broad

## Problem

`stream.rs` manages seven independent sub-slabs with the same repeated pattern:

```rust
pub struct SlabSet {
    pub reader:          Mutex<HashMap<u32, TimestampedEntry<BodyReader>>>,
    pub reader_next:     AtomicU32,
    pub writer:          Mutex<HashMap<u32, TimestampedEntry<BodyWriter>>>,
    pub writer_next:     AtomicU32,
    pub trailer_tx:      Mutex<HashMap<u32, TimestampedEntry<TrailerTx>>>,
    pub trailer_tx_next: AtomicU32,
    pub trailer_rx:      Mutex<HashMap<u32, TimestampedEntry<TrailerRx>>>,
    pub trailer_rx_next: AtomicU32,
    pub fetch_cancel:    Mutex<HashMap<u32, Arc<tokio::sync::Notify>>>,
    pub next_fetch_id:   AtomicU32,
    pub session:         Mutex<HashMap<u32, SessionEntry>>,
    pub session_next:    AtomicU32,
    pub response_head:   Mutex<HashMap<u32, tokio::sync::oneshot::Sender<ResponseHeadEntry>>>,
    pub next_req_id:     AtomicU32,
}
```

Each `AtomicU32` counter increments on every insert and never wraps or checks
for collision. If more than 2^20 handles are created (the stream-index budget
in `compose_handle`), the counter overflows silently and handles alias.

`slab::Slab<T>` is already in the workspace (`slab = "0.4"` in
`[workspace.dependencies]`). It:
- Manages its own integer keys with O(1) insert, O(1) remove, O(1) lookup
- Reuses freed slots (prevents unbounded counter growth)
- Eliminates all seven `AtomicU32` counters

## Solution

Replace each `Mutex<HashMap<u32, T>> + AtomicU32` pair with
`Mutex<slab::Slab<T>>`:

```rust
pub struct SlabSet {
    pub reader:       Mutex<slab::Slab<TimestampedEntry<BodyReader>>>,
    pub writer:       Mutex<slab::Slab<TimestampedEntry<BodyWriter>>>,
    pub trailer_tx:   Mutex<slab::Slab<TimestampedEntry<TrailerTx>>>,
    pub trailer_rx:   Mutex<slab::Slab<TimestampedEntry<TrailerRx>>>,
    pub fetch_cancel: Mutex<slab::Slab<Arc<tokio::sync::Notify>>>,
    pub session:      Mutex<slab::Slab<SessionEntry>>,
    pub response_head: Mutex<slab::Slab<tokio::sync::oneshot::Sender<ResponseHeadEntry>>>,
}
```

All seven `AtomicU32` fields are removed.

### Insert pattern

```rust
// Before
let key = slabs.reader_next.fetch_add(1, Ordering::Relaxed);
slabs.reader.lock().unwrap().insert(key, TimestampedEntry::new(val));
compose_handle(ep_idx, key)

// After
let key = slabs.reader.lock().unwrap().insert(TimestampedEntry::new(val));
compose_handle(ep_idx, key as u32)
```

### Remove pattern

```rust
// Before
slabs.reader.lock().unwrap().remove(&handle_id)

// After
slabs.reader.lock().unwrap().try_remove(handle_id as usize)
```

### Lookup pattern

```rust
// Before
slabs.reader.lock().unwrap().get(&id).cloned()

// After
slabs.reader.lock().unwrap().get(id as usize).cloned()
```

This is a mechanical substitution. The u32 handle values at the FFI boundary
are unchanged — `compose_handle` / `decompose_handle` still produce u32.

## Files changed

| File | Change |
|---|---|
| `iroh-http-core/src/stream.rs` | Replace all 7 sub-slabs; remove all `AtomicU32` fields |
| All files calling insert/remove/lookup | Mechanical update (same u32 handle, different internals) |

Affected call sites to audit:
`insert_reader`, `insert_writer`, `insert_session_for`,
`insert_trailer_receiver`, `insert_trailer_sender`,
`remove_trailer_sender`, `next_chunk`, `send_chunk`,
`cancel_reader`, `finish_body`, `next_trailer`, `send_trailers`

## Validation

```
cargo test -p iroh-http-core
cargo test --test integration --features compression
```

All 49 integration tests must pass. Handle aliasing would cause test failures,
so passing tests confirms the slab key management is correct.

## Notes

- Remove the `AtomicU32` import from `stream.rs` if it becomes dead after
  this change.
- `slab::Slab` pre-allocates capacity in powers of two. If you need a capacity
  hint (for the slab sweep TTL logic), call `Slab::with_capacity(n)` in the
  `SlabSet::new()` constructor.
- The overflow protection is implicit: `slab::Slab` returns a `usize` key;
  casting to `u32` panics in debug mode if the slab grows beyond `u32::MAX`
  entries. In practice, the STREAM_BITS budget (2^20 = 1 048 576 simultaneous
  handles per endpoint) makes this unreachable.
