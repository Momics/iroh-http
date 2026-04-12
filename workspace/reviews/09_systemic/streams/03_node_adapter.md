# S3: Node Adapter Review Brief

## Scope

- `packages/iroh-http-node/src/lib.rs`
- `packages/iroh-http-node/lib.ts`
- `packages/iroh-http-node/test`

## Objective

Validate Node bridge implementation against shared contract and JS platform
guidelines.

## Must-Check Areas

1. Bridge parity with `iroh-http-shared`
2. FFI boundary error mapping and typed error behavior
3. Request/response streaming behavior and backpressure
4. AbortSignal and cancellation propagation
5. Discovery/session function parity
6. Public API surface minimality and docs quality

## Baseline Commands

```bash
npm run typecheck --workspace=packages/iroh-http-node
npm run test --workspace=packages/iroh-http-node
```

## Deliverables

- Findings using `../templates/finding.md`
- Node-specific parity table against S2 conformance checklist

## Exit Criteria

- Any contract mismatches are documented with evidence
- Behavioral drift from Deno/Tauri is flagged with concrete examples

## Latest Run

- 2026-04-12 delegated output: `../evidence/S3_node_adapter_2026-04-12.md`
