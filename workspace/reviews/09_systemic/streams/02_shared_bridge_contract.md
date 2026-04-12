# S2: Shared Bridge Contract Review Brief

## Scope

- `packages/iroh-http-shared/src`

## Objective

Treat shared bridge code as the behavioral contract and verify internal
consistency, API design quality, and conformance expectations for adapters.

## Must-Check Areas

1. `Bridge` interface completeness and minimality
2. Fetch/serve/session semantics consistency
3. Abort, cancellation, and handle cleanup invariants
4. Trailer and streaming invariants
5. Error-classification mapping behavior
6. API ergonomics against `docs/guidelines-javascript.md`

## Baseline Commands

```bash
npm run typecheck --workspace=packages/iroh-http-shared
```

## Deliverables

- Contract-level findings (template-based)
- A conformance checklist that S3/S4/S5 can execute
- List of required cross-adapter parity tests

## Exit Criteria

- Bridge invariants are explicitly documented and testable
- Any contract ambiguity has a proposed single-source definition
