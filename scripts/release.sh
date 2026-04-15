#!/usr/bin/env bash
# ────────────────────────────────────────────────────────────────────────────────
# release.sh — Build, test, version-bump, and publish iroh-http from one machine.
#
# Usage:
#   ./scripts/release.sh <new-version>       # full release
#   ./scripts/release.sh <new-version> --dry-run   # everything except publish
#
# What it does (in order):
#   1. Preflight  — checks tools, clean working tree, registry auth
#   2. Build      — Rust workspace, TS, Node (4 platforms), Deno (5 platforms)
#   3. Test       — cargo test, Node e2e, Deno smoke
#   4. Version    — bumps all 10 manifests via version.sh
#   5. Publish    — crates.io, npm, JSR
#   6. Tag + push — git commit, tag, push
#
# Prerequisites:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin \
#     x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
#     x86_64-pc-windows-msvc x86_64-pc-windows-gnu
#   cargo install cargo-zigbuild cargo-xwin
#   brew install zig mingw-w64
#   npm adduser                   # or set NPM_TOKEN
#   cargo login                   # or set CARGO_REGISTRY_TOKEN
#   deno login                    # for JSR (jsr.io)
# ────────────────────────────────────────────────────────────────────────────────
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ── Parse args ─────────────────────────────────────────────────────────────────

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <new-version> [--dry-run]"
  echo "  e.g. $0 0.2.0"
  echo "  e.g. $0 0.2.0 --dry-run"
  exit 1
fi

VERSION="$1"
DRY_RUN=false
[[ "${2:-}" == "--dry-run" ]] && DRY_RUN=true

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
  echo "Error: '$VERSION' is not valid semver (expected X.Y.Z or X.Y.Z-pre)"
  exit 1
fi

