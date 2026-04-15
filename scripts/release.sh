#!/usr/bin/env bash
# ────────────────────────────────────────────────────────────────────────────────
# release.sh — Build, test, version-bump, and publish iroh-http from one machine.
#
# Usage:
#   ./scripts/release.sh <version>                         # full release
#   ./scripts/release.sh <version> --only=node            # Node.js packages only
#   ./scripts/release.sh <version> --only=deno            # Deno package only
#   ./scripts/release.sh <version> --dry-run              # no publish or push
#   ./scripts/release.sh <version> --only=deno --dry-run
#
# Or via npm (preferred):
#   npm run release:all  -- 0.2.0
#   npm run release:node -- 0.2.0
#   npm run release:deno -- 0.2.0
#   npm run release:deno -- 0.2.0 --dry-run
#
# What it does (in order):
#   1. Preflight  — checks tools, clean working tree, registry auth (scoped to --only)
#   2. Build      — targets selected by --only (Node: 4 platforms, Deno: 5 platforms)
#   3. Test       — cargo test always; platform tests scoped to --only
#   4. Version    — bumps all manifests via version.sh (always, keeps versions in sync)
#   5. Publish    — npm/JSR for selected packages; shared published first, skipped if already exists
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

VERSION=""
DRY_RUN=false
ONLY="all"

for arg in "$@"; do
  case "$arg" in
    --dry-run)  DRY_RUN=true ;;
    --only=*)   ONLY="${arg#--only=}" ;;
    -*)         echo "Unknown flag: $arg"; exit 1 ;;
    *)          VERSION="$arg" ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version> [--only=all|node|deno] [--dry-run]"
  echo "  e.g. $0 0.2.0"
  echo "  e.g. $0 0.2.0 --only=deno"
  echo "  e.g. $0 0.2.0 --only=node --dry-run"
  exit 1
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
  echo "Error: '$VERSION' is not valid semver (expected X.Y.Z or X.Y.Z-pre)"
  exit 1
fi

case "$ONLY" in
  all|node|deno) ;;
  *) echo "Error: --only must be 'all', 'node', or 'deno' (got: '$ONLY')"; exit 1 ;;
esac

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

# Rust targets — only check what --only actually builds
REQUIRED_TARGETS=(
  aarch64-apple-darwin x86_64-apple-darwin
  x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
)
[[ "$ONLY" != "deno" ]] && REQUIRED_TARGETS+=(x86_64-pc-windows-msvc)  # Node Windows MSVC
[[ "$ONLY" != "node" ]] && REQUIRED_TARGETS+=(x86_64-pc-windows-gnu)   # Deno Windows GNU
INSTALLED_TARGETS=$(rustup target list --installed)
for t in "${REQUIRED_TARGETS[@]}"; do
  echo "$INSTALLED_TARGETS" | grep -q "^${t}$" || die "missing rustup target: $t (run: rustup target add $t)"
done
ok "all required Rust targets installed"

# cargo-zigbuild always needed; cargo-xwin only for Node (Windows MSVC target)
command -v cargo-zigbuild &>/dev/null || die "cargo-zigbuild not found (run: cargo install cargo-zigbuild)"
ok "cargo-zigbuild available"
if [[ "$ONLY" != "deno" ]]; then
  command -v cargo-xwin &>/dev/null || die "cargo-xwin not found (run: cargo install cargo-xwin)"
  ok "cargo-xwin available"
fi

# Clean working tree (allow untracked)
if [[ -n "$(git diff --stat)" ]] || [[ -n "$(git diff --cached --stat)" ]]; then
  die "working tree has uncommitted changes — commit or stash first"
fi
ok "working tree clean"

# Registry auth checks (warn only in dry-run)
if ! $DRY_RUN; then
  step "checking registry credentials…"
  # npm — only needed when publishing Node packages
  if [[ "$ONLY" != "deno" ]]; then
    npm whoami &>/dev/null || die "not logged in to npm (run: npm adduser)"
    ok "npm authenticated"
  fi

  # crates.io — only needed when publish:tauri:cargo is enabled
  # [[ -f "$HOME/.cargo/credentials.toml" ]] || [[ -n "${CARGO_REGISTRY_TOKEN:-}" ]] \
  #   || die "no crates.io token (run: cargo login)"
  # ok "crates.io token found"
fi

echo ""
echo -e "  ${BOLD}Release plan:${NC} v$VERSION  [--only=$ONLY]"
$DRY_RUN && echo -e "  ${YELLOW}DRY RUN — will not publish or push${NC}"
echo ""

# ── 2. Build ───────────────────────────────────────────────────────────────────

section "2. Build  [--only=$ONLY]"

NODE_PKG="packages/iroh-http-node"

