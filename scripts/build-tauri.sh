#!/usr/bin/env bash
# Check the Tauri plugin compiles and build the guest-js TypeScript.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ok()   { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; }

echo ""
echo "═══ Tauri plugin ═══"

if (cd packages/iroh-http-tauri && cargo check 2>&1); then
  ok "cargo check (tauri plugin)"
else
  fail "cargo check (tauri plugin)"
  echo "  (Tauri plugin may need system deps: libgtk-3-dev libwebkit2gtk-4.1-dev etc.)"
  exit 1
fi

TSC_OUT=$(cd packages/iroh-http-tauri && npx tsc --project tsconfig.json 2>&1)
TSC_EXIT=$?
if [[ $TSC_EXIT -eq 0 ]]; then
  ok "guest-js → dist/"
else
  fail "tsc (iroh-http-tauri)"
  echo "$TSC_OUT"
  exit 1
fi
