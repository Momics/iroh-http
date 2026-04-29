# `iroh-http-core` — internals assessment

> **Status:** evidence base for ADR-014 and the rework epic [#156](https://github.com/Momics/iroh-http/issues/156).
> **Date:** April 2026 · branch: `main` @ `8438ac3` · scope: `crates/iroh-http-core/src/` (4,704 lines, 10 files)
>
> All line citations are 1-based against `main` at the date above. Verdicts: **keep** = stays roughly as-is · **reshape** = same responsibility, different shape · **replace** = ecosystem code does this · **question** = needs explicit decision in the rework.

## Executive summary

- **server.rs is the biggest liability.** It hand-assembles a tower stack inside a 4-fold nested if/else (compression × load-shed combinations, lines 826–1000) and bridges fallible layers to hyper through a bespoke `TowerErrorHandler` (lines 1048–1097). Both vanish under axum-shaped wiring (single `Body` newtype + infallible service contract).
- **`RequestService` mixes two responsibilities** (lines 207–533): "be a tower service" *and* "do FFI dispatch into JS handles, allocate body channels, manage the duplex upgrade hand-off". These need to split — the FFI dispatch is the legitimate custom code per ADR-013; everything around it is ecosystem.
- **Body type chain is shorter than feared but still not unified.** Inbound: `hyper::body::Incoming` → custom `pump_hyper_body_to_channel_limited` → `mpsc<Result<Bytes, _>>` → `BodyReader` (consumed by JS via FFI handle). Outbound: `BodyReader` from JS → `body_from_reader` → `BoxBody` → hyper. Two distinct concrete body shapes (incoming + outbound) plus the `BoxBody` alias. ADR-014's single `Body` newtype collapses these.
- **Pump functions are the FFI seam.** `pump_hyper_body_to_channel_limited` (server.rs), `pump_quic_recv_to_body`, `pump_body_to_quic_send`, `pump_duplex` (stream.rs) all exist because hyper bodies cannot cross FFI directly — JS holds an integer handle, not a `dyn Body`. They are reshapeable (cleaner `http_body::Body` + `Sink` impls) but not removable. Worth a follow-up issue, not the first slice.
- **Pool stays.** `pool.rs` (181 lines) is well-scoped, uses moka idiomatically, and gives us things `hyper-util::client::legacy::Client` does not (NodeId-keyed authority, single-flight on cache miss with `try_get_with`, eviction events). Not a candidate for replacement.
- **`raw_connect` is dead weight on the public surface.** Already filed as [#160](https://github.com/Momics/iroh-http/issues/160). Sessions cover the legitimate use case.

---

## Per-file findings

### `lib.rs` (352 lines)

**Purpose:** Crate root. Exports the FFI surface, defines `CoreError` / `ErrorCode`, the `BoxBody` alias, key crypto helpers, and the FFI struct shapes (`FfiResponse`, `RequestPayload`, `FfiDuplexStream`).

| Lines | Block | Verdict | Rationale |
|-------|-------|---------|-----------|
| L47–L148 | `ErrorCode` enum + `CoreError` struct + impls | keep | The single error type that crosses every FFI boundary. Stable, well-shaped. |
| L150–L167 | `pub(crate) type BoxBody` + `box_body` helper | reshape | Becomes the seed of the `Body` newtype required by ADR-014 §D2. Same data, named type with a single `From` impl per source. |
| L169–L211 | crypto + base32 helpers | keep | Thin wrappers over `ed25519-dalek` / `data-encoding`. Used by both the FFI surface and `lib.rs::node_ticket`. |
| L226–L277 | `node_ticket`, `ParsedNodeAddr`, `parse_node_addr` | keep | Iroh-specific; no ecosystem equivalent. |
| L279–L351 | FFI payload structs (`FfiResponse`, `RequestPayload`, `FfiDuplexStream`) | keep | Public FFI contract. Naming touched separately by [#143](https://github.com/Momics/iroh-http/issues/143). |

### `endpoint.rs` (927 lines)

**Purpose:** Owns `IrohEndpoint` (the god-object), all options structs, the bind/start/close lifecycle, and exposes accessors to the connection pool, handle store, transport-event bus, and per-peer subscriptions.

| Lines | Block | Verdict | Rationale |
|-------|-------|---------|-----------|
| L18–L131 | option structs (`NetworkingOptions`, `DiscoveryOptions`, `PoolOptions`, `StreamingOptions`, `NodeOptions`, `CompressionOptions`) | keep | Public configuration surface. Field-naming work is in [#143](https://github.com/Momics/iroh-http/issues/143). |
| L133–L179 | `IrohEndpoint` + `EndpointInner` declarations | reshape | Today: god-object holding iroh `Endpoint`, `ConnectionPool`, `HandleStore`, active `ServeHandle`, two atomics, transport-event bus, per-peer subscriptions. ADR-014 §D1 splits this into named, swappable components (`Transport`, `Pool`, `Handles`, `Bus`). |
| L180–L820 | `impl IrohEndpoint` — `bind`, `connect_to`, accessors, transport-event subscription, peer-stats reporting, close path | reshape | Most methods are passthroughs; the bind/start logic is fine. The split above turns these into 4–5 small impl blocks instead of one 640-line monolith. |
| L822–L829 | `classify_bind_error` | keep | Iroh-specific error mapping. |
| L831–L926 | observability structs (`NodeAddrInfo`, `EndpointStats`, `ConnectionEvent`, `PeerStats`, `PathInfo`) + `parse_direct_addrs` | keep | Public FFI shapes. |

### `server.rs` (1,098 lines)

**Purpose:** Accept loop, per-connection task spawning, in-flight counters, drain semantics, hyper http1 wiring, the tower stack assembly, and `RequestService` (mixed tower/FFI service).

| Lines | Block | Verdict | Rationale |
|-------|-------|---------|-----------|
| L36–L37 | local `BoxBody` + `BoxError` aliases | reshape | Subsumed by the unified `Body` newtype in ADR-014. |
| L47–L141 | `ServeOptions`, `ServeHandle`, `respond` | keep | Public surface; only naming/wording belongs in the rework. |
| L142–L205 | `ConnectionEventFn`, `PeerConnectionGuard` | reshape | RAII for the connection-event bus is sound; should move next to the `Bus` component in the new layout. |
| L207–L233 | `RequestService` declaration + `Service` impl | **split** | This is the keystone of the rework. Today this single struct is both "tower `Service`" and "FFI dispatch into JS handles". Split into: (a) an *infallible* tower service over the unified `Body`, (b) an `FfiDispatcher` that owns handle allocation + on_request firing + response-head rendezvous. The dispatcher is the legitimate custom code per ADR-013. |
| L233–L533 | `RequestService::handle` body | reshape | 300-line method that does header validation, peer-id stripping, channel allocation, RAII cleanup, body pumping, callback firing, response-head await, duplex upgrade, regular response shaping. After the split, only the FFI dispatch parts stay; the rest becomes standard middleware (header limits → `tower-http::limit::RequestBodyLimitLayer`, peer-id injection → tiny custom layer, etc.). |
| L585–L1045 | `serve` / `serve_with_events` accept loop | reshape | Spawn-per-connection with manual in-flight counter (L633–L635) and drain `Notify` is functional but reinvents pieces of `axum::serve` and `hyper-util::server::conn::auto`. Replace the structure with the axum-shape; keep the iroh-specific `accept_bi` loop as the `Listener` impl. The 4-fold nested if/else at L826–L1000 (compression × load-shed) collapses to a single `ServiceBuilder::new().option_layer(...).option_layer(...).service(...)` once the inner service is infallible. |
| L1048–L1097 | `TowerErrorHandler` | **replace** | Exists only because `RequestService` lets layer errors propagate to hyper. Once the inner service is infallible (axum-style), this struct disappears entirely. The 50 lines that map `Elapsed → 408` and `Overloaded → 503` become tiny `IntoResponse` impls in two lines. |

### `client.rs` (622 lines)

**Purpose:** Outbound `fetch()` with cancellation, plus `raw_connect` (HTTP `CONNECT` + `Upgrade: iroh-duplex`).

| Lines | Block | Verdict | Rationale |
|-------|-------|---------|-----------|
| L28–L48 | `HyperClientSvc` (tower wrapper around `hyper::client::conn::http1::SendRequest`) | reshape | Replace with `hyper-util::client::legacy::Client` once the iroh transport is wired as a `tower::Service<Uri, Response = IrohStream>`. Removes the manual hyper handshake at L203–L210. |
| L50–L181 | fetch token / cancellation registry | keep | FFI requirement; JS needs a `u64` handle to cancel an in-flight fetch. |
| L184–L351 | `do_fetch` body | reshape | Manual hyper handshake + `tokio::spawn(conn_task)` + `tower_http::DecompressionLayer` assembly disappears once `hyper-util::Client` does the connection management. The decompression and any future request-compression layers become a single `ServiceBuilder` that wraps `Client`. |
| L443–L483 | `body_from_reader`, `extract_path`, helpers | reshape | `body_from_reader` becomes a single `From<BodyReader> for Body` impl on the unified body type. |
| L485–L605 | `raw_connect` (`CONNECT` + `Upgrade`) | **replace (delete)** | Filed as [#160](https://github.com/Momics/iroh-http/issues/160). Zero callers in the JS adapters; sessions cover the use case. |

### `pool.rs` (181 lines)

**Purpose:** NodeId-keyed QUIC connection pool backed by `moka::future::Cache`.

| Lines | Block | Verdict | Rationale |
|-------|-------|---------|-----------|
| L19–L181 | entire file | **keep** | Idiomatic moka usage. `try_get_with` (single-flight on miss), TTL eviction, eviction-event hook to the transport bus. `hyper-util::client::legacy::Client` is keyed by `Uri` authority — we have `NodeId`, no DNS. Replacing this would mean reimplementing exactly what is already here, with worse fit. The custom-code justification (ADR-013) is "ecosystem expects DNS authority; we have public-key authority" — concrete and uncontested. |

### `stream.rs` (953 lines)

**Purpose:** `HandleStore` (slotmap-backed handle arena with TTL sweep), body channels (`BodyWriter`/`BodyReader`), pump functions for moving bytes between hyper bodies, mpsc channels, and iroh QUIC streams.

| Lines | Block | Verdict | Rationale |
|-------|-------|---------|-----------|
| L31–L48 | `SessionEntry`, `ResponseHeadEntry` | keep | Slot payload shapes; tied to the FFI handle model. |
| L50–L70 | `key_to_handle` / `handle_to_key` (slotmap key ↔ `u64`) | keep | The `u64` handle is the FFI contract. |
| L73–L152 | `BodyReader`, `BodyWriter`, `make_body_channel` | reshape | The mpsc-backed body channel is the right shape for FFI (JS pulls frames via a handle). Becomes part of the unified `Body` newtype: `Body::from_channel(reader)` / `Body::into_channel() -> writer`. |
| L154–L309 | `StoreConfig`, `Timed`, `PendingReaderEntry`, `InsertGuard`, `TrackedHandle` | keep | TTL sweep + RAII insert guard. Both are real requirements (JS may drop a handle without telling Rust; the sweep cleans up). |
| L309–L754 | `HandleStore` impl | keep | Central slotmap arena. Bug-fixed in [#151](https://github.com/Momics/iroh-http/issues/151) (slab→slotmap migration); behaviour now correct under contention. |
| L755–L953 | `pump_quic_recv_to_body`, `pump_body_to_quic_send`, `pump_duplex` (3 pump functions) | **question** | Each pump is ~30–50 lines of `loop { read; send; flush }`. They could be replaced by `impl http_body::Body for IrohRecvStream` + `impl Sink<Bytes> for IrohSendStream`, which would let the existing tower stack handle backpressure and cancellation natively. Cost: each impl is ~80 lines but reusable; saves the spawn-per-pump cost and makes the code typeable through the tower stack. **Recommendation:** out of scope for the first rework slice; file as a follow-up issue once the new server wiring is stable. |

### `session.rs` (323 lines)

**Purpose:** WebTransport-shaped session API on ALPN `iroh-http/2-duplex` (bi-streams, uni-streams, datagrams).

| Lines | Block | Verdict | Rationale |
|-------|-------|---------|-----------|
| L1–L323 | entire file | **keep** | Well-scoped, single responsibility. Mirrors WHATWG WebTransport shape for the JS surface (the user-stated non-negotiable). No ecosystem alternative; iroh's connection primitives are the right substrate. The dial/accept/incoming naming agreed in the planning conversation can land as a small rename without touching behaviour. |

### `io.rs` (62 lines)

**Purpose:** `IrohStream` — the `AsyncRead + AsyncWrite` adapter that lets hyper drive an iroh `(SendStream, RecvStream)` pair.

| Lines | Block | Verdict | Rationale |
|-------|-------|---------|-----------|
| L22–L62 | `IrohStream` + `AsyncRead` + `AsyncWrite` impls | **keep** | The legitimate Iroh transport adapter from ADR-013 §D1. No replacement exists. This is the smallest piece of code in the crate and arguably the most important. |

### `events.rs` (82 lines)

**Purpose:** `TransportEvent` enum + `now_ms` helper for the transport-event bus.

| Lines | Block | Verdict | Rationale |
|-------|-------|---------|-----------|
| L1–L82 | entire file | keep | Public observability shape. Belongs to the `Bus` component after the endpoint split. |

### `registry.rs` (104 lines)

**Purpose:** Process-global slotmap of `IrohEndpoint` instances, keyed by FFI handle.

| Lines | Block | Verdict | Rationale |
|-------|-------|---------|-----------|
| L1–L104 | entire file | keep | The single global the FFI surface owns. Tiny and correct. |

---

## Cross-cutting findings

1. **The body type chain is short but not unified.** Three concrete shapes (`hyper::body::Incoming`, `BodyReader` + mpsc channel, `BoxBody` for everything else) plus the `BoxBody` alias. ADR-014's single `Body` newtype with `From` impls for each source collapses every per-layer adapter. This is the single highest-leverage change.
2. **`TowerErrorHandler` is purely compensatory.** It exists only because `RequestService::Error = BoxError`. Make the inner service infallible (errors → `IntoResponse`) and the struct + 50 lines vanish. axum's `serve.rs` does this in one line by typing `S::Error = Infallible`.
3. **The 4-fold nested if/else (L826–L1000) is the visible cost of mixing fallible service + optional layers.** Once the inner service is infallible, the whole block becomes `ServiceBuilder::new().option_layer(comp_layer).option_layer(load_shed_layer).layer(timeout_layer).service(svc)`. Roughly 10 lines instead of 175.
4. **The accept loop reinvents `axum::serve` shape, not function.** The actual logic (accept QUIC conn → spawn → accept_bi → spawn per-stream → http1 serve_connection) is correct. What's bespoke is the in-flight counter + drain `Notify`, which `hyper-util::server::conn::auto::Builder` and `tokio_util::task::TaskTracker` solve directly. Adopting them removes ~50 lines and aligns with the ecosystem's drain semantics.
5. **`RequestService` is a god-method, not a god-object.** The 300-line `handle()` collapses into ~80 lines once header validation, body-limit, header-size limit, and peer-id injection move to layers (`tower-http::limit::RequestBodyLimitLayer`, a tiny custom `PeerIdLayer`). What remains is purely FFI dispatch — the legitimate custom code.
6. **The pump functions are real custom code, not accidental.** They exist because hyper bodies cannot cross FFI. The `http_body::Body` + `Sink` impls would be cleaner but are not strictly required for the rework's first slice. Park them as a follow-up.
7. **Outside server.rs / client.rs, the crate is in good shape.** `pool.rs`, `session.rs`, `io.rs`, `events.rs`, `registry.rs`, `lib.rs` all need at most cosmetic changes. ~3,000 of the 4,704 lines are "keep" — the rework concentrates on ~1,700 lines (server + client + endpoint reshape + body unification).

## Position on the seven open questions

1. **Split `RequestService`?** Yes. Two responsibilities, clearly separable. The new shape: `IrohHttpService` (infallible tower service over `Body`) wraps an injected `Arc<dyn FfiDispatcher>` trait object. The dispatcher owns handle allocation, on_request firing, the response-head rendezvous, and the duplex upgrade hand-off. Header validation, body limits, peer-id injection move to dedicated layers around the service.
2. **Body type chain.** Three concrete shapes + the `BoxBody` alias today. After the rework: one `Body` newtype with `From<hyper::body::Incoming>`, `From<BodyReader>`, `From<http_body_util::Empty>`, `From<http_body_util::Full<Bytes>>`. Every layer composes on `Body`. The `BoxBody` alias is renamed and re-exported for back-compat in the FFI structs (no breaking change there).
3. **`TowerErrorHandler`.** Confirmed: exists only because layers are fallible at the hyper boundary. Delete it; make `IrohHttpService::Error = Infallible`; emit `408 / 503 / 500` via `IntoResponse`-style mapping inside the timeout / load-shed layers. Net: −60 lines.
4. **Per-connection task spawning, in-flight counters, drain.** Replace the manual `AtomicUsize` + `Notify` with `tokio_util::task::TaskTracker::spawn` for the per-connection tasks; `tracker.wait()` with a timeout becomes the drain. Keep `accept_bi` as our `Listener` impl. Net: −50 lines, fewer `Arc<AtomicUsize>` clones, idiomatic shutdown.
5. **`pool.rs` vs hyper's pooling.** Keep ours. The custom-code justification is concrete: hyper-util's `Client` keys connections by `Uri` authority (DNS host + port). We have `NodeId` (Ed25519 public key, 32 bytes), no DNS, no port (QUIC multiplexes per-ALPN). Wedging hyper's pool to accept opaque keys is more code than the current 181 lines, with worse fit.
6. **Pump functions → `http_body::Body` + `Sink`.** Yes, eventually, but **not in the first slice.** The rework's first slice is the `Body` newtype + the service split + `TowerErrorHandler` removal. Folding the pumps into trait impls is a clean follow-up once that ground is settled. File as a separate issue under [#156](https://github.com/Momics/iroh-http/issues/156).
7. **Sessions — separate runtime?** Yes, and it should stay that way. Sessions and HTTP share the iroh `Endpoint` and dispatch by ALPN; that's the right seam. No code is shared *inside* the runtimes (sessions have no tower stack, no body channels, no request/response — just bi-streams, uni-streams, datagrams). Forcing a join would only contaminate both. Keep them as parallel modules consuming the same `Transport` component.

---

## Recommended slicing of the rework (input to [#156](https://github.com/Momics/iroh-http/issues/156))

Each slice is independently shippable, behind tests, and produces a smaller, more conventional codebase.

1. **Slice 1 — Unified `Body` newtype.** Introduce `crate::body::Body`, route every existing `BoxBody` through it via `From` impls. No behaviour change. Foundation for everything else. *Estimate: ~300 LOC delta, mostly mechanical.*
2. **Slice 2 — Infallible service + delete `TowerErrorHandler`.** Convert `RequestService` to `Service<Request<Body>, Response = Response<Body>, Error = Infallible>`. Move the 408/503/500 mapping into small `IntoResponse`-shaped helpers. *Estimate: −60 LOC, no behaviour change.*
3. **Slice 3 — Collapse the 4-fold if/else.** With the service infallible, replace the L826–L1000 block with a single `ServiceBuilder` chain using `option_layer`. *Estimate: −150 LOC, no behaviour change.*
4. **Slice 4 — Split `RequestService` into service + `FfiDispatcher`.** Move FFI dispatch behind a trait, isolate the legitimate custom code per ADR-013. *Estimate: ~400 LOC reshuffle, no behaviour change.*
5. **Slice 5 — Standard layers replace inline checks.** Header-size limit → `tower-http::limit`; body-size limit → `tower-http::limit::RequestBodyLimitLayer`; peer-id injection → tiny custom `PeerIdLayer`. *Estimate: −100 LOC.*
6. **Slice 6 — Adopt `TaskTracker` for accept-loop drain.** Replace `AtomicUsize` + `Notify` with `tokio_util::task::TaskTracker`. *Estimate: −50 LOC.*
7. **Slice 7 — Endpoint split.** Break `EndpointInner` into `Transport`, `Pool`, `Handles`, `Bus` components. *Estimate: ~600 LOC reshuffle, no behaviour change.*
8. **Follow-up issues (not part of #156's first cut):** `http_body::Body` + `Sink` impls for iroh streams; `hyper-util::Client` adoption on the fetch path; rename to `Dial`/`Accept`/`Incoming` per the planning conversation.

The first three slices unblock [#153](https://github.com/Momics/iroh-http/issues/153) (request decompression) — once the inner service is infallible and the if/else collapse is done, adding `RequestDecompressionLayer` is a one-line `option_layer` addition.
