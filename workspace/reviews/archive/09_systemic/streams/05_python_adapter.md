# S5: Python Adapter Review Brief

## Scope

- `packages/iroh-http-py/src/lib.rs`
- `packages/iroh-http-py/iroh_http`
- `packages/iroh-http-py/tests`

## Objective

Validate Python binding behavior, async patterns, API design, and compliance
with `docs/guidelines-python.md`.

## Must-Check Areas

1. API ergonomics (`create_node`, request/response/session objects)
2. Async behavior and `asyncio` integration
3. Body consumption semantics and stream ownership
4. Error behavior at the PyO3 boundary
5. Type quality (`py.typed`, stubs, `__all__`, docs)
6. Test depth for two-node flows and hostile inputs

## Baseline Commands

```bash
cd packages/iroh-http-py && pytest -q
```

## Deliverables

- Findings using `../templates/finding.md`
- Python parity table against S2 conformance checklist

## Exit Criteria

- Any mismatch with JS contract behavior is documented
- Typed exception roadmap is either accepted risk or backlog item

## Latest Run

- 2026-04-12 delegated output: `../evidence/S5_python_2026-04-12.md`
