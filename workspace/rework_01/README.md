# Rework 01 — Hyper-First Rework (Source of Truth)

## Status: Approved design direction, implementation not started

---

## Summary

Replace custom HTTP machinery with the hyper/tower ecosystem wherever possible,
while preserving FFI/API compatibility and security invariants.

This plan is intentionally critical of custom code. We keep custom
implementation only where the ecosystem does not cleanly solve our exact
contract.

The rework is executed in two phases:

- **Phase 1**: Hyper migration (changes 01-04, 06-07). Replace custom HTTP
  framing, compression, and pooling with hyper/tower/moka.
- **Phase 2**: Generational handles (change 05). Replace `HashMap<u32, T>` +
  `AtomicU32` with `slotmap` generational keys and move FFI handles from `u32`
  to `u64`, eliminating stale-handle aliasing structurally. Phase 2 executes
  after phase 1 stabilizes and must complete before any public release.

No adapter API changes are required in phase 1. The FFI boundary (`fetch`,
`respond`, `next_chunk`, `send_chunk`, `finish_body`, `next_trailer`,
`send_trailers`, `raw_connect`) is preserved exactly. Phase 2 changes handle
parameters from `u32` to `u64` across all adapters (see change 05).

---

## Problem

iroh-http currently implements large parts of HTTP semantics manually on top of
Iroh QUIC streams:

- Custom chunked transfer encoding (`iroh-http-framing`)
- Custom header encoding via a stateless QPACK wrapper (`qpack_bridge.rs`)
- Custom body streaming pump functions (~300 lines)
- Custom streaming zstd compression (`compress.rs`, 255 lines)
- Custom connection pool with a bespoke single-flight mechanism (`pool.rs`)
- Custom handle slab using `HashMap<u32,T>` + `AtomicU32` pairs
- Monolithic serve accept loop with inline concurrency/timeout logic

Many of these are better owned by mature ecosystem crates.

---

## Solution

Iroh's `SendStream` and `RecvStream` implement `tokio::io::AsyncWrite` and
`AsyncRead`. hyper v1 can drive them via `hyper_util::rt::TokioIo` with one
thin stream-pair wrapper (`IrohStream`).

Core HTTP machinery moves to hyper:

- request/response parsing
- chunked body framing
- trailer frame handling
- upgrade semantics

tower/tower-http are used for middleware concerns.

Compression policy is intentionally **zstd-only** (no silent expansion to
gzip/br).

---

## Documents in this folder

| File | Contents |
|---|---|
| `README.md` | This file — overview and rationale |
| `architecture.md` | Before/after architecture, layer diagram |
| `wire-format.md` | Wire format change and ALPN versioning |
| `security-checklist.md` | Non-negotiable security and behavior parity gates |
| `changes/01-hyper-core.md` | Adopt hyper v1 as HTTP engine |
| `changes/02-tower-service.md` | Serve loop and per-request middleware via tower |
| `changes/03-tower-http-compression.md` | Replace compress.rs with zstd-only tower-http path |
| `changes/04-pool-rework.md` | Pool strategy using ecosystem cache primitives |
| `changes/05-slab-handles.md` | Generational handle model via slotmap (phase 2) |
| `changes/06-http-validation.md` | FFI input validation + typed ErrorCode enum |
| `changes/07-framing-crate.md` | Removal/deprecation strategy for iroh-http-framing |
| `embedded-tracking.md` | Host-only dependency decisions per embedded roadmap template |
| `implementation-notes.md` | Critical path patterns, gotchas, and pseudocode for tricky parts |

---

## What does NOT change

- All public function signatures: `fetch`, `respond`, `raw_connect`, `serve`,
  `next_chunk`, `send_chunk`, `finish_body`, `next_trailer`, `send_trailers`
- `ServeOptions`, `ServeHandle`, `RequestPayload`, `FfiResponse`, `FfiDuplexStream`
- `iroh-http-discovery` — completely untouched
- `session.rs` — WebTransport-style raw stream API, does not use HTTP framing,
  completely unaffected by this rework
- Ticket-based peer addressing
- Per-peer connection limiting, graceful drain, circuit breaker
- The u32 FFI handle type at the adapter boundary (changes to `u64` in phase 2)
- All platform adapters (napi-rs, PyO3, Deno FFI)

