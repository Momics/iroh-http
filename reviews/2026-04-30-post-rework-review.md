# Post-Rework Architectural Review

**Date:** 2026-04-30  
**Scope:** `crates/iroh-http-core/src/{server,client,body,stream,endpoint}.rs` against ADR-013 & ADR-014  
**Reviewer:** Copilot

---

## 1. Verdict

**Mostly.** The foundational promises shipped—`Body` newtype, infallible service contract, standard tower-http stack—but server.rs remains bloated (1120 lines vs ~150 axum), pump tasks survive, and `EndpointInner` was never split. The architecture is *composable now* but not *concise yet*.

---

## 2. ADR-013/014 Promises vs What Shipped

| Promise | Status | Evidence |
|---------|--------|----------|
| D2: Single `Body` newtype | ✅ | [body.rs#L32](crates/iroh-http-core/src/body.rs#L32) — `pub struct Body(UnsyncBoxBody<Bytes, BoxError>)` |
| D2: `Error = Infallible` service | ✅ | [server.rs#L264](crates/iroh-http-core/src/server.rs#L264) — `type Error = std::convert::Infallible` |
| D2: Standard tower-http stack | ✅ | [server.rs#L848-958](crates/iroh-http-core/src/server.rs#L848-958) — `CompressionLayer`, `RequestDecompressionLayer`, `RequestBodyLimitLayer`, `TimeoutLayer`, `LoadShedLayer` |
| D3: Drop `raw_connect` | ✅ | [client.rs](crates/iroh-http-core/src/client.rs) — only `fetch` remains |
| D4: Replace pump tasks with Body/Sink impls | ❌ | [stream.rs#L755-830](crates/iroh-http-core/src/stream.rs#L755-830) — `pump_quic_recv_to_body`, `pump_body_to_quic_send`, `pump_duplex` still exist |
| D1: Split EndpointInner god-object | ❌ | [endpoint.rs#L29-71](crates/iroh-http-core/src/endpoint.rs#L29-71) — still a 17-field struct |

---

## 3. Where We Are Better Than Typical

- **Compression predicate honesty** ([server.rs#L863-906](crates/iroh-http-core/src/server.rs#L863-906)): respects `Content-Encoding` (skip pre-compressed), `Cache-Control: no-transform`, and media types (`image/*`, `audio/*`, `video/*`, `application/zstd`). Most naive setups forget the `no-transform` check.
- **Request body limit via tower-http** ([server.rs#L924-937](crates/iroh-http-core/src/server.rs#L924-937)): uses `RequestBodyLimitLayer` instead of hand-rolled counter—prevents memory exhaustion without reinventing the wheel.
- **Shared concurrency semaphore** ([server.rs#L679-682](crates/iroh-http-core/src/server.rs#L679-682)): `ConcurrencyLimitLayer` built once and shared across connections—true global cap.
- **Observability counters** ([endpoint.rs#L57-62](crates/iroh-http-core/src/endpoint.rs#L57-62)): `active_connections`, `active_requests` atomics exposed to FFI stats API.

---

## 4. Remaining Smells

| Location | Issue |
|----------|-------|
| [server.rs](crates/iroh-http-core/src/server.rs) (1120 lines) | 7× axum's ~150-line equivalent. Structural cause: accept loop, per-connection layer assembly, FFI dispatcher, drain logic, and `HandleLayerError` live in one file. |
| [server.rs#L848-970](crates/iroh-http-core/src/server.rs#L848-970) | Layer stack assembled **inline inside the accept loop** rather than extracted to a builder or composed once at startup. Duplicates `#[cfg]` branching for compression. |
| [server.rs#L863-906](crates/iroh-http-core/src/server.rs#L863-906) | Custom predicate closures instead of `tower_http::compression::DefaultPredicate::default().and(...)`. The logic is correct but the composition is bespoke. |
| [stream.rs#L755-830](crates/iroh-http-core/src/stream.rs#L755-830) | Bespoke pump tasks (~80 lines). ADR-014 D4 specified `BodyReader: impl http_body::Body`, `BodyWriter: impl Sink`—never landed. |
| [endpoint.rs#L29-71](crates/iroh-http-core/src/endpoint.rs#L29-71) | `EndpointInner` remains a god-object (pool, handles, serve_handle, stats, events, path_subs, compression). ADR-014 D1 named five layers—not split. |
| [stream.rs#L500-550](crates/iroh-http-core/src/stream.rs#L500) | `BodyReader` / `BodyWriter` still channel-backed helpers, not `http_body::Body` impls—hyper polls an adapter, not the channel directly. Hidden allocation per chunk. |

---

## 5. Concrete Next-Issue Recommendations

### 5.1 `refactor(server): extract layer stack builder from accept loop`

**Rationale:** The tower stack (compression, decompression, limit, timeout, load-shed) is rebuilt per-connection with redundant `#[cfg]` branches. Extract a `fn build_service_stack(base: IrohHttpService, opts: &ServeOptions) -> impl Service` that's called once; clone the resulting `BoxCloneService` per connection. Cuts ~120 lines and removes conditional-compilation noise from the hot path.

**Labels:** `refactor`, `rust`, `P2`

### 5.2 `refactor(stream): replace pump tasks with Body/Sink impls`

**Rationale:** `pump_quic_recv_to_body` and `pump_body_to_quic_send` are ~35 lines each of async glue that could be zero-cost `impl http_body::Body` / `impl Sink`. Landing this closes ADR-014 D4 and removes an allocation-per-frame hop.

**Labels:** `refactor`, `rust`, `P3`

### 5.3 `refactor(endpoint): split EndpointInner into named subsystems`

**Rationale:** ADR-014 D1 proposed `Transport`, `HttpRuntime`, `SessionRuntime`, `FfiBridge`. Today all 17 fields live in one struct. Splitting makes each subsystem independently testable and reduces cognitive load when touching just one concern.

**Labels:** `refactor`, `rust`, `P3`

### 5.4 `refactor(server): use DefaultPredicate.and() for compression`

**Rationale:** The predicate logic (skip pre-compressed, no-transform, opaque types) is correct but hand-written. `tower_http::compression::predicate::DefaultPredicate` already handles size-above and can chain with `.and()`. Using it documents intent via well-known types and may pick up upstream improvements.

**Labels:** `refactor`, `rust`, `P4`

### 5.5 `docs(compression): document zstd-only policy and Deno/browser differences`

**Rationale:** iroh-http supports only zstd (not gzip/brotli). This is intentional (lower CPU, better ratio) but undocumented. Callers expecting `Accept-Encoding: gzip` negotiation will be confused. A short section in `docs/features/compression.md` explaining the choice and fallback behaviour (no compression when client lacks zstd) removes ambiguity.

**Labels:** `documentation`, `P4`

---

## 6. Compression: Comparison with Deno / MDN Best Practices

| Aspect | Deno | MDN Recommendation | iroh-http |
|--------|------|--------------------|-----------|
| Algorithms | gzip, brotli | gzip, brotli (zstd emerging) | **zstd-only** — deliberate choice for better ratio + speed |
| Min body size | 64 bytes | none specified | 512 bytes (doc says 1 KB—see issue #162) |
| Vary header | auto | required | ✅ tower-http `CompressionLayer` emits `Vary: Accept-Encoding` |
| Content-Length stripping | implicit (streaming) | required when chunked | ✅ tower-http drops it on compressed streaming responses |
| Skip pre-compressed | yes | yes | ✅ [server.rs#L863-869](crates/iroh-http-core/src/server.rs#L863-869) |
| Skip `no-transform` | yes | yes | ✅ [server.rs#L870-880](crates/iroh-http-core/src/server.rs#L870-880) |
| Skip opaque media | yes (mime-db list) | yes | ✅ (simpler list: image/audio/video + octet-stream) |

**Gap:** zstd-only means older browsers without zstd support receive uncompressed responses. For P2P use-cases (Tauri, Node, Deno) this is fine—those runtimes ship modern zstd. For future browser-adjacent targets, consider feature-gated gzip fallback.
