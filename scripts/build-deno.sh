#!/usr/bin/env bash
# Build the Deno FFI native library.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ok()   { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; }
skip() { echo "  ⏭  $1 (skipped)"; }

echo ""
echo "═══ Deno (FFI) ═══"

if ! command -v deno &>/dev/null; then
  skip "deno not installed"
  exit 0
fi

(cd packages/iroh-http-deno && deno task build 2>&1)
DENO_EXIT=$?

DENO_LIB=$(ls packages/iroh-http-deno/lib/*.{dylib,so,dll} 2>/dev/null | head -1)
if [[ $DENO_EXIT -eq 0 && -n "$DENO_LIB" ]]; then
  ok "deno task build → $(basename "$DENO_LIB") ($(du -h "$DENO_LIB" | cut -f1 | xargs))"
else
  fail "deno task build"
  exit 1
fi
