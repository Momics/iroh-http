#!/usr/bin/env bash
# Build the Python extension with maturin and run an import smoke test.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ok()   { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; }
skip() { echo "  ⏭  $1 (skipped)"; }

echo ""
echo "═══ Python (maturin) ═══"

if ! command -v maturin &>/dev/null; then
  skip "maturin not installed (pip install maturin)"
  exit 0
fi

(cd packages/iroh-http-py && maturin develop --release 2>&1)
ok "maturin develop --release"

PY_BIN="${ROOT}/packages/iroh-http-py/.venv/bin/python"
[[ ! -x "$PY_BIN" ]] && PY_BIN="python3"

PY_SMOKE_OUT=$("$PY_BIN" -c "import iroh_http; print(f'  module: {iroh_http.__name__}')" 2>&1)
if [[ $? -eq 0 ]]; then
  ok "python import smoke test passed"
  echo "$PY_SMOKE_OUT"
else
  fail "python import smoke test"
  echo "$PY_SMOKE_OUT"
  exit 1
fi
