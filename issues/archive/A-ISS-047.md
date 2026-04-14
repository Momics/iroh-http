---
id: "A-ISS-047"
title: "Discovery error type uses String instead of CoreError"
status: duplicate
priority: P2
duplicate_of: "A-ISS-048"
date: 2026-04-14
area: core
package: "iroh-http-discovery"
tags: [architecture, error-handling, consistency]
---

# [A-ISS-047] Discovery error type uses String instead of CoreError

## Summary

`iroh-http-discovery`'s public functions `start_browse()` and `start_advertise()` return `Result<_, String>` instead of `Result<_, CoreError>`. This forces every caller to convert the raw string into a structured error, bypassing the error taxonomy (ErrorCode) that `iroh-http-core` provides.

## Evidence

- `crates/iroh-http-discovery/src/lib.rs:92` — `pub async fn start_browse(...) -> Result<BrowseSession, String>`
- `crates/iroh-http-discovery/src/lib.rs:125` — `pub fn start_advertise(...) -> Result<AdvertiseSession, String>`
- `crates/iroh-http-core/src/lib.rs:55-115` — `CoreError` provides structured error codes
- `docs/principles.md` §6 — "Errors are values, not afterthoughts. Every fallible operation surfaces its failure."

## Impact

- Platform adapters receiving `Err(String)` from discovery cannot classify the error into `CoreError::connection_failed` vs `CoreError::invalid_input` — they must wrap it as a generic string error.
- String-based errors make it impossible for TypeScript's `classifyByCode()` to apply the correct error class.
- Inconsistent with every function in `iroh-http-core` which returns `Result<_, CoreError>`.

## Remediation

1. Add `iroh-http-core` as a dependency of `iroh-http-discovery` (or use a shared error crate).
2. Change both functions to return `Result<_, CoreError>`.
3. Classify the mDNS builder errors as `CoreError::invalid_input` (config errors) or `CoreError::connection_failed` (runtime errors).

Alternative: If keeping the crates independent is important, define a `DiscoveryError` enum in `iroh-http-discovery` that maps cleanly to `CoreError` at the adapter layer.

## Acceptance criteria

1. `start_browse` and `start_advertise` return a typed error, not `String`.
2. Platform adapters can classify discovery errors using the same error taxonomy as core errors.
