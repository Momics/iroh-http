---
agent: 'agent'
model: Claude Opus 4.6 (copilot)
description: 'Does a full evaluation of the codebase'
---

You are performing a structured evaluation of the iroh-http library.

iroh-http is HTTP/1.1 over Iroh QUIC transport. Nodes are addressed by Ed25519 public key. The stack has two distinct layers with different trust levels:

- **External (trust as-is):** iroh, hyper, and tower. Do not evaluate these.
- **Owned layer (evaluate this):** the glue code that connects those external libraries — connection pooling, handle lifecycle, body channels, the serve and fetch entry points, session management, input validation at the FFI boundary, and error mapping.

Before forming conclusions, explore the codebase to understand where each of these responsibilities lives.

## Evaluation order

Work through these dimensions in priority order. Report findings as you go.

1. **Security** — iroh guarantees peer identity; the owned layer is responsible for everything that touches user-controlled input before it reaches iroh or hyper.

2. **Correctness** — verify that the owned glue behaves correctly under normal and edge conditions.

3. **Performance** — establish loopback baselines before introducing relay into the path; those are two different operating modes.

4. **Robustness** — error handling, graceful shutdown, and any heuristics over external library internals.

5. **FFI adapter consistency** — error code mapping, message escaping, and type shape parity across all adapters.

6. **Developer experience** — last, only after the above are clean.

## Output

Write findings to `temp/evaluations/<YYYY-MM-DD>/` using today's date. Use previous evaluations in that folder as a reference for depth.

Each evaluation uses this structure:

```
README.md            # coordination plan
A-architecture.md    # workstream instructions (run first)
B-security.md
C-ffi.md
D-correctness.md
E-api-consistency.md
F-operations.md
synthesis.md         # aggregation instructions (run last)
findings/
  A-architecture.md  # output per workstream
  …
  synthesis.md
```

B–F can run in parallel once A is done. Each `findings/` file opens with a one-paragraph summary, then detailed findings with file paths and line numbers as evidence. Flag issues **P0** (blocking) → **P3** (cosmetic).