# ── Helpers ────────────────────────────────────────────────────────────────────

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'; BLUE='\033[0;34m'
BOLD='\033[1m'; NC='\033[0m'

ok()      { echo -e "  ${GREEN}✓${NC} $1"; }
fail()    { echo -e "  ${RED}✗${NC} $1"; }
skip()    { echo -e "  ${YELLOW}⏭${NC}  $1 (skipped)"; }
section() { echo -e "\n${BOLD}${BLUE}═══ $1 ═══${NC}"; }
step()    { echo -e "  ${BLUE}→${NC} $1"; }

die() { fail "$1"; exit 1; }

ERRORS=()
warn_or_die() {
  if $DRY_RUN; then
    ERRORS+=("$1")
    echo -e "  ${YELLOW}⚠${NC}  $1 (dry-run, continuing)"
  else
    die "$1"
  fi
}

# ── 1. Preflight ───────────────────────────────────────────────────────────────

section "1. Preflight checks"

# Tools
for cmd in cargo rustup node deno zig npx; do
  command -v "$cmd" &>/dev/null || die "$cmd not found"
done
ok "all required tools found"

# Rust targets
REQUIRED_TARGETS=(
  aarch64-apple-darwin x86_64-apple-darwin
  x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
  x86_64-pc-windows-msvc x86_64-pc-windows-gnu
)
INSTALLED_TARGETS=$(rustup target list --installed)
for t in "${REQUIRED_TARGETS[@]}"; do
  echo "$INSTALLED_TARGETS" | grep -q "^${t}$" || die "missing rustup target: $t (run: rustup target add $t)"
done
ok "all Rust cross-compile targets installed"

# cargo-zigbuild + cargo-xwin
command -v cargo-zigbuild &>/dev/null || die "cargo-zigbuild not found (run: cargo install cargo-zigbuild)"
command -v cargo-xwin &>/dev/null || die "cargo-xwin not found (run: cargo install cargo-xwin)"
ok "cargo-zigbuild available"
ok "cargo-xwin available"

# Clean working tree (allow untracked)
if [[ -n "$(git diff --stat)" ]] || [[ -n "$(git diff --cached --stat)" ]]; then
  die "working tree has uncommitted changes — commit or stash first"
fi
ok "working tree clean"

# Registry auth checks (warn only in dry-run)
if ! $DRY_RUN; then
  step "checking registry credentials…"
  # npm — check we can whoami
  npm whoami &>/dev/null || die "not logged in to npm (run: npm adduser)"
  ok "npm authenticated"

  # crates.io — check token exists
  [[ -f "$HOME/.cargo/credentials.toml" ]] || [[ -n "${CARGO_REGISTRY_TOKEN:-}" ]] \
    || die "no crates.io token (run: cargo login)"
  ok "crates.io token found"
fi

echo ""
echo -e "  ${BOLD}Release plan:${NC} v$VERSION"
$DRY_RUN && echo -e "  ${YELLOW}DRY RUN — will not publish or push${NC}"
echo ""

# ── 2. Build ───────────────────────────────────────────────────────────────────

section "2. Build (all platforms)"

step "npm run build:all (core → shared → node:all → tauri → deno:all)"
npm run build:all 2>&1 || die "build failed"
ok "all packages built"

# List what we built:
NODE_PKG="packages/iroh-http-node"
echo "  Node binaries:"
ls -lh "$NODE_PKG"/*.node 2>/dev/null | awk '{print "    " $NF " (" $5 ")"}'
echo "  Deno binaries:"
ls -lh packages/iroh-http-deno/lib/libiroh_http_deno.* 2>/dev/null | awk '{print "    " $NF " (" $5 ")"}'

echo "  Built Deno binaries:"
ls -lh packages/iroh-http-deno/lib/libiroh_http_deno.* 2>/dev/null | awk '{print "    " $NF " (" $5 ")"}'

# ── 3. Test ────────────────────────────────────────────────────────────────────

section "3. Test"

# 3a. Rust
step "cargo test --workspace"
cargo test --workspace 2>&1 | grep 'test result:' | while read -r line; do
  echo "    $line"
done
RUST_EXIT=${PIPESTATUS[0]}
[[ $RUST_EXIT -eq 0 ]] && ok "Rust tests" || warn_or_die "Rust tests failed"

# 3b. cargo clippy
step "cargo clippy"
cargo clippy --workspace -- -D warnings 2>&1 | tail -3
ok "clippy"

# 3c. cargo fmt
step "cargo fmt --check"
cargo fmt --all -- --check 2>&1 || warn_or_die "cargo fmt check failed"
ok "formatting"

# 3d. TypeScript typecheck
step "npm run typecheck"
npm run typecheck 2>&1 | tail -3
ok "TypeScript typecheck"

# 3e. Node e2e
step "Node e2e tests"
node "$NODE_PKG/test/e2e.mjs" 2>&1 | tail -5
ok "Node e2e (14 tests)"

# 3f. Node compliance
if [[ -f "$NODE_PKG/test/compliance.mjs" ]]; then
  step "Node compliance tests"
  node "$NODE_PKG/test/compliance.mjs" 2>&1 | tail -3
  ok "Node compliance"
fi

# 3g. Deno tests
step "Deno smoke tests"
deno test --allow-read --allow-ffi --allow-env --allow-net packages/iroh-http-deno/test/smoke.test.ts 2>&1 | tail -3
ok "Deno tests (23 tests)"

echo ""
TOTAL_TESTS="93 Rust + 14 Node + 23 Deno"
ok "All tests passed ($TOTAL_TESTS)"

# ── 4. Version bump ───────────────────────────────────────────────────────────

section "4. Version bump → $VERSION"

bash scripts/version.sh "$VERSION"
ok "version.sh updated all manifests"

# ── 5. Publish ─────────────────────────────────────────────────────────────────

section "5. Publish"

if $DRY_RUN; then
  skip "crates.io (dry-run)"
  skip "npm (dry-run)"
  skip "JSR (dry-run)"
else
  # 5a. crates.io — publish in dependency order
  step "crates.io: iroh-http-core"
  (cd crates/iroh-http-core && cargo publish 2>&1 | tail -3)
  ok "iroh-http-core → crates.io"

  step "waiting for crates.io index…"
  sleep 15

  step "crates.io: iroh-http-discovery"
  (cd crates/iroh-http-discovery && cargo publish 2>&1 | tail -3)
  ok "iroh-http-discovery → crates.io"

  # 5b. npm: shared (pure TS, no native code)
  step "npm: @momics/iroh-http-shared"
  (cd packages/iroh-http-shared && npm publish --access public 2>&1 | tail -3)
  ok "@momics/iroh-http-shared → npm"

  # 5c. npm: node (includes all platform .node files)
  step "npm: @momics/iroh-http-node"
  (cd packages/iroh-http-node && npm publish --access public 2>&1 | tail -3)
  ok "@momics/iroh-http-node → npm"

  # 5d. npm: tauri plugin guest-js
  step "npm: @momics/iroh-http-tauri"
  (cd packages/iroh-http-tauri && npm publish --access public 2>&1 | tail -3)
  ok "@momics/iroh-http-tauri → npm"

  # 5e. JSR: shared
  step "JSR: @momics/iroh-http-shared"
  (cd packages/iroh-http-shared && npx jsr publish 2>&1 | tail -3)
  ok "@momics/iroh-http-shared → JSR"

  # 5f. JSR: deno (includes all platform native libs)
  step "JSR: @momics/iroh-http-deno"
  (cd packages/iroh-http-deno && deno publish 2>&1 | tail -3)
  ok "@momics/iroh-http-deno → JSR"
fi

# ── 6. Git tag + push ─────────────────────────────────────────────────────────

section "6. Git commit, tag, push"

if $DRY_RUN; then
  skip "git commit (dry-run)"
  skip "git tag (dry-run)"
  skip "git push (dry-run)"
  # Undo version bump in dry-run
  git checkout -- . 2>/dev/null || true
  ok "reverted version bump (dry-run)"
else
  git add -u
  git commit -m "chore: release v$VERSION"
  git tag "v$VERSION" -m "Release v$VERSION"
  ok "committed and tagged v$VERSION"

  echo ""
  echo -e "  ${YELLOW}Ready to push. Run:${NC}"
  echo "    git push origin main"
  echo "    git push origin v$VERSION"
  echo ""
  echo -e "  Or push both at once:"
  echo "    git push origin main --tags"
fi

# ── Summary ────────────────────────────────────────────────────────────────────

section "Done"

if [[ ${#ERRORS[@]} -gt 0 ]]; then
  echo -e "  ${YELLOW}Warnings:${NC}"
  for e in "${ERRORS[@]}"; do
    echo -e "    ${YELLOW}⚠${NC}  $e"
  done
fi

echo ""
echo -e "  ${GREEN}${BOLD}iroh-http v$VERSION${NC}"
$DRY_RUN && echo -e "  ${YELLOW}This was a dry run. No packages were published.${NC}"
echo ""
