#!/usr/bin/env bash
# Build the Node.js napi-rs addon, compile the TS wrapper, and run a smoke test.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ok()   { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; }

echo ""
echo "═══ Node (napi-rs) ═══"

(cd packages/iroh-http-node && npx napi build --platform --release 2>&1)

NODE_BIN=$(ls packages/iroh-http-node/*.node 2>/dev/null | head -1)
if [[ -n "$NODE_BIN" ]]; then
  ok "napi build → $(basename "$NODE_BIN") ($(du -h "$NODE_BIN" | cut -f1 | xargs))"
else
  fail "napi build (no .node file produced)"
  exit 1
fi

TSC_OUT=$(cd packages/iroh-http-node && npx tsc 2>&1)
TSC_EXIT=$?
if [[ $TSC_EXIT -eq 0 ]]; then
  ok "lib.ts → lib.js + lib.d.ts"
else
  fail "tsc (iroh-http-node)"
  echo "$TSC_OUT"
  exit 1
fi

SMOKE_OUT=$(node -e "require('./packages/iroh-http-node/lib.js')" 2>&1)
if [[ $? -eq 0 ]]; then
  ok "require() smoke test passed"
else
  fail "require() smoke test — lib.js failed to load"
  echo "$SMOKE_OUT"
  exit 1
fi
