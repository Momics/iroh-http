---
id: "TAURI-015"
title: "Tauri create_endpoint ignores guest-js compressionLevel option"
status: fixed
priority: P2
date: 2026-04-13
area: tauri
package: "iroh-http-tauri"
tags: ["parity", "compression", "api"]
---

# [TAURI-015] Tauri create_endpoint ignores guest-js compressionLevel option

## Summary

The guest JS adapter sends `compressionLevel` for `create_endpoint`, but the Rust Tauri command args do not define or wire this field. As a result, the option is silently ignored on Tauri even though the public API accepts it.

## Evidence

- `packages/iroh-http-tauri/guest-js/index.ts:495-499` — `createNode` sends `compressionLevel` derived from `options.compression`.
- `packages/iroh-http-tauri/src/commands.rs:23-50` — `CreateEndpointArgs` has `compression_min_body_bytes` but no `compression_level`.
- `packages/iroh-http-tauri/src/commands.rs:104-110` — compression config only sets `min_body_bytes`; no level passthrough.
- `packages/iroh-http-node/src/lib.rs:93-94` and `:166-175` — Node adapter supports both `compression_level` and `compression_min_body_bytes`.

## Impact

Users configuring `compression.level` on Tauri get behavior that differs from other adapters, with no warning. This is a parity and observability issue that can cause performance tuning to fail silently.

## Remediation

1. Add `compression_level` to Tauri `CreateEndpointArgs`.
2. Wire it into `iroh_http_core::CompressionOptions.level`.
3. Decide and document validation behavior (range checks vs. delegate to core defaults/errors).
4. Add adapter-level tests that verify both `compressionLevel` and `compressionMinBodyBytes` are honored.

## Acceptance criteria

1. Passing `compression: { level: X }` from JS sets compression level X in Tauri endpoint config.
2. Passing `compression: { minBodyBytes: Y }` still works as before.
3. Tauri behavior matches Node/Deno for compression option mapping.