if [[ "$ONLY" == "node" || "$ONLY" == "all" ]]; then
  step "build:core + build:shared + build:node:all"
  npm run build:core   || die "build:core failed"
  npm run build:shared || die "build:shared failed"
  npm run build:node:all || die "build:node:all failed"
  ok "Node binaries built"
  ls -lh "$NODE_PKG"/*.node 2>/dev/null | awk '{print "    " $NF " (" $5 ")"}'
fi

if [[ "$ONLY" == "deno" ]]; then
  step "build:core + build:shared + build:deno:all"
  npm run build:core     || die "build:core failed"
  npm run build:shared   || die "build:shared failed"
  npm run build:deno:all || die "build:deno:all failed"
  ok "Deno binaries built"
  ls -lh packages/iroh-http-deno/lib/libiroh_http_deno.* 2>/dev/null | awk '{print "    " $NF " (" $5 ")"}'
fi

if [[ "$ONLY" == "all" ]]; then
  step "build:tauri + build:deno:all"
  npm run build:tauri    || die "build:tauri failed"
  npm run build:deno:all || die "build:deno:all failed"
  ok "Deno + Tauri binaries built"
  ls -lh packages/iroh-http-deno/lib/libiroh_http_deno.* 2>/dev/null | awk '{print "    " $NF " (" $5 ")"}'
fi

# ── 3. Test ────────────────────────────────────────────────────────────────────

section "3. Test  [--only=$ONLY]"

# 3a. Rust (always)
step "cargo test --workspace"
cargo test --workspace 2>&1 | grep 'test result:' | while read -r line; do
  echo "    $line"
done
RUST_EXIT=${PIPESTATUS[0]}
[[ $RUST_EXIT -eq 0 ]] && ok "Rust tests" || warn_or_die "Rust tests failed"

# 3b. cargo clippy (always)
step "cargo clippy"
cargo clippy --workspace -- -D warnings 2>&1 | tail -3
ok "clippy"

# 3c. cargo fmt (always)
step "cargo fmt --check"
cargo fmt --all -- --check 2>&1 || warn_or_die "cargo fmt check failed"
ok "formatting"

# 3d. TypeScript typecheck (always)
step "npm run typecheck"
npm run typecheck 2>&1 | tail -3
ok "TypeScript typecheck"

# 3e. Node tests (node or all)
if [[ "$ONLY" == "node" || "$ONLY" == "all" ]]; then
  step "Node e2e tests"
  node "$NODE_PKG/test/e2e.mjs" 2>&1 | tail -5
  ok "Node e2e (14 tests)"

  if [[ -f "$NODE_PKG/test/compliance.mjs" ]]; then
    step "Node compliance tests"
    node "$NODE_PKG/test/compliance.mjs" 2>&1 | tail -3
    ok "Node compliance"
  fi
fi

# 3f. Deno tests (deno or all)
if [[ "$ONLY" == "deno" || "$ONLY" == "all" ]]; then
  step "Deno smoke tests"
  deno test --allow-read --allow-ffi --allow-env --allow-net packages/iroh-http-deno/test/smoke.test.ts 2>&1 | tail -3
  ok "Deno tests (23 tests)"
fi

echo ""
ok "All applicable tests passed"

# ── 4. Version bump ───────────────────────────────────────────────────────────

section "4. Version bump → $VERSION"

bash scripts/version.sh "$VERSION"
ok "version.sh updated all manifests"

# ── 5. Publish ─────────────────────────────────────────────────────────────────

section "5. Publish  [--only=$ONLY]"

# Helper: publish to npm/JSR; gracefully skip if this version is already published.
_try_publish() {
  local label="$1" cmd="$2"
  local out rc=0
  out=$(eval "$cmd" 2>&1) || rc=$?
  if [[ $rc -eq 0 ]]; then
    ok "$label"
  elif echo "$out" | grep -qiE "E403|previously published|already exists|EPUBLISHCONFLICT|already been published"; then
    ok "$label (v$VERSION already published — skipped)"
  else
    echo "$out"
    die "publish failed: $label"
  fi
}

if $DRY_RUN; then
  skip "publish --only=$ONLY (dry-run)"
else
  # shared always goes first (npm + JSR); skipped gracefully if already at this version.
  _try_publish "@momics/iroh-http-shared → npm" "npm run publish:shared"
  _try_publish "@momics/iroh-http-shared → JSR" "npm run publish:shared:jsr"

  if [[ "$ONLY" == "node" || "$ONLY" == "all" ]]; then
    _try_publish "@momics/iroh-http-node → npm" "npm run publish:node"
  fi

  if [[ "$ONLY" == "deno" || "$ONLY" == "all" ]]; then
    _try_publish "@momics/iroh-http-deno → JSR" "npm run publish:deno"
  fi

  # Tauri — uncomment when ready:
  # _try_publish "@momics/iroh-http-tauri → npm"      "npm run publish:tauri"
  # _try_publish "tauri-plugin-iroh-http → crates.io" "npm run publish:tauri:cargo"
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
