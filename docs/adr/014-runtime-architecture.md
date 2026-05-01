---
id: "014"
title: "Runtime architecture: axum-shaped layers, single Body type, FFI bridge"
status: proposed
date: 2026-04-29
area: api
tags: [server, client, ffi, refactor, tower, hyper, axum]
---

# [014] Runtime architecture: axum-shaped layers, single Body type, FFI bridge

## Context

Recent attempts at "small" middleware additions (most visibly [#153 — inbound request body decompression](https://github.com/Momics/iroh-http/issues/153)) have repeatedly turned into multi-hour Rust type-system fights against the tower / hyper composition layer. The pattern is consistent:

- We try to add a standard tower-http layer (e.g. `RequestDecompressionLayer`) into the existing stack in `crates/iroh-http-core/src/server.rs`.
- The compiler rejects it because our `BoxBody`, `RequestService`, and per-connection wiring carry several incompatible body and error types through the stack.
- We end up reinventing pieces of axum (a `TowerErrorHandler` because we let layers be fallible at the hyper boundary; per-request body type juggling because we never normalise to a single `Body` newtype).

This is the second time in two iterations (#147 + #153) that "small" middleware work has ballooned. The signal is that **`iroh-http-core`'s internal architecture is not the architecture the tower / hyper / tower-http ecosystem assumes you have**. Axum's `serve.rs` does the equivalent of our 1098-line `server.rs` in ~150 lines because it took the time to define the right seams (a single `Listener` trait, a single `Body` newtype, an infallible service contract at the hyper boundary).

The repository principles already encode "[Belong to the platform](../principles.md)" and "[Earn every concept](../principles.md)" but they speak only to the public API surface. Nothing today constrains the internal implementation to *compose* the way the ecosystem expects. ADR-013 (in flight, see #157) captures the rule. ADR-014 captures the resulting architecture.

This ADR is also informed by the mental model the maintainer reaffirmed during the planning conversation:

- The **wire format** (ALPN + HTTP/1.1 over QUIC, see [protocol.md](../protocol.md)) is a stable contract.
- The **JS-facing API** (WHATWG `fetch` and Deno `serve`, plus the WebTransport-shaped `Session` interface) is a stable contract.
- **Everything between** can be reshaped freely — that is the whole point of putting the core in Rust behind FFI bridges.

## Questions

1. What seams should `iroh-http-core` expose internally so that the standard tower / hyper / tower-http stack composes without per-layer body or error juggling?
2. Where does custom code legitimately live, and where must it not?
3. How should the single god-object (`IrohEndpoint`) be split so that "swap the runtime in a release" actually works?
4. Should the `raw_connect` HTTP `CONNECT`+`Upgrade` tunnel survive in the public surface?
5. What is the renaming for the session API in the Rust core, and how does the JS surface continue to mirror WebTransport?
6. Should the bespoke body-pump tasks (`pump_body_to_quic_send`, `pump_quic_recv_to_body`, `pump_duplex` in `stream.rs`) be replaced with `http_body::Body` and `Sink` impls?

## What we know

### Today's state (April 2026)

- `crates/iroh-http-core/src/server.rs` is **1098 lines**. It owns the accept loop, per-connection task spawning, in-flight counters, drain semantics, the tower stack assembly, and `RequestService` (which mixes "I am a tower service" with "I am the FFI dispatch into JS handles").
- `crates/iroh-http-core/src/endpoint.rs` is **927 lines**. `EndpointInner` is a god-object holding the iroh `Endpoint`, the connection pool, the `HandleStore`, the active `ServeHandle`, two atomic counters, the closed-signal channel, the transport-event bus, per-peer path subscriptions, header / body / compression configuration. Every FFI call passes `&IrohEndpoint` and reaches into whichever fields it needs.
- `crates/iroh-http-core/src/stream.rs` is **953 lines**, mostly `HandleStore` (slab + TTL sweep) plus three bespoke pump functions (`pump_body_to_quic_send`, `pump_quic_recv_to_body`, `pump_duplex`).
- `crates/iroh-http-core/src/session.rs` is **323 lines**, well-scoped, implementing dedicated non-pooled QUIC connections with WebTransport-shaped semantics (bi-streams, uni-streams, datagrams). Uses ALPN `iroh-http/2-duplex`.
- `crates/iroh-http-core/src/client.rs` `raw_connect()` is **~120 lines** and is **not actually a separate transport**: it is HTTP `CONNECT` + `Upgrade: iroh-duplex`, exactly the WebSocket handshake pattern, riding on the pooled HTTP path.
- The Node.js and Deno JS adapters wire `node.connect(peer)` to the `sessionConnect` FFI function ([packages/iroh-http-node/lib.js#L36](../../packages/iroh-http-node/lib.js#L36); [packages/iroh-http-deno/src/adapter.ts#L949](../../packages/iroh-http-deno/src/adapter.ts#L949)). `raw_connect` has **zero callers in Node/Deno**. The Tauri plugin exposes `rawConnect` but it is redundant with `node.connect`.

### What axum's runtime does in ~150 lines

- Single `Body` newtype around `UnsyncBoxBody<Bytes, BoxError>`. Every request becomes `Request<Body>` at accept time, every response is `Response<Body>` — middleware composes trivially because everyone agrees on the body type.
- The user-supplied service is required to be `Service<Request, Response = Response<B>, Error = Infallible>`. Errors from middleware are converted to responses *inside* the service via tower-http error layers and axum's `IntoResponse` machinery — never propagated to hyper.
- `Listener` trait abstracts the accept loop (TcpListener, UnixListener, custom).
- `Executor` trait abstracts task spawning.
- Per-connection: `make_service.call(IncomingStream)` produces a service, `req.map(Body::new)` normalises the body, `TowerToHyperService::new(...)` adapts to hyper, `serve_connection_with_upgrades` does the rest.

The reason it stays small is that **every architectural seam is named**.

### What iroh-http genuinely needs that axum does not

- A QUIC transport adapter (Iroh `Connection` → per-bistream `(IrohStream, NodeId)`).
- A connection pool keyed by `NodeId` (HTTP libraries assume DNS authority keys; Iroh has neither DNS nor authority).
- A `HandleStore` that turns Rust resources into `u64` handles for FFI consumption — JS cannot hold `Box<dyn Future>` or hyper bodies, so we use channel-backed handles.
- A WebTransport-shaped `Session` API for users who need datagrams or multi-stream — this is a real differentiator and has no equivalent in axum.
- Two ALPNs: `iroh-http/2` (HTTP) and `iroh-http/2-duplex` (sessions). Dispatch on incoming connections is by ALPN.

Everything else should be ecosystem code, not bespoke.

## Decisions

### D1 — Five named layers, one crate, strict module boundaries

```text
┌───────────────────────────────────────────────────────────────┐
│ FFI surface (lib.rs re-exports)                                │
└──────────────────────────┬────────────────────────────────────┘
                           │
┌──────────────────────────▼────────────────────────────────────┐
│ FfiBridge                                                      │
│   HandleStore + body channels + event bus                      │
│   BodyReader: impl http_body::Body                             │
│   BodyWriter: impl Sink<Bytes>                                 │
└──────────┬───────────────────────────────┬────────────────────┘
           │                               │
┌──────────▼─────────────┐    ┌────────────▼────────────────────┐
│ HttpRuntime            │    │ SessionRuntime                   │
│ - serve()              │    │ - dial / accept / incoming       │
│ - fetch()              │    │ - bi/uni streams, datagrams      │
│ axum-shaped runtime    │    │ (WebTransport-shaped, dedicated  │
│ + tower-http stack     │    │  non-pooled QUIC connections)    │
│ ALPN: iroh-http/2      │    │ ALPN: iroh-http/2-duplex         │
└──────────┬─────────────┘    └────────────┬────────────────────┘
           │                               │
           └───────────────┬───────────────┘
                           │
┌──────────────────────────▼────────────────────────────────────┐
│ Transport                                                      │
│   iroh::Endpoint + ConnectionPool + IrohStream                 │
└────────────────────────────────────────────────────────────────┘
```

**`Node`** is a thin facade that holds `Transport` + optional `HttpRuntime` + optional `SessionRuntime` + `FfiBridge`. Each subsystem is testable in isolation. The crate stays as one crate with strict module boundaries (publishing overhead of multiple crates outweighs the compile-time enforcement benefit at this stage).

### D2 — Single `Body` newtype, axum-shaped service contract

- Define one internal `Body` newtype: `pub(crate) struct Body(UnsyncBoxBody<Bytes, BoxError>);` with `impl http_body::Body`.
- `BoxError` is `pub(crate) use tower_http::BoxError;` so error types unify by identity, not by structural equality (this is what bit us in #153).
- Inside the runtime, every request becomes `Request<Body>` immediately after accept, every response is `Response<Body>`.
- The innermost service is `Service<Request<Body>, Response = Response<Body>, Error = Infallible>`. Errors are converted to responses **inside** the service. Hyper never sees an `Err`.
- The standard tower-http stack (`CompressionLayer`, `RequestDecompressionLayer`, `TimeoutLayer`, `ConcurrencyLimitLayer`, `LoadShedLayer`, `AddExtensionLayer`) is composed once, type-erased once via `BoxCloneSyncService`, and shared across connections.

This is **directly inspired by axum's [`serve.rs`](https://github.com/tokio-rs/axum/blob/main/axum/src/serve/mod.rs)**. The `serve()` module in `iroh-http-core` will carry a header comment to that effect so future maintainers can compare against the reference and pull in improvements when axum evolves.

### D3 — Drop `raw_connect` from the public surface

- Zero callers in Node and Deno; redundant with `node.dial(peer)` in Tauri.
- `iroh-http/2-duplex` ALPN is **kept** because the session runtime needs it.
- HTTP `CONNECT`+`Upgrade` continues to work at the wire level (hyper supports it natively). We just no longer expose a Rust function or FFI verb to initiate it. If real demand surfaces, it is ~120 lines to re-add and lives behind a feature flag.

Tracked in a separate follow-up issue so it lands as its own commit.

### D4 — Replace bespoke pump tasks with `Body` and `Sink` impls

- `BodyReader` becomes `impl http_body::Body<Data = Bytes, Error = BoxError>` so hyper consumes it directly. No more `pump_quic_recv_to_body` task; hyper polls the channel.
- `BodyWriter` becomes `impl futures::Sink<Bytes, Error = BoxError>` so writers feed it through standard tokio-util adapters. The QUIC send path becomes a `tokio::io::copy_buf` (or equivalent) over the standard adapters instead of a hand-rolled pump loop.
- `pump_duplex` (for the `raw_connect` upgraded socket) goes away with `raw_connect` itself.

Net effect: ~150 lines of bespoke glue removed from `stream.rs`, behaviour preserved.

> **Status update (post-Slice E, epic #182; resolved 2026-05).** The
> `BodyReader: http_body::Body` half landed in `6fb9c1b`; hyper now
> polls `BodyReader` directly on both serve and fetch paths. The
> `pump_duplex` half landed when `raw_connect` was dropped. The
> `BodyWriter: Sink<Bytes>` half landed in `26adf78` — producers now
> compose with stock `futures` combinators via `forward`.
>
> The original perf motivation for D4 (eliminating a channel hop on the
> *internal* hyper path) was overtaken by Slices C/D: the surviving pumps
> (`pump_body_to_quic_send`, `pump_quic_recv_to_body`) only run on FFI
> session streams where JS adapters explicitly observe the channel
> boundary, so lazy insertion saves no work.
>
> The remaining *elegance* half — replacing the two raw-byte pumps with
> `Sink`/`Stream` adapters over `iroh::endpoint::SendStream` /
> `RecvStream` — was prototyped end-to-end and **rejected**: net +74 LoC
> vs the bespoke pumps, with less-readable call sites. `SendStream` is
> `&mut`-bound and stream-sequential, so a `Sink<Bytes>` impl has to
> invent the shape (Option-slot for ownership shuffling + boxed
> in-flight `write_all`) rather than expose one that is already there —
> the structural opposite of `mpsc::Sender` + `PollSender`. ADR-013
> ("lean on the ecosystem") applies when the shape exists; here it does
> not. The two pumps stay as imperative loops with backlinks to #174's
> closing comment for the LoC analysis.
>
> `pump_hyper_body_to_channel_limited` also stays as-is: it carries real
> FFI policy (byte-limit overflow oneshot, per-frame timeout) that no
> stock adapter encodes.

### D5 — Naming

**Rust core (idiomatic Rust, mirrors Quinn / tokio / libp2p):**

| Today | New |
|---|---|
| `IrohEndpoint` | `Node` |
| `EndpointInner` (god-object) | split into `Transport`, `HttpRuntime`, `SessionRuntime`, `FfiBridge` |
| `session_connect` | `Node::dial(peer) -> Session` |
| `session_accept` | `Node::accept() -> Option<Session>` (single) and incoming iterator helper |
| `session_create_bidi_stream` | `Session::open_bi() -> (Send, Recv)` |
| `session_next_bidi_stream` | `Session::accept_bi() -> Option<(Send, Recv)>` |
| `session_create_uni_stream` | `Session::open_uni() -> Send` |
| `session_next_uni_stream` | `Session::accept_uni() -> Option<Recv>` |
| `session_send_datagram` | `Session::send_datagram(bytes)` |
| `session_recv_datagram` | `Session::recv_datagram() -> Option<Bytes>` |
| `session_close` | `Session::close(code, reason)` |
| `session_closed` | `Session::closed() -> CloseInfo` |
| `session_ready` | `Session::ready()` |

**JS surface (continues to mirror [W3C WebTransport](https://www.w3.org/TR/webtransport/) verbatim, with two P2P-specific additions):**

| Concept | JS API |
|---|---|
| Open session to a peer (P2P-specific — no URL, just node id) | `node.dial(peer)` → `Session` |
| Accept incoming sessions (P2P-specific) | `node.incoming` (async iterable of `Session`) |
| Open bidi stream | `session.createBidirectionalStream()` |
| Incoming bidi streams | `session.incomingBidirectionalStreams` |
| Open uni stream | `session.createUnidirectionalStream()` |
| Incoming uni streams | `session.incomingUnidirectionalStreams` |
| Datagrams | `session.datagrams` (`{ readable, writable }`) |
| Close | `session.close({ closeCode, reason })` |
| Closed promise | `session.closed` |
| Ready promise | `session.ready` |

The FFI bridge is exactly the place where the Rust idiom and the WebTransport mirror translate into each other. Neither side bends for the other.

### D6 — Custom code is allowed only in three places

(Per ADR-013 — see #157.)

1. The **Transport layer** (Iroh adapter, pool, IrohStream).
2. The **FfiBridge layer** (HandleStore, body channels, event bus, `Body`/`Sink` impls for FFI-backed bodies).
3. The **public JS-facing types** (must mirror WHATWG / WebTransport / Deno).

Anywhere else, custom code requires a one-paragraph justification comment that references ADR-013 and explains why the ecosystem option does not fit. The "stop signal" applies: more than ~2 compile iterations fighting tower/hyper type or lifetime errors means you are off-pattern; stop and look at how axum / hyper-util do it.

### D7 — Wire format is unchanged in this rework

- ALPN values: `iroh-http/2` (HTTP) and `iroh-http/2-duplex` (sessions) — both retained.
- HTTP/1.1 framing over QUIC bi-stream — unchanged.
- `httpi://` URL scheme — unchanged.

This rework is internal. No ALPN bump.

### D8 — JS-facing API surface is preserved except where explicitly changed

- `node.fetch(url, init)`, `node.serve(handler)` — unchanged.
- `node.connect(peer)` → renamed to `node.dial(peer)`. Documented as a breaking change in CHANGELOG (project is pre-1.0 with explicit "breaking changes possible" notice).
- `node.sessions` (current async iterable) → renamed to `node.incoming`.
- All other `Session` methods/properties unchanged (continue mirroring WebTransport).

## Sequencing

The rework lands in tracer-bullet vertical slices, each in its own PR with green CI. Likely order:

1. Introduce single `Body` newtype, `BoxError` re-export, no behaviour change. (Foundation.)
2. Convert inner service to `Error = Infallible`; move error → response inside the service. Remove `TowerErrorHandler`.
3. Replace bespoke per-connection wiring with the standard tower-http stack composed once, type-erased once. Closes #153 (request decompression) as a one-line addition that validates the architecture.
4. Replace bespoke pump tasks with `Body` / `Sink` impls.
5. Drop `raw_connect` from the public surface (separate small follow-up issue, can land in any order after #1).
6. Split `IrohEndpoint` god-object into `Node` + `Transport` + `HttpRuntime` + `SessionRuntime` + `FfiBridge`.
7. Rename `session_*` → `Session::*`; rename `node.connect` → `node.dial`; rename `node.sessions` → `node.incoming`.

Each slice is independently reviewable. If any slice surfaces an issue with the design, this ADR gets updated rather than the slice merged.

## References

- [ADR-013 — lean on the ecosystem](013-lean-on-the-ecosystem.md) (in flight, #157)
- [docs/architecture.md](../architecture.md) — current architecture (will be updated as slices land)
- [docs/principles.md](../principles.md) — engineering values; "Belong to the platform" and "Earn every concept" both apply
- [axum/src/serve/mod.rs](https://github.com/tokio-rs/axum/blob/main/axum/src/serve/mod.rs) — the reference implementation we are mirroring
- [W3C WebTransport](https://www.w3.org/TR/webtransport/) — the reference spec for the JS `Session` API
- Epic [#156](https://github.com/Momics/iroh-http/issues/156)
- Assessment [#158](https://github.com/Momics/iroh-http/issues/158)
- Adoption [#159](https://github.com/Momics/iroh-http/issues/159)
- Deferred bug [#153](https://github.com/Momics/iroh-http/issues/153)
