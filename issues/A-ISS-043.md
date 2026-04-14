---
id: "A-ISS-043"
title: "Stale QPACK references in endpoint.rs doc comments and test comments"
status: open
priority: P2
date: 2026-04-14
area: core
package: "iroh-http-core"
tags: [docs, correctness, stale-reference]
---

# [A-ISS-043] Stale QPACK references in endpoint.rs doc comments and test comments

## Summary

Three locations in `iroh-http-core` reference "QPACK-encoded head" in doc comments. QPACK was removed during the hyper migration (design-decisions.md §1 explicitly notes it was replaced). The wire format is now standard HTTP/1.1 over QUIC — no QPACK involved. These stale references are confusing and technically incorrect.

## Evidence

- `crates/iroh-http-core/src/endpoint.rs:125` — `/// Maximum byte size of a QPACK-encoded head (request or response).` (private field doc)
- `crates/iroh-http-core/src/endpoint.rs:396` — `/// Maximum byte size of a QPACK-encoded head.` (public method doc)
- `crates/iroh-http-core/tests/integration.rs:1097` — `// Build headers that exceed 256 bytes when QPACK-encoded.`

The correct description already exists at `endpoint.rs:74`: `/// Maximum byte size of the HTTP/1.1 request or response head (status line + headers).`

## Impact

Misleads contributors and protocol reviewers into thinking QPACK is part of the wire format. Violates Principle 6 ("deviating from a specification is a correctness failure") — the doc claims a wire format detail that is not true.

## Remediation

1. Change endpoint.rs:125 to: `/// Maximum byte size of the HTTP/1.1 request or response head (status line + headers).`
2. Change endpoint.rs:396 to: `/// Maximum byte size of the HTTP/1.1 request or response head.`
3. Change integration.rs:1097 to: `// Build headers that exceed 256 bytes.`

## Acceptance criteria

1. `grep -r "QPACK" crates/` returns zero matches.
