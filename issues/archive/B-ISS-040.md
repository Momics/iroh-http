---
id: "B-ISS-040"
title: "JS error classifier has unreachable branches — real errors fall to UNKNOWN"
status: fixed
priority: P0
date: 2026-04-13
area: core
package: iroh-http-shared
tags: [errors, correctness, ffi, protocol]
---

# [B-ISS-040] JS error classifier has unreachable branches — real errors fall to UNKNOWN

## Summary

`classifyByCode` in `errors.ts` handles 15+ error codes that are never emitted by the Rust layer. Real failure conditions (ALPN mismatch, parse failure, stream reset, etc.) fall into the `UNKNOWN` catch-all. Callers matching on specific error subclasses will silently receive `IrohError { code: "UNKNOWN" }` instead of the intended typed class.

The root cause is a taxonomy divergence: TypeScript's `classifyByCode()` handles 16 distinct codes, but Rust's `ErrorCode` enum only defines 8 variants. The gap exists because `format_error_json(code: &str, msg)` accepts arbitrary string codes, enabling adapters to invent codes outside the enum — but in practice they rarely do, leaving the TypeScript branches dead.

## Evidence

- `packages/iroh-http-shared/src/errors.ts` — `classifyByCode` has branches for `DNS_FAILURE`, `ALPN_MISMATCH`, `PARSE_FAILURE`, `UPGRADE_REJECTED`, `WRITER_DROPPED`, `READER_DROPPED`, `INVALID_KEY`, `INVALID_ARGUMENT`, `TOO_MANY_HEADERS`, `STREAM_RESET`, `ABORTED`
- `crates/iroh-http-core/src/lib.rs:133` — `ErrorCode` enum only serialises to: `INVALID_INPUT`, `REFUSED`, `TIMEOUT`, `BODY_TOO_LARGE`, `HEADER_TOO_LARGE`, `PEER_REJECTED`, `CANCELLED`, `UNKNOWN`
- `crates/iroh-http-core/src/lib.rs:149` — `format_error_json(code: &str, msg)` accepts arbitrary string codes, enabling adapters to invent codes outside the enum
- `packages/iroh-http-node/src/lib.rs:49,187,213,250` — adapters only emit `ENDPOINT_FAILURE`, `INVALID_HANDLE`, `REFUSED`, `UNKNOWN` via `format_error_json`
- `docs/architecture.md` — "New failure modes get new codes — never rely on catch-all"

## Impact

Any Rust error not in the small emitted set is classified as `IrohError { code: "UNKNOWN" }`. Callers using `instanceof IrohConnectError`, `instanceof IrohStreamError`, or switching on `.code` silently miss real failures. The error model described in architecture.md is not operational.

## Remediation

1. Audit every failure path in all adapters and core to produce the complete list of codes actually emitted at runtime.
2. For each branch in `classifyByCode` with no emitting counterpart in Rust: either add a `format_error_json` call in Rust, or remove the dead branch.
3. Add the missing error codes to Rust's `ErrorCode` enum (`DnsFailure`, `AlpnMismatch`, `UpgradeRejected`, `ParseFailure`, `TooManyHeaders`, `WriterDropped`, `ReaderDropped`, `StreamReset`) so the taxonomy is closed and typed.
4. Remove or deprecate `format_error_json()` — all errors crossing the FFI boundary should go through `core_error_to_json()` with a typed `CoreError`.
5. Add a round-trip test in `errors.test.ts` asserting that every emitted Rust code maps to the correct JS subclass.

## Acceptance criteria

1. Every `case` in `classifyByCode` is emitted by at least one Rust path (verified by test or grep).
2. No real failure condition produces `IrohError { code: "UNKNOWN" }` when a specific type applies.
3. Round-trip tests cover the full `classifyError` dispatch table.

## Absorbed

- **A-ISS-044** (duplicate) — same problem described as "error code taxonomy divergence between Rust and TypeScript."
