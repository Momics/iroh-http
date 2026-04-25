---
id: "012"
title: "Benchmark infrastructure"
status: open
date: 2026-04-25
area: testing
tags: [benchmarks, performance, deno, node, rust, criterion, ci]
---

# [012] Benchmark infrastructure

## Context

Performance benchmarks exist for all three layers (Rust/Criterion,
Node/Mitata, Deno/`Deno.bench`) and a CI workflow (`bench.yml`) is set up to
run them and post results to GitHub Pages. However, the workflow is currently
disabled — it hangs during Deno bench runs. The benchmarks are also the tool
that originally surfaced the concurrency bugs in #119.

Benchmarks serve two purposes: regression detection (automated, CI) and
characterisation (manual, exploratory). Both are currently broken. Without
them, performance regressions ship silently and concurrency bugs hide until
they hit production.

## Questions

1. Can the Deno benchmarks run reliably once the FFI bridge issues from
   [009](009-ffi-bridge-reliability.md) are resolved, or do they need their
   own architectural changes (e.g. isolated processes, lower concurrency)?
2. Should bench CI run on every PR (expensive), only on tags (delayed signal),
   or on a nightly schedule (compromise)?
3. What is the right baseline: fixed hardware (self-hosted runner), or
   normalised relative numbers (ratio to a known-good commit)?
4. Should the three bench layers (Rust, Node, Deno) be independent or
   unified into a single report?

## What we know

### Current benchmark inventory

| Layer | Tool | Suites | Status |
|-------|------|--------|--------|
| Rust | Criterion | `throughput`, `latency` | Working locally; smoke-tested in CI |
| Node | Mitata | `throughput.mjs`, `latency.mjs` | Working locally |
| Deno | `Deno.bench` | `throughput.bench.ts`, `latency.bench.ts` | Hangs under load |

### CI workflow (`bench.yml`)

- Trigger: `workflow_dispatch` only (tag trigger disabled).
- Three parallel jobs: `bench-node`, `bench-deno`, `bench-rust`.
- Each normalises output and posts to `gh-pages` via `benchmark-action`.
- The Deno job hangs, which is why the workflow was disabled.

### The Deno hang

The benchmarks drive 32 concurrent fetch/serve cycles at high iteration rate.
This is exactly the load pattern that triggers the race conditions documented
in #119 and #122. The hang is not a benchmark bug — it's the adapter bug
manifesting under benchmark load.

### What the benchmarks measure

- **Throughput:** requests/second at various concurrency levels (1, 8, 32).
- **Latency:** p50/p95/p99 round-trip time for single requests.
- **Body sizes:** small (JSON), medium (64 KB), large (1 MB).
- A `report.ts` / `report.mjs` normaliser produces JSON compatible with
  `github/benchmark-action`.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Wait for #009, then re-enable bench.yml as-is | No new work; benches already written | Blocked on #009; no signal until then |
| Run Rust and Node benches now; add Deno later | Partial signal immediately | Incomplete picture; Deno is the problematic adapter |
| Lower Deno bench concurrency to avoid the race | Quick unblock | Doesn't test the interesting (high-concurrency) path |
| Run benches in isolated subprocess per iteration | Avoids accumulated state bugs | Slower; may not reproduce real-world patterns |
| Nightly schedule instead of per-PR | Manageable cost; catches regressions within 24h | Delayed signal; harder to bisect |

## Implications

- Directly blocked by [009 — FFI bridge reliability](009-ffi-bridge-reliability.md).
  The Deno benchmarks will not run cleanly until the adapter is stable.
- Interacts with [010 — CI stabilisation](010-ci-pipeline-stabilisation.md):
  re-enabling the bench workflow adds another CI job that must be reliable.
- Benchmark results are public (gh-pages). Unreliable results are worse than
  no results — they mislead consumers about performance characteristics.
- Self-hosted runner (`vars.BENCHMARK_RUNNER`) is already configured but not
  verified. If it's unavailable, benches fall back to `ubuntu-latest` which
  produces noisy numbers.

## Next steps

- [ ] Re-enable Rust and Node bench jobs immediately (they work; Deno can be
      added later). Update `bench.yml` to skip the Deno job with a clear
      comment linking to #009.
- [ ] Verify the self-hosted benchmark runner is operational. If not, decide
      whether to use `ubuntu-latest` with statistical smoothing or defer
      until the runner is available.
- [ ] Once #009 lands: re-enable Deno bench job and run a full characterisation
      pass (baseline numbers for v0.3.x).
- [ ] Decide on CI frequency: nightly schedule as default, with manual
      `workflow_dispatch` for ad-hoc runs.
- [ ] Add a benchmark regression threshold (e.g. >10% regression fails the
      job) to catch performance regressions automatically.
