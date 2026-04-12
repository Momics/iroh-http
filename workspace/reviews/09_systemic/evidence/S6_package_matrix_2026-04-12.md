# S6 Trusted Package Matrix Snapshot (Delegated)

Date: 2026-04-12

## Priority `adopt_now` recommendations

1. Replace JS Ed25519 key/sign/verify implementation with `@noble/ed25519`.
2. Replace custom `BodyAsyncRead` adapter with `tokio-util::io::StreamReader` + `tokio-stream`.
3. Move Rust error classification to typed errors (`thiserror` + explicit `ErrorCode` enum).
4. Use `httparse` for trailer header-line parsing in framing.

## Decision Summary

| Subsystem | Decision | Notes |
|---|---|---|
| JS crypto primitives | `adopt_now` | Security/correctness and consistency gain |
| Compression stream adapter | `adopt_now` | Lower complexity, removes panic edge |
| Rust error classification | `adopt_now` | Stabilizes cross-platform error contract |
| Trailer/header parsing | `adopt_now` | Parser hardening |
| Connection pool | `keep_custom_justified` | Protocol-specific `(node, ALPN)` semantics |
| Global handle registry | `adopt_later` | Large migration blast radius |
| QPACK state management | `adopt_later` | Defer until measurable perf need |
| Deno bridge wire format | `adopt_later` | MessagePack path may improve throughput |
| Python object marshalling | `keep_custom_justified` | Better immediate win is removing unsafe path directly |

## Source

Delegated agent output stored in thread notifications on 2026-04-12.
