#!/usr/bin/env bash
# ── release:fmt ────────────────────────────────────────────────────────────────
# Apply cargo fmt and auto-commit if anything changed.
#
# Usage:
#   scripts/release/fmt.sh
#
# Guards: no-op if `cargo fmt --check` already passes.
source "$(dirname "$0")/_common.sh"

section "Format"

if cargo fmt --all -- --check 2>/dev/null; then
  ok "formatting already clean"
  exit 0
fi

step "applying cargo fmt --all"
cargo fmt --all
ok "formatted"

if [[ -n "$(git diff --stat)" ]]; then
  step "committing formatting changes"
  git add -u
  git commit -m "chore: apply cargo fmt"
  ok "committed formatting fix"
else
  ok "no changes after fmt (already clean)"
fi
