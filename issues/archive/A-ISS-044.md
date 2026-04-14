---
id: "A-ISS-044"
title: "Error code taxonomy divergence between Rust ErrorCode and TypeScript classifyByCode"
status: duplicate
priority: P2
duplicate_of: "B-ISS-040"
date: 2026-04-14
area: core
package: "iroh-http-shared"
tags: [architecture, error-handling, protocol]
---

# [A-ISS-044] Error code taxonomy divergence between Rust ErrorCode and TypeScript classifyByCode

## Summary

TypeScript's `classifyByCode()` in `errors.ts` handles 16 distinct error code strings, but Rust's `ErrorCode` enum only defines 8 variants. The additional 8 TypeScript codes (`DNS_FAILURE`, `ALPN_MISMATCH`, `UPGRADE_REJECTED`, `PARSE_FAILURE`, `TOO_MANY_HEADERS`, `WRITER_DROPPED`, `READER_DROPPED`, `STREAM_RESET`) are generated ad-hoc by platform adapters using `format_error_json()` with free-form string codes, bypassing the typed `CoreError` system.

## Evidence

- `crates/iroh-http-core/src/lib.rs:41-49` — `ErrorCode` enum: 8 variants (`InvalidInput`, `ConnectionFailed`, `Timeout`, `BodyTooLarge`, `HeaderTooLarge`, `PeerRejected`, `Cancelled`, `Internal`)
- `crates/iroh-http-core/src/lib.rs:129-139` — `core_error_to_json()` maps these to 8 JSON strings
- `packages/iroh-http-shared/src/errors.ts` — `classifyByCode()` handles 16 codes
- `crates/iroh-http-core/src/lib.rs:149` — `format_error_json(code: &str, msg)` accepts arbitrary string codes, enabling adapters to invent codes outside the enum

## Impact

- The error taxonomy documented in `architecture.md` ("Error codes are a finite enum") is violated — the effective code space is open-ended via `format_error_json`.
- Platform adapters can introduce new codes without updating the Rust enum, breaking the guarantee that "new failure modes get new codes."
- TypeScript may handle error codes that core Rust code never generates, or miss codes that adapters generate from new paths.

## Remediation

1. Add the missing 8 error codes to Rust's `ErrorCode` enum: `DnsFailure`, `AlpnMismatch`, `UpgradeRejected`, `ParseFailure`, `TooManyHeaders`, `WriterDropped`, `ReaderDropped`, `StreamReset`.
2. Remove or deprecate `format_error_json()` — all errors crossing the FFI boundary should go through `core_error_to_json()` with a typed `CoreError`.
3. If `format_error_json()` must remain for edge cases, add a `#[deprecated]` attribute and document that all new failure modes must add an `ErrorCode` variant.

## Acceptance criteria

1. Every error code string that `classifyByCode()` handles has a corresponding `ErrorCode` variant in Rust.
2. `format_error_json()` is either removed or clearly documented as escape-hatch-only.
3. No platform adapter generates error code strings that are not in `ErrorCode`.
