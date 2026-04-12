# S3 Node Adapter Findings (Delegated)

Date: 2026-04-12

## Findings

1. `R9-S3-01` (`P1`): `disableNetworking` option ignored in Node `createNode` mapping.
2. `R9-S3-02` (`P1`): `reconnect/lifecycle` options accepted in types but dropped by adapter.
3. `R9-S3-03` (`P2`): `discovery.mdns` preconfiguration is not honored.
4. `R9-S3-04` (`P2`): non-fetch methods can leak unclassified native errors.
5. `R9-S3-05` (`P2`): internal handle-based FFI APIs are publicly importable.
6. `R9-S3-06` (`P2`): several Rust NAPI paths bypass structured `classify_error_json`.
7. `R9-S3-07` (`P3`): README options drift from actual NodeOptions shape.

## Parity Summary

1. Fully aligned: core body bridge primitives and fetch token cancellation path.
2. Partial: error parity outside fetch and discovery preconfiguration behavior.
3. Divergent: option contract implementation and public exposure of internal FFI APIs.

## Source

Delegated agent output stored in thread notifications on 2026-04-12.
