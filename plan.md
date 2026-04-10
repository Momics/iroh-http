# iroh-http — Plan

## Status

All foundational patches (00–05) and reviews (00–01) are **integrated**.
The work below covers what remains before open-sourcing.

---

## Patch tracker

| # | Title | Status | Summary |
|---|-------|--------|---------|
| 00 | [Brief](docs/patches/00_brief.md) | integrated | Architecture, JS API surface, repo layout |
| 01 | [Protocol extensions](docs/patches/01_patch.md) | integrated | Iroh-HTTP framing, custom URL scheme |
| 02 | [Deno FFI](docs/patches/02_patch.md) | integrated | Deno native adapter via `Deno.dlopen` |
| 03 | [Python bindings](docs/patches/03_patch.md) | integrated | `iroh-http-py` via maturin/PyO3 |
| 04 | [WebTransport alignment](docs/patches/04_patch.md) | integrated | WHATWG-familiar naming (`BidirectionalStream`, etc.) |
| 05 | [Keys + errors](docs/patches/05_patch.md) | integrated | `PublicKey`/`SecretKey` classes, `IrohError` hierarchy |
| 06 | [Discovery](docs/patches/06_patch.md) | **pending** | mDNS compiled-in behind feature flag + Tauri mobile native |
| 07 | [Stream hardening](docs/patches/07_patch.md) | **pending** | Drain timeouts, body TTL sweep, slab leak prevention |
| 08 | [Structured Rust errors](docs/patches/08_patch.md) | **pending** | Replace `Result<T, String>` with `{code, message}` JSON |
| 09 | [Mobile lifecycle](docs/patches/09_patch.md) | **pending** | Tauri visibilitychange, health probe, auto-resurrection |
| 10 | [DX + packaging](docs/patches/10_patch.md) | **pending** | Folder cleanup, `@momics` scope, registry publishing |
| 11 | [Open-source readiness](docs/patches/11_patch.md) | **pending** | LICENSE, READMEs, CI, examples, checklist |

## Review tracker

| # | Title | Status | Summary |
|---|-------|--------|---------|
| 00 | [Base review](docs/reviews/00_review.md) | integrated | 12 findings across all source files |
| 01 | [Patch 01 review](docs/reviews/01_review.md) | integrated | 6 findings (P0 framing parse bug, etc.) |
| 02 | [Reference patterns](docs/reviews/02_review.md) | written | Gap analysis vs old implementations |

---

## Architecture decisions

### Naming scope: `@momics`

`@iroh` is reserved. All JS packages publish under `@momics`:

| Package | Registry | Published name |
|---------|----------|---------------|
| Shared TS layer | npm + JSR | `@momics/iroh-http-shared` |
| Node adapter | npm | `@momics/iroh-http-node` |
| Tauri JS | npm | `@momics/iroh-http-tauri` |
| Deno adapter | JSR | `@momics/iroh-http-deno` |
| Tauri Rust | crates.io | `iroh-http-tauri` |
| Core Rust | crates.io | `iroh-http-core` |
| Framing Rust | crates.io | `iroh-http-framing` |
| Discovery Rust | crates.io | `iroh-http-discovery` |
| Python | PyPI | `iroh-http` |

### Discovery: compiled-in, dormant by default

Discovery (mDNS) ships inside each platform binary behind a Cargo feature
flag (`discovery`, enabled by default). Users activate it via config:

```ts
const node = await createNode({
  discovery: { mdns: true, serviceName: "my-app" }
})
```

If the feature was compiled out (e.g. custom build), requesting it produces a
clear error explaining exactly what happened and how to fix it.

No separate JS discovery packages. Rust-only users get
`iroh-http-discovery` on crates.io as a standalone crate.

Tauri mobile (iOS/Android) uses native service discovery via Swift/Kotlin
plugins, wired through `PluginHandle::run_mobile_plugin()`.

### Folder conventions: follow platform norms

- **Node** (`packages/iroh-http-node/`): `index.ts` at root — napi-rs convention
- **Tauri** (`packages/iroh-http-tauri/`): `guest-js/` — Tauri plugin convention
- **Deno** (`packages/iroh-http-deno/`): `mod.ts` at root, `src/` for internals
- **Shared** (`packages/iroh-http-shared/`): `src/` — standard TS library layout

Don't force them to match. Each follows its ecosystem's convention.

### One import per platform

Users install one package and get `createNode` → `fetch`/`serve`/`connect`:

```ts
// Node
import { createNode } from "@momics/iroh-http-node"

// Deno
import { createNode } from "@momics/iroh-http-deno"

// Tauri
import { createNode } from "@momics/iroh-http-tauri"
```

The shared layer is a transitive dependency — never imported directly.

---

## Open-source checklist

- [ ] LICENSE file (decide: MIT / Apache-2.0 / dual)
- [ ] Root README.md — architecture overview, platform comparison, quick start
- [ ] Per-package README.md — install + 3-line hello world
- [ ] CONTRIBUTING.md
- [ ] GitHub Actions CI (build + typecheck + test, all platforms)
- [ ] `.gitignore` covers `target/`, `dist/`, `node_modules/`, `*.node`
- [ ] No secrets/keys in git history
- [ ] Verify published names available (npm, JSR, crates.io, PyPI)
- [ ] `cargo test` passes at workspace level
- [ ] `npm run typecheck` passes for all TS packages
- [ ] Example apps: `examples/node/`, `examples/deno/`, `examples/tauri/`, `examples/python/`
- [ ] `cargo audit` clean
- [ ] Remove `.old_references/` from published repo (keep locally or archive)
- [ ] Version alignment: all packages at 0.1.0
- [ ] Changelog / RELEASES.md