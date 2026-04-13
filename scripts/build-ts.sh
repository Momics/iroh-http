#!/usr/bin/env bash
# Build the TypeScript shared package (iroh-http-shared → dist/).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ok()   { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; }

echo ""
echo "═══ TypeScript shared ═══"

npm install --ignore-scripts 2>&1 | tail -3

TSC_OUT=$(cd packages/iroh-http-shared && npx tsc --project tsconfig.json 2>&1)
TSC_EXIT=$?
if [[ $TSC_EXIT -eq 0 ]]; then
  ok "iroh-http-shared → dist/"
else
  fail "tsc (iroh-http-shared)"
  echo "$TSC_OUT"
  exit 1
fi
