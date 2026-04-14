# Building & Versioning

## Prerequisites

| Tool | Required for | Install |
|------|-------------|---------|
| Rust toolchain | Everything | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Node.js 18+ | Node package, TS shared | [nodejs.org](https://nodejs.org) |
| Deno | Deno package | `curl -fsSL https://deno.land/install.sh \| sh` |

## Building locally

Run everything for the current platform:

```sh
./scripts/build.sh
```

Skip what you don't need:

```sh
./scripts/build.sh --skip-deno   # just Rust + Node + TS
./scripts/build.sh --skip-rust                  # rebuilds only JS/TS layers
```

The script builds in order:

1. **Rust workspace** — `cargo build --release`
2. **Tauri plugin** — `cargo check` (separate Cargo workspace)
3. **TypeScript shared** — `tsc` → `packages/iroh-http-shared/dist/`
4. **Node addon** — `napi build --platform --release` → `.node` binary + `lib.js`
5. **Deno native lib** — `deno task build` → `lib/*.dylib` (or `.so`/`.dll`)

Each step includes a smoke test where possible.

## Cross-compilation (Deno only, for now)

The Deno package has a full cross-compile script that builds for 5 platforms from macOS:

```sh
cd packages/iroh-http-deno
deno task build:all
```

Requires: `cargo-zigbuild`, `zig`, `mingw-w64`, and the rustup targets:
```sh
rustup target add x86_64-apple-darwin aarch64-apple-darwin \
  x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
  x86_64-pc-windows-gnu
```

Node cross-compilation will be added when we set up CI (napi-rs has a turnkey GitHub Actions template for this).

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

One command to build (all platforms), test, version-bump, and publish:

```sh
./scripts/release.sh 0.2.0           # full release
./scripts/release.sh 0.2.0 --dry-run # everything except publish + push
```

The release script:
1. **Preflight** — checks tools, clean working tree, registry auth
2. **Build** — Rust workspace, TS shared, Node (4 platforms), Deno (5 platforms)
3. **Test** — cargo test, clippy, fmt, tsc, Node e2e, Deno smoke
4. **Version bump** — updates all 12 manifests via `version.sh`
5. **Publish** — crates.io (in dependency order), npm, JSR
6. **Git** — commit, tag `v0.2.0`, print push commands

All cross-compilation happens locally using `cargo-zigbuild` (Linux targets), plain `cargo` (macOS/Windows targets), and `zig` as a linker. No CI needed.

### Prerequisites

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin \
  x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-gnu
cargo install cargo-zigbuild
brew install zig mingw-w64
npm adduser               # npm auth
cargo login               # crates.io auth
```

## Release checklist (manual, for now)

When you're ready to tag a release:

1. `./scripts/version.sh X.Y.Z`
2. `./scripts/build.sh` — verify everything builds clean
3. Commit and tag: `git tag vX.Y.Z`
4. Publish (when ready):
   - **npm:** `cd packages/iroh-http-shared && npm publish` (repeat for node, tauri)
   - **crates.io:** `cargo publish -p iroh-http-core` (then discovery)
   - **JSR:** `cd packages/iroh-http-shared && deno publish`
   - **Deno:** `cd packages/iroh-http-deno && deno publish`

Order matters: shared crates/packages first, then platform packages that depend on them.
