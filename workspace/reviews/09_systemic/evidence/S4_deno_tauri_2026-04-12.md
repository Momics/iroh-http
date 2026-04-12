# S4 Deno/Tauri Adapter Findings (Delegated)

Date: 2026-04-12

## Findings

1. `R9-S4-01` (`P1`): Deno `nextChunk` uses shared mutable buffer across concurrent nonblocking FFI calls.
2. `R9-S4-02` (`P1`): Deno stream read errors are collapsed into EOF (`null`).
3. `R9-S4-03` (`P1`): Deno `stopServe` does not cleanly terminate polling loop.
4. `R9-S4-04` (`P1`): Tauri bridge methods surface raw invoke errors instead of typed classified errors.
5. `R9-S4-05` (`P1`): Tauri serve channel send failure logs only and can leave request unresolved.
6. `R9-S4-06` (`P2`): Tauri reconnect option closes node but does not reconnect.
7. `R9-S4-07` (`P2`): Tauri lifecycle health check validates handle existence, not network health.
8. `R9-S4-08` (`P2`): lifecycle listener cleanup callback is dropped (event-listener leak).
9. `R9-S4-09` (`P2`): Tauri ignores pooling options supplied by guest JS.

## Source

Delegated agent output stored in thread notifications on 2026-04-12.
