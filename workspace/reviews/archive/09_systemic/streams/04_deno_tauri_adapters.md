# S4: Deno and Tauri Adapter Review Brief

## Scope

- `packages/iroh-http-deno`
- `packages/iroh-http-tauri`

## Objective

Validate Deno and Tauri adapter behavior against shared bridge contract and
confirm platform-specific details do not violate contract semantics.

## Must-Check Areas

1. Deno FFI dispatch correctness and memory-safety assumptions
2. Tauri invoke/channel request lifecycle correctness
3. Cancellation, trailers, streaming parity with shared contract
4. Error propagation and classification parity with Node
5. Mobile lifecycle behavior in Tauri guest JS
6. Feature-flag parity with Rust core (`discovery`, `compression`)

## Baseline Commands

```bash
deno test packages/iroh-http-deno/test/smoke.test.ts
npm run typecheck --workspace=packages/iroh-http-tauri
cd packages/iroh-http-tauri && cargo check
```

## Deliverables

- Findings using `../templates/finding.md`
- Deno/Tauri parity report against S2 checklist

## Exit Criteria

- Cross-platform contract mismatches are clearly evidenced
- Any FFI safety assumptions are either proven or flagged as risk

## Latest Run

- 2026-04-12 delegated output: `../evidence/S4_deno_tauri_2026-04-12.md`
