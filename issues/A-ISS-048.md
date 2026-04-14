---
id: "A-ISS-048"
title: "Public API functions return Result<_, String> instead of Result<_, CoreError>"
status: fixed
priority: P2
date: 2026-04-14
area: core
package: "iroh-http-core"
tags: [architecture, error-handling, consistency]
---

# [A-ISS-048] Public API functions return Result<_, String> instead of Result<_, CoreError>

## Summary

Several public API functions return `Result<_, String>` instead of `Result<_, CoreError>`, inconsistent with every other public function in `iroh-http-core` (`fetch`, `serve`, `session_connect`, etc.) which all return `Result<_, CoreError>`.

**Affected functions:**
- `IrohEndpoint::bind()` — the primary entry point for creating a node
- `iroh-http-discovery::start_browse()` — mDNS browse session
- `iroh-http-discovery::start_advertise()` — mDNS advertise session

## Evidence

- `crates/iroh-http-core/src/endpoint.rs` — `pub async fn bind(opts: NodeOptions) -> Result<Self, String>`
- `crates/iroh-http-discovery/src/lib.rs:92` — `pub async fn start_browse(...) -> Result<BrowseSession, String>`
- `crates/iroh-http-discovery/src/lib.rs:125` — `pub fn start_advertise(...) -> Result<AdvertiseSession, String>`
- `crates/iroh-http-core/src/client.rs` — `pub async fn fetch(...) -> Result<FfiResponse, CoreError>` (follows convention)
- `crates/iroh-http-core/src/server.rs` — `pub fn respond(...) -> Result<(), CoreError>` (follows convention)
- `crates/iroh-http-core/src/session.rs` — `pub async fn session_connect(...) -> Result<u64, CoreError>` (follows convention)
- `docs/principles.md` §6 — "Errors are values, not afterthoughts. Every fallible operation surfaces its failure."

## Impact

- Platform adapters must special-case bind and discovery errors with string matching or wrapping, unlike all other core functions.
- TypeScript's `classifyBindError()` exists as a separate function specifically because bind errors aren't `CoreError` — this is a workaround for the type inconsistency.
- String-based errors from discovery make it impossible for TypeScript's `classifyByCode()` to apply the correct error class.
- Bind failures (invalid key, relay misconfiguration, socket bind failure) are among the most important errors to classify correctly for user-facing messages.

## Remediation

1. Change `bind()` to return `Result<Self, CoreError>`.
2. Change `start_browse()` and `start_advertise()` to return `Result<_, CoreError>` (either by adding `iroh-http-core` as a dependency, or by defining a `DiscoveryError` that maps cleanly to `CoreError` at the adapter layer).
3. Classify all error paths using appropriate error codes: `InvalidInput` for bad config, `ConnectionFailed` for socket/relay/mDNS failures.
4. Remove the separate `classify_bind_error()` helper in TypeScript and use the standard `classifyError` path.

## Acceptance criteria

1. `IrohEndpoint::bind()` returns `Result<Self, CoreError>`.
2. `start_browse()` and `start_advertise()` return a typed error, not `String`.
3. All bind and discovery error paths use typed error codes.
4. `classifyBindError` in TypeScript is collapsed into the standard `classifyError` path.

## Absorbed

- **A-ISS-047** (duplicate) — same pattern for discovery functions (`start_browse`, `start_advertise`).