---

## Security gates (must pass before merge)

See `security-checklist.md`. In short:

1. Preserve or strengthen all resource limits.
2. Keep deterministic cancellation and trailer completion semantics.
3. Keep per-peer fairness and graceful drain behavior.
4. Add regression tests for every invariant touched by the rework.

---

## Getting started

### Prerequisites

- Rust stable (no `rust-toolchain.toml` — use current stable, minimum 1.75+
  for hyper v1 MSRV).
- The following targets are used in CI but only `aarch64-apple-darwin` or
  your native target is needed for local development.

### Verify baseline before starting

Run the full test suite on the current code to confirm a green baseline:

```bash
# Core crate
cargo test -p iroh-http-core

# Integration tests with compression
cargo test --test integration --features compression

# Node.js adapter (requires npm install)
cd packages/iroh-http-node && npm test && cd ../..

# Deno adapter
cd packages/iroh-http-deno && deno test && cd ../..

# Python adapter (requires venv)
cd packages/iroh-http-py && maturin develop && pytest && cd ../..
```

### Workspace dependencies

When adding new crates (`hyper`, `http`, `tower`, etc.), add them as
**workspace dependencies** in the root `Cargo.toml`:

```toml
[workspace.dependencies]
hyper = { version = "1", features = ["http1", "client", "server"] }
hyper-util = { version = "0.1", features = ["tokio"] }
http = "1"
http-body-util = "0.1"
tower = { version = "0.5", features = ["limit", "timeout", "util"] }
tower-http = { version = "0.6", features = ["timeout", "trace"] }
moka = { version = "0.12", features = ["future"] }
slotmap = "1"
```

Then reference them in `iroh-http-core/Cargo.toml` as:

```toml
hyper = { workspace = true }
```

### Key files to read first

| File | Why |
|---|---|
| `crates/iroh-http-core/src/lib.rs` | ALPN constants, error classification, FFI types |
| `crates/iroh-http-core/src/stream.rs` | Slab handles, body channels, trailer channels |
| `crates/iroh-http-core/src/server.rs` | Accept loop, respond(), dispatch_request() |
| `crates/iroh-http-core/src/client.rs` | fetch(), pump functions, pool usage |
| `crates/iroh-http-core/src/endpoint.rs` | IrohEndpoint, NodeOptions, max_header_size |
| `workspace/rework_01/implementation-notes.md` | Critical path patterns and gotchas |

### Branch and commit strategy

Each change document is one self-contained commit. Keep commits small and
individually reviewable. Phase 1 (changes 01-04, 06-07) can be one PR.
Phase 2 (change 05) is a separate PR after phase 1 lands.

---

## Implementation order

Changes must be applied in this order. Each is a self-contained commit:

```
01 → 06 → 02 → 03 → 04 → 07 → 05
      │         │         │         │
      │         │         │         └─ phase 2: generational handles
      │         │         └─ framing crate removed last (depends on 01-04)
      │         │
      │         └─ compression depends on tower service from 02
      │
      └─ CoreError enum needed before 02 uses it
```

### Dependency rationale

- **01 first**: everything else depends on hyper being wired to QUIC streams.
- **06 before 02**: `CoreError` / `ErrorCode` must exist before
  `RequestService` and the accept loop use them for error handling.
- **02 after 01+06**: the tower `Service` wraps hyper's `serve_connection`
  (from 01) and returns `CoreError` (from 06).
- **03 after 02**: `CompressionLayer` is added to the `ServiceBuilder` chain
  built in 02.
- **04 independent of 02/03**: pool rework only touches `pool.rs` and can
  proceed once 01 has landed. Placed here for logical grouping.
- **07 after 01-04**: framing crate removal is cleanup — only safe once all
  code paths use hyper instead.
- **05 last (phase 2)**: handle storage change runs after all data flow
  changes are stable and tested. Must complete before release.

---

- **01** (hyper core) is the foundation everything else builds on.
- **06** (http validation) can land with 01.
- **02** (tower service) lands early to establish middleware architecture.
- **03** (compression) is scoped to zstd-only behavior.
- **04** (pool) and **05** (handles) are explicit follow-up hardening steps.
- **07** (framing crate removal/deprecation) is last, after core behavior parity is verified.
