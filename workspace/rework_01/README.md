# Rework 01 — Adopt the hyper/tower ecosystem

## Status: Design complete, implementation not started

---

## Summary

Replace the custom HTTP machinery in `iroh-http-core` and `iroh-http-framing`
with the hyper/tower/tower-http ecosystem. The result is a codebase with
dramatically less custom Rust, better HTTP standards compliance, and the same
public API surface visible to all platform adapters.

No platform adapter changes are required. The FFI boundary (`fetch`,
`respond`, `next_chunk`, `send_chunk`, `finish_body`, `next_trailer`,
`send_trailers`, `raw_connect`) is preserved exactly.

---

## Problem

iroh-http currently implements HTTP from scratch on top of Iroh QUIC streams:

- Custom chunked transfer encoding (`iroh-http-framing`)
- Custom header encoding via a stateless QPACK wrapper (`qpack_bridge.rs`)
- Custom body streaming pump functions (~300 lines)
- Custom streaming zstd compression (`compress.rs`, 255 lines)
- Custom connection pool with a bespoke single-flight mechanism (`pool.rs`)
- Custom handle slab using `HashMap<u32,T>` + `AtomicU32` pairs
- Monolithic serve accept loop with inline concurrency/timeout logic

Every one of these is a re-implementation of something the Rust ecosystem
already solves in a production-grade, fuzz-tested, maintained way.

---

## Solution

Iroh's `SendStream` and `RecvStream` implement `tokio::io::AsyncWrite` and
`AsyncRead` respectively (they are `iroh-quinn` wrappers over Quinn 0.11
streams). This means **hyper v1 can drive them directly** — no adapters needed.

The HTTP machinery disappears. hyper handles framing, headers, chunked
encoding, trailers, and Upgrade semantics. tower-http handles compression and
decompression as middleware layers. The codebase becomes an integration layer
between Iroh's P2P QUIC transport and standard HTTP semantics.

---

## Documents in this folder

| File | Contents |
|---|---|
| `README.md` | This file — overview and rationale |
| `architecture.md` | Before/after architecture, layer diagram |
| `wire-format.md` | Wire format change and ALPN versioning |
| `changes/01-hyper-core.md` | Adopt hyper v1 as HTTP engine |
| `changes/02-tower-service.md` | Serve loop and per-request middleware via tower |
| `changes/03-tower-http-compression.md` | Replace compress.rs with tower-http layers |
| `changes/04-pool-rework.md` | Connection pool: dashmap + OnceCell |
| `changes/05-slab-handles.md` | Handle slab: slab crate |
| `changes/06-http-validation.md` | FFI input validation via http crate |
| `changes/07-framing-crate.md` | What happens to iroh-http-framing |
| `embedded-tracking.md` | Host-only dependency decisions per embedded roadmap template |

---

## What does NOT change

- All public function signatures: `fetch`, `respond`, `raw_connect`, `serve`,
  `next_chunk`, `send_chunk`, `finish_body`, `next_trailer`, `send_trailers`
- `ServeOptions`, `ServeHandle`, `RequestPayload`, `FfiResponse`, `FfiDuplexStream`
- `iroh-http-discovery` — completely untouched
- Ticket-based peer addressing
- Per-peer connection limiting, graceful drain, circuit breaker
- The u32 handle model at the FFI boundary
- All platform adapters (napi-rs, PyO3, Deno FFI)

---

## Implementation order

Changes must be applied in this order. Each is a self-contained commit:

```
01 → 06 → 07 → 05 → 04 → 02 → 03
```

- **01** (hyper core) is the foundation everything else builds on.
- **06** (http validation) can go in alongside 01.
- **07** (framing crate) becomes trivial once 01 lands.
- **05** (slab) and **04** (pool) are independent of each other, both after 01.
- **02** (tower service) depends on the handle slab being clean (05).
- **03** (compression) is the last layer, sits on top of 02.
