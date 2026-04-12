# Trusted Package Decision Matrix

Use this table to decide whether to keep custom logic or adopt a trusted package.

| Subsystem | Current Custom Implementation | Candidate Package(s) | Fit (High/Med/Low) | Migration Complexity (High/Med/Low) | Security/Perf Considerations | Decision (`adopt_now`, `adopt_later`, `keep_custom_justified`) | Rationale | Required Validation |
|---|---|---|---|---|---|---|---|---|
| Example: error typing | string matching in FFI layers | thiserror + typed mapping | Med | Med | Better stability, migration touches adapters | adopt_later | Improves typed handling, non-trivial API impact | adapter parity tests |

## Decision Notes

- Explain non-obvious tradeoffs.
- Record blockers for `adopt_now` items.
