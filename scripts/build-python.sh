#!/usr/bin/env bash
# Build the Python extension with maturin and run an import smoke test.
# Uses `maturin build` (no project-local .venv created) + installs the wheel
# into a uv-managed venv stored in the OS cache dir (not the project tree).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ok()   { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; }
skip() { echo "  ⏭  $1 (skipped)"; }

echo ""
echo "═══ Python (maturin) ═══"

if ! command -v maturin &>/dev/null; then
  skip "maturin not installed (brew install maturin or uv tool install maturin)"
  exit 0
fi

# Build wheel — maturin build never creates a local .venv.
WHEEL_DIR="${ROOT}/packages/iroh-http-py/dist"
BUILD_OUT=$(cd packages/iroh-http-py && maturin build --release --out "$WHEEL_DIR" 2>&1)
echo "$BUILD_OUT" | grep -E "Finished|Built wheel|warning:" || true
WHEEL_FILE=$(ls -t "$WHEEL_DIR"/*.whl 2>/dev/null | head -1)
[[ -z "$WHEEL_FILE" ]] && { fail "no wheel produced"; echo "$BUILD_OUT"; exit 1; }
ok "maturin build --release → $(basename "$WHEEL_FILE")"

# Use a uv-managed venv in the cache dir (not inside the project tree) so that
# shell auto-activate plugins don't pick it up.
CACHE_VENV="${XDG_CACHE_HOME:-$HOME/.cache}/iroh-http-py-build-venv"
uv venv --quiet "$CACHE_VENV" 2>&1 || true
uv pip install --quiet --python "$CACHE_VENV/bin/python" --force-reinstall "$WHEEL_FILE" 2>&1
ok "installed into cache venv"

PY_SMOKE_OUT=$("$CACHE_VENV/bin/python" -c "import iroh_http; print(f'  module: {iroh_http.__name__}')" 2>&1)
if [[ $? -eq 0 ]]; then
  ok "python import smoke test passed"
  echo "$PY_SMOKE_OUT"
else
  fail "python import smoke test"
  echo "$PY_SMOKE_OUT"
  exit 1
fi

if [[ $? -eq 0 ]]; then
  ok "python import smoke test passed"
  echo "$PY_SMOKE_OUT"
else
  fail "python import smoke test"
  echo "$PY_SMOKE_OUT"
  exit 1
fi
