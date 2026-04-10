#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/build.sh [--skip-rust] [--skip-node] [--skip-deno] [--skip-python] [--skip-ts]
#
# Builds everything locally for the current platform:
#   1. Rust workspace  (cargo build --release)
#   2. TypeScript shared package (tsc)
#   3. Node napi addon (napi build)
#   4. Deno native lib (cargo build + copy)
#   5. Python wheel (maturin develop)
#
# Prerequisites:
#   - Rust toolchain (rustup)
#   - Node.js + npm
#   - Deno (for Deno package)
#   - Python 3.9+ + maturin (pip install maturin) for Python package

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SKIP_RUST=false
SKIP_NODE=false
SKIP_DENO=false
SKIP_PYTHON=false
SKIP_TS=false

for arg in "$@"; do
  case "$arg" in
    --skip-rust)   SKIP_RUST=true ;;
    --skip-node)   SKIP_NODE=true ;;
    --skip-deno)   SKIP_DENO=true ;;
    --skip-python) SKIP_PYTHON=true ;;
    --skip-ts)     SKIP_TS=true ;;
    -h|--help)
      echo "Usage: $0 [--skip-rust] [--skip-node] [--skip-deno] [--skip-python] [--skip-ts]"
      exit 0
      ;;
    *) echo "Unknown arg: $arg"; exit 1 ;;
  esac
done

ok()   { echo "  ✓ $1"; }
skip() { echo "  ⏭ $1 (skipped)"; }
fail() { echo "  ✗ $1"; }

# ── 1. Rust workspace ─────────────────────────────────────────────────────────
echo ""
echo "═══ Rust workspace ═══"
if [[ "$SKIP_RUST" == true ]]; then
  skip "cargo build"
else
  # Build workspace excluding Python (PyO3 needs maturin, not bare cargo build)
  if cargo build --release --workspace --exclude iroh-http-py 2>&1; then
    ok "cargo build --release (workspace, excl. python)"
  else
    fail "cargo build --release"
    exit 1
  fi

  # Tauri plugin is a separate workspace
  if (cd packages/iroh-http-tauri && cargo check 2>&1); then
    ok "cargo check (tauri plugin)"
  else
    fail "cargo check (tauri plugin)"
    echo "  (Tauri plugin may need system deps: libgtk-3-dev libwebkit2gtk-4.1-dev etc.)"
  fi
fi

# ── 2. TypeScript shared ──────────────────────────────────────────────────────
echo ""
echo "═══ TypeScript shared ═══"
if [[ "$SKIP_TS" == true ]]; then
  skip "tsc"
else
  npm install --ignore-scripts 2>&1 | tail -3
  (cd packages/iroh-http-shared && npx tsc --project tsconfig.json 2>&1)
  ok "iroh-http-shared → dist/"
fi

# ── 3. Node napi addon ────────────────────────────────────────────────────────
echo ""
echo "═══ Node (napi-rs) ═══"
if [[ "$SKIP_NODE" == true ]]; then
  skip "napi build"
else
  (cd packages/iroh-http-node && npx napi build --platform --release 2>&1 | tail -3)

  # Check what platform binary was produced
  NODE_BIN=$(ls packages/iroh-http-node/*.node 2>/dev/null | head -1)
  if [[ -n "$NODE_BIN" ]]; then
    ok "napi build → $(basename "$NODE_BIN") ($(du -h "$NODE_BIN" | cut -f1 | xargs))"
  else
    fail "napi build (no .node file produced)"
    exit 1
  fi

  # Compile the TypeScript wrapper
  (cd packages/iroh-http-node && npx tsc 2>&1)
  ok "lib.ts → lib.js + lib.d.ts"

  # Quick import smoke test
  if node -e "require('./packages/iroh-http-node/lib.js')" 2>/dev/null; then
    ok "require() smoke test passed"
  else
    fail "require() smoke test — lib.js failed to load"
  fi
fi

# ── 4. Deno native lib ────────────────────────────────────────────────────────
echo ""
echo "═══ Deno (FFI) ═══"
if [[ "$SKIP_DENO" == true ]]; then
  skip "deno build"
else
  if command -v deno &>/dev/null; then
    (cd packages/iroh-http-deno && deno task build 2>&1 | tail -5)

    DENO_LIB=$(ls packages/iroh-http-deno/lib/*.{dylib,so,dll} 2>/dev/null | head -1)
    if [[ -n "$DENO_LIB" ]]; then
      ok "deno task build → $(basename "$DENO_LIB") ($(du -h "$DENO_LIB" | cut -f1 | xargs))"
    else
      fail "deno task build (no native lib produced)"
    fi
  else
    skip "deno not installed"
  fi
fi

# ── 5. Python wheel ───────────────────────────────────────────────────────────
echo ""
echo "═══ Python (maturin) ═══"
if [[ "$SKIP_PYTHON" == true ]]; then
  skip "maturin"
else
  if command -v maturin &>/dev/null; then
    (cd packages/iroh-http-py && maturin develop --release 2>&1 | tail -5)
    ok "maturin develop --release"

    # Quick import smoke test
    if python3 -c "import iroh_http; print(f'  module: {iroh_http.__name__}')" 2>/dev/null; then
      ok "python3 import smoke test passed"
    else
      fail "python3 import smoke test"
    fi
  else
    skip "maturin not installed (pip install maturin)"
  fi
fi

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo "═══ Build complete ═══"
VERSION=$(grep '^version = ' crates/iroh-http-core/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "  Version: $VERSION"
echo "  Platform: $(uname -ms)"
echo ""
