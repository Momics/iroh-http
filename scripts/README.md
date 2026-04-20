# Building, Checking & Releasing

## Daily development

Before pushing to `main`, run the pre-push check script. It mirrors exactly what the CI `verify` job does, so there are no surprises:

```sh
scripts/check.sh
```

This runs: `cargo fmt --check` → `cargo clippy` → `cargo test` → feature checks → TypeScript typecheck.

## Releasing

Tag-based releases are handled by `scripts/release/run.sh`. Run it locally before tagging:

```sh
scripts/release/run.sh 0.2.0 --platform=node --dry-run   # validate
scripts/release/run.sh 0.2.0 --platform=node             # execute
```

Pushing the resulting tag triggers the GitHub Actions release workflow (multi-platform build → publish to crates.io, npm, JSR).

---

# Building & Versioning

## Prerequisites

| Tool | Required for | Install |
|------|-------------|---------|
| Rust toolchain | Everything | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Node.js 18+ | Node package, TS shared | [nodejs.org](https://nodejs.org) |
| Deno | Deno package | `curl -fsSL https://deno.land/install.sh \| sh` |

For cross-compilation (all platforms from macOS):

| Tool | Install |
|------|---------|
| `cargo-zigbuild` | `cargo install cargo-zigbuild` |
| `cargo-xwin` | `cargo install cargo-xwin` (Node Windows MSVC target) |
| `zig` | `brew install zig` |
| `mingw-w64` | `brew install mingw-w64` (Deno Windows GNU target) |
| LLVM | `brew install llvm lld` (required by `cargo-xwin` on macOS: `clang-cl`, `lld-link`, `llvm-lib`) |
| Rust targets | `rustup target add aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-msvc x86_64-pc-windows-gnu` |

## Building locally

Build everything for the current platform:

```sh
npm run build
```

Build everything for all platforms (cross-compile):

```sh
npm run build:all
```

Build individual packages:

```sh
npm run build:core      # cargo build --release --workspace
npm run build:shared    # tsc → packages/iroh-http-shared/dist/
npm run build:node      # napi build (host platform) + tsc
npm run build:node:all  # napi build (4 platforms) + tsc
npm run build:tauri     # cargo check + tsc → dist/
npm run build:deno      # cargo build (host platform)
npm run build:deno:all  # cargo zigbuild (5 platforms)
```

Each package owns its own build logic:
- **shared** — `packages/iroh-http-shared/package.json` → `tsc`
- **node** — `packages/iroh-http-node/package.json` → `napi build` + `tsc`, cross-compile via `scripts/build-all.mjs`
- **tauri** — `packages/iroh-http-tauri/package.json` → `cargo check` + `tsc`
- **deno** — `packages/iroh-http-deno/deno.jsonc` → `scripts/build-native.mts` / `scripts/build-all.mts`

## Bumping the version

All packages share the same version. One command updates all 12 manifest files:

```sh
./scripts/version.sh 0.2.0
```

This touches:
- 7 `Cargo.toml` (3 crates + 4 packages)
- 3 `package.json` (shared, node, tauri)
- 1 `deno.jsonc` (deno)
- 1 `jsr.jsonc` (shared on JSR)

It also updates inter-crate dependency versions and the Deno import map range.

After bumping:

```sh
git diff --stat                    # review changes
git add -u && git commit -m "chore: bump version to 0.2.0"
```

## Releasing

### Composed (one command)

```sh
# Full release for one platform:
npm run release:deno -- 0.2.0
npm run release:node -- 0.2.0

# Dry-run (no publish, no push, reverts version bump):
npm run release:deno -- 0.2.0 --dry-run

# Force rebuild even if binaries are up to date:
npm run release:node -- 0.2.0 --rebuild
```

LLVM must be on PATH for Node Windows (MSVC) cross-compile:
```sh
PATH="/opt/homebrew/opt/llvm/bin:$PATH" npm run release:node -- 0.2.0
```

### Individual steps

Each step is idempotent — it skips if already done, so re-running after a
failure picks up where it left off.

```sh
npm run release:preflight -- --scope=deno     # check tools + auth
npm run release:fmt                           # cargo fmt --all, auto-commit
npm run release:build -- --platform=deno      # build (skips if binaries fresh)
npm run release:test -- --platform=deno       # Rust + clippy + fmt + Deno smoke
npm run release:version -- 0.2.0              # bump all manifests (skips if current)
npm run release:upload:deno -- 0.2.0          # upload binaries to GitHub releases
npm run release:publish -- --platform=deno    # shared→npm/JSR, deno→JSR
npm run release:tag -- 0.2.0                  # git commit + tag (skips if exists)
```

> **Forks:** `scripts/release/upload-deno.sh` hard-codes `Momics/iroh-http-releases`
> as the target repository for Deno native binary uploads. If you fork this
> repo, update that value to match your own releases repository before running
> `release:upload:deno`.

### How it works

| Step | What it does | Guard |
|------|-------------|-------|
| `preflight` | check CLI tools, Rust targets, registry auth | none |
| `fmt` | `cargo fmt --all`, auto-commit if dirty | no-op if already clean |
| `build` | core → shared → platform binaries | skip if binaries newer than `*.rs` |
| `test` | cargo test + clippy + fmt check + typecheck + platform tests | none |
| `version` | bump all manifests via `version.sh` (incl. `adapter.ts` VERSION) | skip if already at target |
| `upload:deno` | `gh release create` + upload 5 binaries to `iroh-http-releases` | skip if all assets present |
| `publish` | shared→npm/JSR first, then platform package | skip if version already published |
| `tag` | git commit, `git tag vX.Y.Z` | skip if tag exists |

### Prerequisites

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin \
  x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
  x86_64-pc-windows-msvc x86_64-pc-windows-gnu
cargo install cargo-zigbuild cargo-xwin
brew install zig mingw-w64 llvm lld
npm adduser               # npm auth
gh auth login              # GitHub CLI (for Deno binary uploads)
```
