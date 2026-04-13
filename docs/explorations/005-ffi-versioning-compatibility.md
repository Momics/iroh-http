---
id: "005"
title: "Versioning and compatibility across the FFI boundary"
status: open
date: 2026-04-13
area: ffi
tags: [versioning, compatibility, ffi, semver, node, deno, python, tauri]
---

# [005] Versioning and compatibility across the FFI boundary

## Context

iroh-http has one Rust core and four FFI adapters (Node.js via Napi,
Deno via Napi or WebAssembly, Tauri via a plugin, Python via PyO3). Each
adapter is versioned independently and distributed through its own package
registry (npm, JSR, PyPI, crates.io).

A user of the Node adapter might be running v1.2 of the binding against v1.0
of the core. A Tauri app might bundle the core at build time and never update
it while the Node adapter on the same machine is updated independently. These
version combinations will happen in the wild and may be completely invisible
to the user.

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
  binary is already a known failure mode in the Napi and PyO3 ecosystems.
- The Tauri use case is particularly sensitive: the core may be embedded in an
  app binary that users don't update, while the plugin interface evolves.
- Capabilities-based negotiation (where the core advertises what it supports
  and bindings check before calling) is an established pattern in language
  runtimes and gRPC.

## Options considered

| Option | Upside | Downside |
|--------|--------|----------|
| Strict version pinning; adapters only work with exact core version | No ambiguity | Painful for users; forces lockstep upgrades |
| Semantic versioning on the FFI surface with documented stability tiers | Clear contract | Requires disciplined classification of every FFI function |
| Runtime capability query (core exports a capability bitfield or version tuple) | Fail-fast; debuggable | Adds protocol overhead; must be maintained |
| Do nothing; rely on native binary versioning in package managers | Zero extra work | Silent breakage when skew occurs |

## Implications

- Affects how all four adapters are packaged and distributed.
- Long-lived Tauri apps are the highest-risk scenario.
- Interacts with the roadmap's v1.0 stability commitment.
- Any solution must be implementable without changing the QUIC wire format.

## Next steps

- [ ] Audit how Napi-rs and PyO3 projects handle version compatibility
  in practice.
- [ ] Draft a versioning policy for the FFI surface and socialize it.
- [ ] Decide whether a runtime capability query is needed before v1.0.
