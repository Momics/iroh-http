---
id: "005"
title: "Versioning and compatibility across the FFI boundary"
status: accepted
date: 2026-04-13
resolved: 2026-04-25
area: ffi
tags: [versioning, compatibility, ffi, semver, node, deno, python, tauri]
---

# [005] Versioning and compatibility across the FFI boundary

## Context

iroh-http has one Rust core and three FFI adapters (Node.js via napi-rs,
Deno via FFI/dlopen, Tauri via a plugin). Each adapter is distributed through
its own package registry (npm, JSR, crates.io).

> **Resolved.** The adapter bundling model eliminates version skew by design.
> Each JS package always ships with the matching native binary. See
> [Decisions](#decisions).

A user of the Node adapter might be running v1.2 of the binding against v1.0
of the core. A Tauri app might bundle the core at build time and never update
it while the Node adapter on the same machine is updated independently. These
version combinations could theoretically happen.

## Questions

1. Does the FFI surface need its own versioning contract, separate from
   semver on the public Rust API?
2. Should the Rust core expose a version or capability query so that bindings
   can detect mismatches at runtime and fail fast?
3. What is the minimum supported version skew — how many major versions of a
   binding should the core remain compatible with simultaneously?
4. How should breaking changes to the FFI surface be signalled and managed?

## What we know

- Each adapter ships a native binary built against a specific version of the
  core. Version skew between the binding package and the compiled native
  binary is a known failure mode in FFI ecosystems generally.
- **Node.js:** The npm package uses `optionalDependencies` to pull in
  platform-specific binary packages (`@momics/iroh-http-node-{platform}-{arch}`).
  The JS code and native binary are co-versioned and published together.
  No version skew is possible.
- **Deno:** The Deno adapter downloads the FFI binary from GitHub releases at
  runtime, matched to the exact package version. No version skew is possible.
- **Tauri:** The plugin crate is a direct Cargo dependency of the user's app.
  The Rust compiler resolves versions at build time. No runtime skew is
  possible.
- Capabilities-based negotiation (where the core advertises what it supports
  and bindings check before calling) is an established pattern but is
  unnecessary when the binary and JS layers are always co-versioned.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Strict version pinning; adapters only work with exact core version | No ambiguity | Painful for users; forces lockstep upgrades |
| Semantic versioning on the FFI surface with documented stability tiers | Clear contract | Requires disciplined classification of every FFI function |
| Runtime capability query (core exports a capability bitfield or version tuple) | Fail-fast; debuggable | Adds protocol overhead; must be maintained |
| Do nothing; rely on native binary versioning in package managers | Zero extra work | Silent breakage when skew occurs |

## Decisions

**Q1 — Separate FFI versioning contract?** Not needed. The adapter bundling
model ensures the JS/TS layer and native binary are always the same version.
There is no independent versioning of the FFI surface.

**Q2 — Runtime capability query?** Not needed. Since the binary and JS layers
are co-versioned, there is no mismatch to detect. ALPN protocol identifiers
(`iroh-http/2`) handle wire-level version negotiation between peers.

**Q3 — Minimum supported version skew?** Zero. Each release ships a complete,
self-contained package. There is no supported cross-version combination.

**Q4 — Breaking FFI changes?** Handled by semver on the JS package. Since the
FFI surface is internal (not user-facing), breaking changes to it are
transparent to users as long as the JS API contract is maintained.

## Implications

- The bundling model is the versioning strategy. No additional mechanism is
  needed.
- Future adapters (e.g. Python) should follow the same co-versioning pattern.
- Interacts with [009 — FFI bridge reliability](009-ffi-bridge-reliability.md):
  changes to the FFI surface are internal refactors, not breaking API changes.

## Next steps

- [x] Audit how napi-rs projects handle version compatibility — answered:
  co-versioned `optionalDependencies`.
- [x] Draft a versioning policy — answered: co-versioning eliminates the need.
- [x] Decide whether a runtime capability query is needed — no.
