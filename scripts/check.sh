#!/usr/bin/env bash
# ── check ──────────────────────────────────────────────────────────────────────
# Pre-push development check. Mirrors exactly what the CI `verify` job does.
# Run this before pushing to main.
#
# Usage:
#   scripts/check.sh
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

echo "  → cargo fmt"
cargo fmt --all -- --check || die "cargo fmt failed — run: cargo fmt --all"
ok "fmt"

echo "  → cargo clippy (workspace)"
cargo clippy --workspace -- \
  -D warnings \
  -D clippy::unwrap_used \
  -D clippy::panic \
  -D clippy::arithmetic_side_effects
ok "clippy (workspace)"

echo "  → cargo clippy (iroh-http-tauri)"
(cd packages/iroh-http-tauri && cargo clippy -- \
  -D warnings \
  -D clippy::unwrap_used \
  -D clippy::panic \
  -D clippy::arithmetic_side_effects)
ok "clippy (tauri)"

echo "  → cargo test"
cargo test --workspace --quiet
ok "tests"

echo "  → cargo test (iroh-http-tauri)"
(cd packages/iroh-http-tauri && cargo test --quiet)
ok "tests (tauri)"

echo "  → cargo deny"
if command -v cargo-deny &>/dev/null; then
  cargo-deny check
  ok "deny"
else
  echo "     (skipped — cargo-deny not installed; run: cargo install cargo-deny --locked)"
fi

echo "  → cargo bench --test (smoke)"
# Criterion --test mode: one iteration per bench function, no measurement.
# Fast (~10s) and catches bench code that won't compile or panics at startup.
cargo bench -p iroh-http-core -- --test --quiet
ok "bench smoke"

echo "  → cargo check (no-default-features)"
cargo check -p iroh-http-node --no-default-features --features compression --quiet
cargo check -p iroh-http-deno --no-default-features --features compression --quiet
ok "feature checks"

section "TypeScript"

echo "  → build iroh-http-shared"
npm run build --workspace=packages/iroh-http-shared --silent
ok "build"

echo "  → dist drift check (iroh-http-shared)"
# Ensure the committed dist/ matches a fresh tsc build.
# If any file differs, the developer forgot to rebuild before committing.
if ! git diff --quiet packages/iroh-http-shared/dist/; then
  die "packages/iroh-http-shared/dist/ is out of sync with src. Run: npm run build --workspace=packages/iroh-http-shared"
fi
ok "dist drift"

echo "  → typecheck"
npm run typecheck --workspace=packages/iroh-http-shared --silent
npm run typecheck --workspace=packages/iroh-http-tauri --silent
ok "typecheck"

echo ""
echo -e "${GREEN}${BOLD}All checks passed.${NC} Ready to push."
