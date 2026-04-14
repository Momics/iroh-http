---
id: "DENO-005"
title: "Fetch cancellation tokens are allocated with the wrong endpoint identifier"
status: fixed
priority: P1
date: 2026-04-13
area: deno
package: iroh-http-deno
tags: [deno, cancellation, fetch, handle-lifecycle]
---

# [DENO-005] Fetch cancellation tokens are allocated with the wrong endpoint identifier

## Summary

`allocFetchToken` in the Deno dispatcher uses the adapter's external endpoint slab handle as the endpoint id, and defaults to `0` when the field is missing. Core token ownership and cleanup are keyed by the internal `endpoint_idx`, not the external handle.

## Evidence

- `packages/iroh-http-deno/src/dispatch.rs:343` — `endpointHandle` is read and passed directly to `iroh_http_core::alloc_fetch_token`.
- `packages/iroh-http-deno/src/dispatch.rs:343` — missing `endpointHandle` silently becomes `0` via `unwrap_or(0)`.
- `crates/iroh-http-core/src/endpoint.rs:267` — internal `endpoint_idx` is allocated separately from adapter slab handles.
- `crates/iroh-http-core/src/stream.rs:217` — cancellation token entries are keyed to a core endpoint index (`ep_idx`).
- `crates/iroh-http-core/src/stream.rs:300` — token cleanup during endpoint shutdown filters by `ep_idx`.

## Impact

Cancellation token ownership can be mis-scoped to the wrong endpoint id. This risks tokens not being cleaned up when an endpoint closes and can cause incorrect cancellation behavior in multi-endpoint scenarios.

## Remediation

1. Resolve `endpointHandle` to a real endpoint instance first and read its internal core `endpoint_idx`.
2. Pass that `endpoint_idx` to `alloc_fetch_token`.
3. Return a validation error if `endpointHandle` is missing or invalid instead of defaulting to `0`.

## Acceptance criteria

1. `allocFetchToken` rejects missing/invalid endpoint handles.
2. Tokens allocated for an endpoint are removed when that endpoint is closed.
3. Multi-endpoint tests confirm cancellation only affects fetches on the owning endpoint.
