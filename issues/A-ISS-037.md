---
id: "A-ISS-037"
title: "extract_path drops query for authority-only URLs"
status: open
priority: P2
date: 2026-04-13
area: core
package: "iroh-http-core"
tags: [core, url, parsing, client]
---

# [A-ISS-037] extract_path drops query for authority-only URLs

## Summary

`extract_path` returns `/` when a URL has authority but no slash path segment, which drops query parameters for inputs like `httpi://node?x=1`.

## Evidence

- `crates/iroh-http-core/src/client.rs:417` — path extraction is implemented using string splitting.
- `crates/iroh-http-core/src/client.rs:420` — only `/` is searched after scheme.
- `crates/iroh-http-core/src/client.rs:423` — when no slash exists, function returns `/`, losing any `?query`.

## Impact

Client requests can be sent with an incorrect request target, resulting in routing/matching bugs and incorrect behavior for query-driven handlers.

## Remediation

1. Parse URL authority/path/query explicitly (or with a safe parser) in `extract_path`.
2. Preserve query-only URLs as `/?...`.
3. Add regression tests for:
   - `httpi://node?x=1` -> `/?x=1`
   - `httpi://node/path?x=1` -> `/path?x=1`
   - bare `/path?x=1` passthrough.

## Acceptance criteria

1. Query-only URLs preserve query parameters in the outgoing request target.
2. Existing path/query tests remain green.
3. New regression tests cover authority-only query forms.

