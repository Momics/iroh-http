# Embedded Compatibility Tracking

Per `docs/embedded-roadmap.md`, host-only dependency choices are documented
here.

---

## Choice 1 — hyper v1 as HTTP engine

**Change:** `iroh-http-core` adopts hyper for host-side HTTP framing/parsing.

**Why stronger now:**
- Removes large custom HTTP implementation surface
- Uses maintained, widely-audited HTTP stack

**Embedded impact:**
- `hyper` is host/runtime-oriented (`std`/tokio)

**Mitigation:**
- Protocol behavior is specified by `wire-format.md` + conformance tests
- Embedded backend can implement same protocol later

**Revisit trigger:**
- Embedded QUIC/Iroh runtime becomes production-ready

---

## Choice 2 — tower-http compression (zstd-only policy)

**Change:** Replace custom compression pipeline with tower-http compression path,
keeping policy zstd-only.

**Why stronger now:**
- Eliminates bespoke async compression plumbing
- Keeps explicit policy control in middleware setup

**Embedded impact:**
- tower-http is host-focused

**Mitigation:**
- Compression remains feature-gated
- Embedded can provide its own optional compression path later

**Revisit trigger:**
- Embedded target requires compression feature parity

---

## Choice 3 — Pool strategy via ecosystem cache primitives

**Change:** Replace bespoke slot/watch orchestration with cache-backed
single-flight approach (moka).

**Why stronger now:**
- Less custom concurrency code
- Better-tested building blocks for high-contention paths

**Embedded impact:**
- Host-oriented cache/runtime dependencies

**Mitigation:**
- Pool remains internal implementation detail
- Embedded backend can use simpler pool/no-pool strategy

**Revisit trigger:**
- Embedded backend requires multi-connection pooling behavior

---

## Choice 4 — Framing crate removed/deprecated from host runtime

**Change:** Host runtime no longer depends on `iroh-http-framing` as active code.

**Why stronger now:**
- Avoids dual runtime framing implementations and spec drift

**Embedded impact:**
- No direct reusable runtime crate is kept by default

**Mitigation:**
- Protocol source-of-truth is docs + golden conformance tests
- Embedded-specific crate can be introduced when needed

**Revisit trigger:**
- Start of embedded implementation project

---

## Summary

| Choice | Crate/tool | Embedded-ready now? | Mitigation |
|---|---|---|---|
| HTTP engine | `hyper` | No | Protocol docs + conformance suite |
| Compression | `tower-http` (zstd only) | No | Feature-gated, separate embedded impl later |
| Pool | `moka` | No | Internal boundary, replaceable backend |
| Host framing runtime | removed/deprecated | N/A | Recreate embedded crate from protocol spec if needed |
