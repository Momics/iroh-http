---
id: "007"
title: "Cross-runtime HTTP compliance test strategy"
status: open
date: 2026-04-13
area: testing
tags: [testing, compliance, ffi, node, deno, tauri, ci]
---

# [007] Cross-runtime HTTP compliance test strategy

## Context

iroh-http aims to expose a consistent `fetch` / `serve` interface across three
runtimes: Node.js, Deno, and Tauri. The Rust core provides the
underlying implementation, but each FFI adapter translates the interface into
its own idioms. Without a shared compliance test suite, divergence between
adapters will creep in silently — a behaviour that works in Node may fail in
Deno and no test will catch it.

This is a tooling and CI architecture problem independent of the Rust code
itself.

## Questions

1. What is the minimal set of HTTP behaviours that all four adapters must
   agree on (a compliance baseline)?
2. Should the compliance tests live in a single test harness that is run
   against each adapter in turn, or in per-adapter test suites that
   converge on the same cases?
3. How are end-to-end tests structured — does each test start a real Iroh
   node pair, or is there a mock transport?
4. What does CI need to look like to gate all four adapters on every change to
   the Rust core?

## What we know

- A shared compliance harness now exists at `tests/http-compliance/`: a JSON
  corpus of 102 cases and TypeScript runners for Node and Deno, exercised in CI
  on every PR (`ci.yml` e2e job: `bash tests/http-compliance/run.sh`).
- Per-adapter compliance tests also exist: `packages/iroh-http-node/test/compliance.mjs`
  and `packages/iroh-http-deno/test/compliance.ts`, both now in CI.
- Seven shared test suites exist in `tests/suites/`: lifecycle, errors, stress,
  events, sessions, keys, and discovery. These are consumed by runners for
  Node (`tests/runners/node.mjs`), Deno (`tests/runners/deno.ts`), and Tauri
  (`tests/runners/tauri.ts`).
- The `extended-tests.yml` CI workflow runs the deep test suites for Node and
  Deno on push to main and on PRs.
- Iroh requires a real QUIC transport for meaningful integration tests; testing
  over a mock transport risks missing real connectivity bugs.
- **Remaining gap:** The Tauri runner exists and supports all 7 shared suites,
  but requires a running Tauri application context and is not automated in CI.
  It supports a `?ci=true` query param for headless execution but this is not
  yet wired into `run-all.sh` or any CI workflow.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Shared JSON test case corpus, per-adapter runners | Clear compliance surface; easy to add cases | Requires runner per platform |
| Rust integration tests only; adapter tests for bindings only | Familiar, low overhead | Does not catch FFI translation bugs |
| WPT-style test server; adapters hit it over real Iroh transport | High confidence; tests real stack | Complex CI setup |
| Contract tests at the FFI boundary (snapshot/golden output) | Catches regressions quickly | Doesn't test real transport behaviour |

## Implications

- A cross-runtime test gap means the guarantee "iroh-http behaves the same
  everywhere" is unverified for Tauri in CI.
- CI supports Node and Deno runtimes with good coverage. Tauri CI integration
  requires a running app context, which is harder to automate.

## Next steps

- [x] Enumerate the HTTP behaviours that must be consistent across runtimes
  (status codes, streaming, headers, errors, timeouts).
- [x] Design a minimal shared test fixture format that all adapters can
  consume.
- [x] Prototype a two-node Iroh integration test that runs the same scenario
  in Node and Deno.
- [ ] Automate the Tauri runner in CI (requires headless Tauri app context).
