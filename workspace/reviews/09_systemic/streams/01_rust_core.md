# S1: Rust Core Review Brief

## Scope

- `crates/iroh-http-core`
- `crates/iroh-http-framing`
- `crates/iroh-http-discovery`

## Objective

Validate end-to-end behavior, safety defaults, layering boundaries, and guideline
compliance for Rust core code.

## Must-Check Areas

1. Guideline conformance against `docs/guidelines.md` and `docs/guidelines-rust.md`
2. Security defaults and resource bounds
3. Handle lifecycle and slab cleanup correctness
4. Connection pooling and cancellation behavior
5. Framing/QPACK correctness and error handling
6. Test coverage for hostile and edge inputs
7. Doc quality for all `pub` items

## Baseline Commands

```bash
cargo check --workspace
cargo test -p iroh-http-core
cargo test -p iroh-http-framing
cargo test -p iroh-http-discovery
cargo clippy -p iroh-http-core -- -D warnings
```

## Deliverables

- Findings using `../templates/finding.md`
- A short pass/fail checklist for the seven must-check areas
- Suggested remediation order for P0/P1 items

## Exit Criteria

- All high-severity issues are evidenced with file/line references
- No unresolved assumptions on protocol behavior
- Explicit statement on whether security defaults are safe by default
