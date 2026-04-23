#!/usr/bin/env bash
# tests/run-all.sh — Run all test categories for a given runtime.
#
# Usage:
#   ./tests/run-all.sh              # Node + Deno (all categories)
#   ./tests/run-all.sh --node       # Node only
#   ./tests/run-all.sh --deno       # Deno only
#   ./tests/run-all.sh --category http-compliance
#   ./tests/run-all.sh --category lifecycle,errors

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

RUN_NODE=true
RUN_DENO=true
CATEGORIES="http-compliance,lifecycle,errors,stress"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --node) RUN_DENO=false; shift ;;
    --deno) RUN_NODE=false; shift ;;
    --category) CATEGORIES="$2"; shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

IFS=',' read -ra CATS <<< "$CATEGORIES"

RED='\033[0;31m'
GREEN='\033[0;32m'
BOLD='\033[1m'
NC='\033[0m'

PASS=0
FAIL=0
FAILURES=()

run_test() {
  local runtime="$1"
  local category="$2"
  local label="${runtime}/${category}"

  echo ""
  echo -e "${BOLD}── ${label} ──${NC}"

  local exit_code=0

  if [[ "$runtime" == "node" ]]; then
    case "$category" in
      http-compliance)
        cd "$ROOT_DIR/node"
        node "$SCRIPT_DIR/http-compliance/run-node.mjs" --verbose || exit_code=$?
        ;;
      lifecycle)
        cd "$ROOT_DIR/node"
        node "$SCRIPT_DIR/lifecycle/test-node.mjs" || exit_code=$?
        ;;
      errors)
        cd "$ROOT_DIR/node"
        node "$SCRIPT_DIR/errors/test-node.mjs" || exit_code=$?
        ;;
      stress)
        cd "$ROOT_DIR/node"
        node "$SCRIPT_DIR/stress/test-node.mjs" || exit_code=$?
        ;;
      *)
        echo "  Unknown category: $category" >&2
        return
        ;;
    esac
  elif [[ "$runtime" == "deno" ]]; then
    cd "$ROOT_DIR"
    case "$category" in
      http-compliance)
        deno run -A "$SCRIPT_DIR/http-compliance/run-deno.ts" --verbose || exit_code=$?
        ;;
      lifecycle)
        deno run -A "$SCRIPT_DIR/lifecycle/test-deno.ts" || exit_code=$?
        ;;
      errors)
        deno run -A "$SCRIPT_DIR/errors/test-deno.ts" || exit_code=$?
        ;;
      stress)
        deno run -A "$SCRIPT_DIR/stress/test-deno.ts" || exit_code=$?
        ;;
      *)
        echo "  Unknown category: $category" >&2
        return
        ;;
    esac
  fi

  if [[ $exit_code -eq 0 ]]; then
    echo -e "${GREEN}✓ ${label} PASSED${NC}"
    PASS=$((PASS + 1))
  else
    echo -e "${RED}✗ ${label} FAILED${NC}"
    FAIL=$((FAIL + 1))
    FAILURES+=("$label")
  fi
}

# ── Main ─────────────────────────────────────────────────────────────────────
echo "iroh-http Test Suite"
echo "===================="

for cat in "${CATS[@]}"; do
  if $RUN_NODE; then
    run_test "node" "$cat"
  fi
  if $RUN_DENO; then
    run_test "deno" "$cat"
  fi
done

# ── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo "════════════════════════════════════════════════════════════"
echo "  SUMMARY"
echo "════════════════════════════════════════════════════════════"
echo -e "  ${GREEN}Passed: $PASS${NC}"
echo -e "  ${RED}Failed: $FAIL${NC}"

if [[ ${#FAILURES[@]} -gt 0 ]]; then
  echo ""
  echo "  Failed:"
  for f in "${FAILURES[@]}"; do
    echo "    - $f"
  done
fi

echo ""
[[ $FAIL -eq 0 ]]
