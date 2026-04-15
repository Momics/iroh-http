#!/usr/bin/env bash
# ── release:test ───────────────────────────────────────────────────────────────
# Run tests scoped to a platform, or the shared Rust tests only.
#
# Usage:
#   scripts/release/test.sh                     # Rust + clippy + fmt + typecheck only
#   scripts/release/test.sh --platform=deno     # + Deno smoke tests
#   scripts/release/test.sh --platform=node     # + Node e2e + compliance
source "$(dirname "$0")/_common.sh"

PLATFORM=""

for arg in "$@"; do
  case "$arg" in
    --platform=*) PLATFORM="${arg#--platform=}" ;;
  esac
done

section "Test${PLATFORM:+  [$PLATFORM]}"

# ── Rust tests (always) ──────────────────────────────────────────────────────
step "cargo test --workspace"
cargo test --workspace 2>&1 | grep 'test result:' | while read -r line; do
  echo "    $line"
done
RUST_EXIT=${PIPESTATUS[0]}
[[ $RUST_EXIT -eq 0 ]] && ok "Rust tests" || die "Rust tests failed"

# ── Clippy (always) ──────────────────────────────────────────────────────────
step "cargo clippy --workspace -- -D warnings"
cargo clippy --workspace -- -D warnings 2>&1 | tail -3
ok "clippy"

# ── Fmt check (always) ──────────────────────────────────────────────────────
step "cargo fmt --check"
cargo fmt --all -- --check 2>&1 || die "cargo fmt check failed (run: npm run release:fmt)"
ok "formatting"

# ── TypeScript typecheck (always) ────────────────────────────────────────────
step "npm run typecheck"
npm run typecheck 2>&1 | tail -3
ok "TypeScript typecheck"

# ── Platform-specific tests ──────────────────────────────────────────────────
NODE_PKG="packages/iroh-http-node"

if [[ "$PLATFORM" == "node" ]]; then
  step "Node e2e tests"
  node "$NODE_PKG/test/e2e.mjs" 2>&1 | tail -5
  ok "Node e2e tests"

  if [[ -f "$NODE_PKG/test/compliance.mjs" ]]; then
    step "Node compliance tests"
    node "$NODE_PKG/test/compliance.mjs" 2>&1 | tail -3
    ok "Node compliance"
  fi
fi

if [[ "$PLATFORM" == "deno" ]]; then
  step "Deno smoke tests"
  deno test --allow-read --allow-ffi --allow-env --allow-net \
    packages/iroh-http-deno/test/smoke.test.ts 2>&1 | tail -3
  ok "Deno smoke tests"
fi

echo ""
ok "all tests passed"
