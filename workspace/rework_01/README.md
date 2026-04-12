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
  `AtomicU32` with `slotmap` generational keys, eliminating stale-handle
  aliasing structurally. Phase 2 executes after phase 1 stabilizes and must
  complete before any public release.

No adapter API changes are required. The FFI boundary (`fetch`,
`respond`, `next_chunk`, `send_chunk`, `finish_body`, `next_trailer`,
`send_trailers`, `raw_connect`) is preserved exactly.

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
- The u32 FFI contract at the adapter boundary (internal storage changes in phase 2)
- All platform adapters (napi-rs, PyO3, Deno FFI)

---

## Security gates (must pass before merge)

See `security-checklist.md`. In short:

1. Preserve or strengthen all resource limits.
2. Keep deterministic cancellation and trailer completion semantics.
3. Keep per-peer fairness and graceful drain behavior.
4. Add regression tests for every invariant touched by the rework.

---

## Implementation order

Changes must be applied in this order. Each is a self-contained commit:

```
01 → 06 → 02 → 03 → 04 → 05 → 07
```

- **01** (hyper core) is the foundation everything else builds on.
- **06** (http validation) can land with 01.
- **02** (tower service) lands early to establish middleware architecture.
- **03** (compression) is scoped to zstd-only behavior.
- **04** (pool) and **05** (handles) are explicit follow-up hardening steps.
- **07** (framing crate removal/deprecation) is last, after core behavior parity is verified.
