---
id: "A-ISS-034"
title: "Zero max_chunk_size can hang send_chunk"
status: open
priority: P1
date: 2026-04-13
area: core
package: "iroh-http-core"
tags: [core, streaming, validation, backpressure]
---

# [A-ISS-034] Zero max_chunk_size can hang send_chunk

## Summary

`max_chunk_size_bytes` is accepted without validation and can be configured as `0`. This causes `send_chunk` to enter a non-progressing split loop for non-empty payloads.

## Evidence

- `crates/iroh-http-core/src/endpoint.rs:250` — bind forwards `opts.max_chunk_size_bytes` directly into global backpressure config.
- `crates/iroh-http-core/src/stream.rs:430` — `max` is loaded from global config.
- `crates/iroh-http-core/src/stream.rs:439` — split loop computes `end = (offset + max).min(chunk.len())`; when `max == 0`, `end == offset` forever.

## Impact

At runtime, sending payloads larger than zero bytes can hang indefinitely, tying up async tasks and potentially stalling request/response pipelines under malformed config.

## Remediation

1. Validate `max_chunk_size_bytes` at bind time and reject `0` as invalid input.
2. Optionally treat `0` as "use default" to align with other tunables.
3. Add a defensive guard in `send_chunk` to fail fast if `max == 0`.

## Acceptance criteria

1. Binding with `max_chunk_size_bytes: Some(0)` returns an error (or normalizes to default by spec).
2. A regression test proves `send_chunk` always makes progress and does not loop forever.
3. Existing streaming tests continue to pass.

