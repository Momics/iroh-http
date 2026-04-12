# Embedded Compatibility Tracking

Per the template in `docs/embedded-roadmap.md`, each host-only dependency
choice is documented here.

---

## Choice 1 — hyper v1 as HTTP engine

**Change:** `iroh-http-core` adopts hyper for HTTP framing, header parsing,
chunked encoding, and Upgrade handling.

**Why this is safer/stronger now:**
- Eliminates ~1,400 lines of custom HTTP implementation
- Fuzz-tested, CVE-tracked, maintained by the Hyperium team
- Full HTTP/1.1 standard compliance (trailers, Upgrade, proper chunked encoding)
- Iroh's AsyncRead/AsyncWrite implementation allows direct integration with
  zero adapter complexity

**Embedded impact:**
- hyper requires `std` and tokio — it cannot run on embedded targets
- An embedded implementation would need to implement HTTP/1.1 framing
  independently (or use a purpose-built embedded HTTP crate)

**Mitigation plan:**
- `iroh-http-framing` is kept as the `no_std` reference implementation of the
  wire format, with golden conformance tests
- The framing crate's tests define byte-exact expected outputs for every
  encode/decode operation — an embedded reimplementation validates against
  these
- The FFI boundary (`fetch`, `respond`, `next_chunk`, etc.) is the interface
  contract; embedded targets implement the same interface differently

**Conformance tests added/updated:**
- `chunk_encoding_golden` — byte-exact chunk output
- `trailer_serialization_golden` — byte-exact trailer output
- `trailer_parse_golden` — byte-exact parse result
- All existing `trailer_round_trip`, `trailer_empty_block` tests remain

**Revisit trigger:**
- When Iroh publishes an embedded/no_std QUIC layer and there is a viable
  embedded HTTP/1.1 crate (e.g., `embedded-http` or similar matures)

---

## Choice 2 — tower-http CompressionLayer (replaces compress.rs)

**Change:** `compress.rs` (custom async zstd via `async-compression`) is
replaced by `tower_http::compression::CompressionLayer`.

**Why this is safer/stronger now:**
- Eliminates 255 lines of custom async streaming code
- Automatic `Accept-Encoding` negotiation (standards-compliant)
- Adds gzip and brotli support at no additional maintenance cost
- Maintained alongside hyper and tower

**Embedded impact:**
- tower-http requires std; not available on embedded
- Compression on embedded would need a separate implementation (zlib, miniz, etc.)

**Mitigation plan:**
- Compression is a feature flag (`compression`) — embedded builds that don't
  enable it are unaffected
- No protocol-level changes to trailer or framing semantics from this change

**Conformance tests added/updated:**
- Existing `test_compression_zstd` remains; now also covers gzip and brotli

**Revisit trigger:**
- When embedded target needs compression support

---

## Choice 3 — dashmap + tokio::sync::OnceCell (pool rewrite)

**Change:** Custom `Slot` enum + `watch` channel pool replaced by
`dashmap::DashMap` + `tokio::sync::OnceCell`.

**Why this is safer/stronger now:**
- Eliminates the subtle three-phase lock sequence
- Eliminates the `watch::Receiver` missed-wake edge case
- `OnceCell::get_or_try_init` is the canonical single-flight primitive

**Embedded impact:**
- dashmap and tokio::sync::OnceCell require std
- Embedded connection pooling would need a different approach (likely simpler
  since embedded targets typically have one or very few connections)

**Mitigation plan:**
- The pool is an internal implementation detail (`pub(crate)`) — its interface
  (`get_or_connect`) is the boundary an embedded implementation would implement

**Conformance tests added/updated:**
- `pool_single_flight` — verifies one handshake despite concurrent callers
- `pool_retry_on_immediate_close` — verifies stale-connection retry

**Revisit trigger:**
- Embedded target needs multi-connection management

---

## Choice 4 — Drop iroh-http-framing from host path

**Change:** The host path (iroh-http-core) no longer imports or uses
`iroh-http-framing` for I/O. hyper handles all framing.

**Why this is safer/stronger now:**
- Single framing implementation (hyper's) rather than two
- Removes the risk of subtle divergence between custom framing and hyper's

**Embedded impact:**
- `iroh-http-framing` continues to exist as a `no_std` crate
- It becomes the reference spec rather than the host implementation

**Mitigation plan:**
- Keep `iroh-http-framing` in the workspace
- Add httparse (no_std) for robust trailer parsing within the framing crate
- Add golden conformance tests
- Add fuzz target

**Conformance tests added/updated:**
- Golden test vectors for all encode/decode operations

**Revisit trigger:**
- Never — the framing crate is the embedded foundation, not a temporary artefact

---

## Summary table

| Choice | Crate | Embedded OK? | Mitigation |
|---|---|---|---|
| HTTP engine | `hyper` | No | Framing crate + conformance tests define the spec |
| Compression | `tower-http` | No | Feature flag; embedded implements separately |
| Pool | `dashmap` + `OnceCell` | No | Interface boundary is `get_or_connect` |
| Framing from host | Dropped | N/A | Crate kept as reference; golden tests added |
