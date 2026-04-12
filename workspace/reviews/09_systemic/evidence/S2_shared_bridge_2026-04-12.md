# S2 Shared Bridge Contract Findings (Delegated)

Date: 2026-04-12

## Findings

1. `R9-S2-01` (`P1`): trailer completion invariant can break on trailer callback/send failure.
2. `R9-S2-02` (`P1`): `acceptWebTransport()` can be invoked multiple times for same handle pair.
3. `R9-S2-03` (`P1`): request upload pipe failures are logged but not surfaced to caller.
4. `R9-S2-04` (`P2`): method-gated request-body reconstruction may drop valid bodies.
5. `R9-S2-05` (`P2`): stream cancel path does not await/handle async cancel result.
6. `R9-S2-06` (`P1`): shared error class/name mapping diverges from documented contract.
7. `R9-S2-07` (`P2`): legacy regex fallback can misclassify invalid-handle as stream error.
8. `R9-S2-08` (`P2`): public API throws plain `Error` for unsupported features.

## Cross-Adapter Conformance Checklist (from stream)

1. Abort-before-call rejects immediately with `AbortError`.
2. Mid-flight abort cancels fetch token and body reader exactly once.
3. One readable stream per body handle; duplicate creation fails deterministically.
4. `ReadableStream.cancel()` is observable and does not create unhandled rejections.
5. Request upload failures propagate to caller.
6. Trailers and serve completion semantics are deterministic and single-resolution.
7. Error classification parity is consistent across all adapters.

## Source

Delegated agent output stored in thread notifications on 2026-04-12.
