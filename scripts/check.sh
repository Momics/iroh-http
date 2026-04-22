#!/usr/bin/env bash
# ── check ──────────────────────────────────────────────────────────────────────
# Pre-push development check. Mirrors exactly what the CI `verify` job does.
# Run this before pushing to main.
#
# Each step delegates to an npm script so the same atomic commands work both
# here and when called directly by a developer (e.g. `npm run lint`).
#
# Usage:
#   scripts/check.sh        # full check
#   npm run ci              # same thing
#
# Exit code is non-zero if any check fails.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

RED='\033[0;31m'; GREEN='\033[0;32m'; BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'

ok()      { echo -e "  ${GREEN}✓${NC}  $1"; }
section() { echo -e "\n${BOLD}${BLUE}── $1 ──${NC}"; }
die()     { echo -e "  ${RED}✗${NC}  $1"; exit 1; }

section "Rust"

echo "  → lint"
npm run lint --silent || die "lint failed — run: npm run lint"
ok "lint"

echo "  → test:rust"
npm run test:rust --silent
ok "tests"

echo "  → test:tauri"
npm run test:tauri --silent
ok "tests (tauri)"

echo "  → deny"
if command -v cargo-deny &>/dev/null; then
  cargo-deny check
  ok "deny"
else
  echo "     (skipped — cargo-deny not installed; run: cargo install cargo-deny --locked)"
fi

echo "  → bench:smoke"
npm run bench:smoke --silent
ok "bench smoke"

echo "  → check:features"
npm run check:features --silent
ok "feature checks"

section "Build"

echo "  → build:shared"
npm run build:shared --silent
ok "shared"

echo "  → build:node"
npm run build:node --silent
ok "node"

echo "  → build:deno"
npm run build:deno --silent
ok "deno"

section "TypeScript"

echo "  → typecheck"
npm run typecheck --silent
ok "typecheck"

section "Tests"

echo "  → test:node"
npm run test:node --silent
ok "node"

echo "  → test:deno"
npm run test:deno --silent
ok "deno"

echo "  → test:interop"
npm run test:interop --silent
ok "interop"

echo ""
echo -e "${GREEN}${BOLD}All checks passed.${NC} Ready to push."
