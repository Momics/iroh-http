---
id: "B-ISS-046"
title: "architecture.md references removed classify_error_json function"
status: open
priority: P3
date: 2026-04-14
area: docs
package: iroh-http-core
tags: [docs, errors, correctness]
---

# [B-ISS-046] architecture.md references removed classify_error_json function

## Summary

The error model section of `docs/architecture.md` shows `classify_error_json()` as the function that converts `CoreError` to a JSON envelope for the FFI boundary. This function was removed. The actual functions are `core_error_to_json()` and `format_error_json()`. Any contributor following the architecture doc to understand or extend the error pipeline will look for a function that doesn't exist.

## Evidence

- `docs/architecture.md` — Error Model section: `classify_error_json()` shown in the pipeline diagram
- `crates/iroh-http-core/src/lib.rs:146` — comment: "Use this instead of the removed `classify_error_json`"
- `crates/iroh-http-core/src/lib.rs` — exported functions are `core_error_to_json` and `format_error_json`; no `classify_error_json` export exists

## Impact

Low — documentation only. But a contributor tracing the error path from the architecture doc will not find `classify_error_json` and will have to discover the rename independently.

## Remediation

1. Update the error model pipeline in `docs/architecture.md` to reference `core_error_to_json` (for `CoreError`) and `format_error_json` (for explicit-code errors).
2. Optionally, describe why both functions exist (typed enum path vs. ad-hoc string code path).

## Acceptance criteria

1. `docs/architecture.md` does not mention `classify_error_json`.
2. The pipeline diagram references functions that actually exist in `lib.rs`.
