---
id: "B-ISS-043"
title: "max_header_bytes naming inconsistency across ServeOptions, NodeOptions, and Tauri args"
status: open
priority: P2
date: 2026-04-14
area: core
package: iroh-http-core
tags: [naming, correctness, tauri, api]
---

# [B-ISS-043] max_header_bytes naming inconsistency across ServeOptions, NodeOptions, and Tauri args

## Summary

The maximum HTTP/1.1 request head size limit is named three different things in three different places: `ServeOptions::max_header_bytes` (referenced in architecture docs), `NodeOptions::max_header_size` (the actual field name), and `max_header_bytes` in `CreateEndpointArgs` for Tauri. The architecture docs security table references a field that does not exist.

## Evidence

- `docs/architecture.md` — security defaults table lists `Max request head size → ServeOptions::max_header_bytes`
- `crates/iroh-http-core/src/server.rs` — `ServeOptions` has no `max_header_bytes` field
- `crates/iroh-http-core/src/endpoint.rs` — `NodeOptions` uses `max_header_size: Option<usize>`
- `packages/iroh-http-tauri/src/commands.rs` — `CreateEndpointArgs` uses `max_header_bytes: Option<usize>`, a third name

## Impact

Contributors following the architecture doc cannot find `ServeOptions::max_header_bytes`. Tauri callers setting `maxHeaderBytes` are mapped to `NodeOptions::max_header_size` via a translation layer; any mismatch in that mapping is invisible. Search-based navigation of the codebase gives three different entry points for the same concept.

## Remediation

1. Pick one canonical name (e.g. `max_header_size`) and apply it consistently to `NodeOptions`, `ServeOptions` (if applicable), `CreateEndpointArgs`, and all platform adapters.
2. Update `docs/architecture.md` security table to reference the correct struct and field name.
3. Ensure the Tauri `camelCase` serialisation name (`maxHeaderSize`) is consistent with the chosen name.

## Acceptance criteria

1. The limit has one name throughout core, all adapters, and all docs.
2. `docs/architecture.md` security table references a field that actually exists with that exact name.
