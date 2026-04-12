# S6: Trusted Package Replacement Matrix Brief

## Scope

Cross-cutting across:

- Rust core/framing/discovery
- Shared JS layer
- Node/Deno/Tauri adapters
- Python bindings

## Objective

Identify where custom logic should be replaced by trusted, maintained packages,
and where custom logic is justified by protocol or FFI constraints.

## Decision Rule

Custom code is acceptable only when at least one is true:

1. Protocol semantics are unique and not supported by mature packages
2. FFI/ABI constraints require precise ownership or memory behavior
3. Existing packages create unacceptable security/performance/maintenance risk

If none apply, prefer package adoption.

## Deliverables

- Completed matrix using `../templates/package_decision_matrix.md`
- Recommendations labeled:
  - `adopt_now`
  - `adopt_later`
  - `keep_custom_justified`
- Migration risks and test requirements for each `adopt_*` row

## Inputs Required

- Findings from S1 to S5
- Existing dependency manifests and test constraints

## Exit Criteria

- Every major custom subsystem has an explicit decision
- At least one concrete adoption candidate per high-maintenance custom area
