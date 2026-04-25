# Scripts

## Daily development

```sh
npm run ci
```

Runs: `cargo fmt --check` → `cargo clippy` → `cargo test` → feature checks →
TypeScript typecheck → builds → Node/Deno/interop tests.

Mirrors the GitHub CI `verify` job exactly. Run this before pushing to `main`.

---

## Releasing

```sh
npm run release
```

Or pass the version directly to skip the prompt:

```sh
npm run release -- 0.4.0
```

The script is interactive and walks you through each step:

1. Shows unreleased commits since the last tag
2. Runs `npm run ci` — **exits immediately if any check fails**
3. Bumps all manifests (`Cargo.toml`, `package.json`, `deno.jsonc`, `adapter.ts`)
4. Shows the diff and asks you to confirm
5. Commits `chore: release vX.Y.Z` and creates the git tag
6. Asks whether to push

Pushing the tag triggers two GitHub Actions workflows:

| Workflow | What it does |
|----------|-------------|
| `build.yml` | Creates the GitHub release, builds native binaries across 5 platforms (macOS arm64/x86, Linux x64/arm64, Windows x64) |
| `publish.yml` | Publishes to npm, JSR, and crates.io — fires automatically after `build.yml` succeeds |

---

## Individual commands

```sh
# Bump all manifests without committing or tagging:
npm run version:bump -- 0.4.0

# Run CI checks only:
npm run ci

# Manually republish a package (if publish.yml needs a retry):
npm run publish:shared          # → npm
npm run publish:shared:jsr      # → JSR
npm run publish:node            # → npm (all platform packages)
npm run publish:deno            # → JSR
npm run publish:tauri           # → npm
```

---

## Prerequisites

| Tool | Purpose | Install |
|------|---------|---------|
| Rust (stable) | Core build | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Node.js 22+ | JS packages, tests | [nodejs.org](https://nodejs.org) |
| Deno | Deno package, tests | `curl -fsSL https://deno.land/install.sh \| sh` |
| cargo-deny | License / advisory checks | `cargo install cargo-deny --locked` |
| cargo-audit | Security advisories | `cargo install cargo-audit --locked` |

Cross-compilation (5 platforms) is handled entirely by GitHub Actions — no local cross-compile toolchain is needed for releasing.
