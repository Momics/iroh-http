# Embedded Portability Roadmap (ESP and similar targets)

This document explains how we can support embedded targets in the future
without blocking near-term robustness work on currently supported host
platforms.

## Context

- iroh-http runs on top of Iroh, which is QUIC-based.
- Embedded QUIC/Iroh support is still evolving.
- Today, our primary users are host platforms (Node, Deno, Tauri, Python).

Because of this, we optimise host-platform quality now while preserving clear
boundaries so an embedded implementation is feasible later.

## Goals

1. Keep protocol behaviour stable across implementations.
2. Minimise custom code where trusted crates are stronger.
3. Preserve a clean path for embedded implementations.

## Non-goals (for now)

1. Shipping an ESP implementation immediately.
2. Forcing every host-platform dependency to be `no_std`-compatible.

## Architectural boundaries that preserve future embedded support

1. Wire-level parsing/serialization must remain isolated from runtime/transport.
2. Protocol semantics must be specified by tests, not implicit host runtime behavior.
3. FFI/platform adapters must not define protocol behavior; they only map APIs.
4. Error codes and failure semantics must be canonical and cross-platform.

## Practical strategy

1. Host-first implementation:
   - Use robust, battle-tested `std` ecosystem crates where they materially
     reduce risk.
2. Embedded-ready contract:
   - Maintain wire/protocol conformance tests and test vectors.
   - Keep framing and protocol logic transport-agnostic.
3. Deferred embedded runtime:
   - Add embedded transport/runtime integration once QUIC/Iroh support is
     mature enough for production.

## Milestones

### M1 — Stabilise protocol contract on host platforms

- Canonical error-code taxonomy.
- Conformance tests for:
  - request/response framing
  - trailers
  - cancellation
  - timeout and limits
  - malformed/hostile input handling

### M2 — Extract transport-agnostic protocol surface

- Ensure wire crate has no transport/runtime coupling.
- Add golden test vectors for parse/serialize and edge-case behavior.
- Document all protocol invariants (header limits, trailer rules, stream end semantics).

### M3 — Define embedded backend interface

- Define minimal traits an embedded backend must provide (stream I/O,
  timers, cancellation, identity material, addressing hooks).
- Keep trait boundaries independent of host async runtime details.

### M4 — Pilot embedded implementation

- Build a proof-of-concept backend.
- Run conformance suite against both host and embedded backends.
- Close behavior gaps before calling embedded support stable.

## Decision rule for dependency choices today

A host-only dependency is acceptable when all are true:

1. It significantly improves correctness, safety, or maintainability now.
2. It does not erase protocol boundaries needed for embedded.
3. We can still express protocol behavior through conformance tests.
4. Any embedded impact is documented here with a mitigation plan.

## Tracking template for host-only choices

Use this when a change improves host robustness but may reduce direct embedded reuse:

- Change:
- Why this is safer/stronger now:
- Embedded impact:
- Mitigation plan:
- Conformance tests added/updated:
- Revisit trigger (what would make us re-evaluate):
