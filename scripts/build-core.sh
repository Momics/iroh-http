#!/usr/bin/env bash
# Build the Rust workspace.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ok()   { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; }

echo ""
echo "═══ Rust workspace ═══"

if cargo build --release --workspace 2>&1; then
  ok "cargo build --release (workspace)"
else
  fail "cargo build --release"
  exit 1
fi
