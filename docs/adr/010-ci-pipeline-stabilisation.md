---
id: "010"
title: "CI pipeline stabilisation"
status: open
date: 2026-04-25
area: testing
tags: [ci, github-actions, reliability, workflow, release]
---

# [010] CI pipeline stabilisation

## Context

The v0.3.0 release was built and published correctly, but the main-branch CI
run that followed it reported errors. This is not the first time: CI
failures have occurred intermittently throughout development, eroding trust in
the green/red signal. When CI is unreliable, every contributor — human or
AI agent — loses the ability to distinguish "my change broke something" from
"CI is flaky again."

The current CI landscape is mature in scope (6 workflows covering lint, test,
build, publish, bench, fuzz) but fragile in practice.

## Questions

1. Which CI failures are flaky (non-deterministic, unrelated to code changes)
   and which are genuine regressions that were missed?
2. Should the main-branch CI and tag-triggered CI share a single workflow with
   different job matrices, or remain separate?
3. What is the acceptable CI wall-clock time, and are we within it?
4. Should the extended test suite (`extended-tests.yml`) be promoted to the
   main gate, or remain a supplementary signal?
5. How should CI handle the Deno adapter's known concurrency issues (skip,
   allow-failure, or block)?

## What we know

### Current workflow inventory

| Workflow | Trigger | Purpose | Reliability |
|----------|---------|---------|-------------|
| `ci.yml` | push main, PRs | Gate: lint, test, typecheck, audit, e2e | Intermittent failures |
| `build.yml` | version tags | Cross-platform native builds (5 targets × 2 runtimes) | Stable |
| `publish.yml` | after `build.yml` | Publish to npm, crates.io, JSR | Stable |
| `extended-tests.yml` | push main, PRs (path-filtered) | Deep Node/Deno integration tests | Deno suite unstable |
| `bench.yml` | manual only | Benchmarks (disabled — hangs) | Broken |
| `fuzz.yml` | nightly schedule | Fuzz + ASAN + Miri | Stable |

### Observed failure patterns

- **Post-release main CI failure (v0.3.0):** Exact cause not triaged. The
  release tag CI (`build.yml` + `publish.yml`) succeeded, but the merge
  commit on main triggered `ci.yml` which failed. Suggests either a timing
  dependency (artifacts not yet available) or a test that depends on the
  published package state.
- **Deno e2e flakiness:** The Deno adapter's concurrency issues (#119, #122)
  cause intermittent test failures in the e2e and extended-test jobs. These are
  real bugs, not flakes — but they present as flakes because they depend on
  scheduling timing.
- **Bench workflow disabled:** `bench.yml` hangs during Deno bench runs. The
  benchmarks were the original signal that found the concurrency issues (#119).
  Disabling the workflow was the right triage call but the underlying issue
  remains.

### What works well

- `scripts/check.sh` mirrors the CI verify job locally — contributors can
  pre-validate before pushing.
- The publish pipeline (`build.yml` → `publish.yml`) with OIDC trusted
  publishers is well-designed and has not failed.
- Nightly fuzzing (`fuzz.yml`) runs independently and has not produced false
  positives.
- The `ci.yml` concurrency setting (cancel prior runs) keeps costs down.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Triage every recent CI failure and fix root causes one by one | Precise; fixes real problems | Reactive; doesn't prevent new flakes |
| Add retry logic to flaky steps | Green signal faster | Masks real bugs; erodes trust further |
| Promote extended tests to main gate; drop separate workflow | Single source of truth for test results | Longer CI times on every PR |
| Mark Deno e2e as `continue-on-error` until #009 is resolved | Unblocks main CI; clear signal on what's known-broken | Deno regressions slip through unnoticed |
| Add a CI health dashboard (workflow success rate tracking) | Visibility into trends | Overhead to maintain |

## Implications

- CI trust directly affects agent productivity. The `fix-issues` skill runs
  `npm run ci` before committing. If that signal is unreliable, agents either
  skip it (dangerous) or stall on false negatives.
- Blocked by [009 — FFI bridge reliability](009-ffi-bridge-reliability.md) for
  the Deno-specific failures. CI will not be fully green until the Deno adapter
  is stable.
- Interacts with [012 — Benchmark infrastructure](012-benchmark-infrastructure.md):
  re-enabling `bench.yml` depends on the same Deno concurrency fix.

## Next steps

- [ ] Triage the v0.3.0 post-release CI failure: read the logs, identify root
      cause, file a fix or document as known.
- [ ] Audit the last 10 CI runs on main: classify each failure as
      flaky/real/infra.
- [ ] Decide on Deno e2e strategy: `continue-on-error` with a tracking label,
      or gate on Node-only until #009 lands.
- [ ] Verify that `scripts/check.sh` and `ci.yml` verify job are in sync — any
      drift means local pre-push gives a different signal than CI.
- [ ] Establish a policy: every CI failure on main gets a issue within 24h,
      even if it's a known flake. No silent reds.
