# Change 07 — `iroh-http-framing` removal/deprecation plan

## Risk: Medium — clean-up and ownership clarity

## Decision

For the host rework, framing is hyper's responsibility. We should not keep a
second unused runtime framing implementation in-tree as if it were active.

So this rework treats `iroh-http-framing` as deprecated runtime code.

## What this means

1. Host path: no references to `iroh-http-framing`.
2. Protocol source of truth: `workspace/rework_01/wire-format.md` + integration
   conformance tests in `iroh-http-core`.
3. Embedded future: if/when needed, create a dedicated embedded-targeted crate
   from the protocol docs and test vectors, not by carrying dead host code.

## Two acceptable execution options

### Option A (preferred): remove crate from workspace now

- Remove `crates/iroh-http-framing` from workspace members.
- Keep protocol and golden vectors in docs/tests.
- Re-introduce an embedded-focused crate later if needed.

### Option B: keep crate as explicitly deprecated archive

- Keep crate but mark as deprecated and unused by host path.
- Remove from critical CI path.
- Do not treat it as active protocol engine.

## Why

Keeping duplicate, unused framing code increases maintenance burden and creates
spec confusion. One active implementation (Hyper) plus protocol conformance
artifacts is clearer than two divergent code paths.

## Files changed

| File | Change |
|---|---|
| `Cargo.toml` (workspace) | Remove framing crate from members (Option A) or mark deprecated (Option B) |
| `iroh-http-core/*` | Ensure no framing imports remain |
| `workspace/rework_01/wire-format.md` | Be explicit about protocol source-of-truth docs/tests |

## Validation

```bash
cargo check --workspace
cargo test -p iroh-http-core
cargo test --test integration --features compression
```

## Exit criteria

- Exactly one active host framing implementation (Hyper).
- Protocol behavior captured by tests/docs, not by dead duplicate runtime code.
