#!/usr/bin/env bash
# tests/run-all.sh — Run all test suites for a given runtime.
#
# Usage:
#   ./tests/run-all.sh              # Node + Deno
#   ./tests/run-all.sh --node       # Node only
#   ./tests/run-all.sh --deno       # Deno only

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

RUN_NODE=true
RUN_DENO=true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --node) RUN_DENO=false; shift ;;
    --deno) RUN_NODE=false; shift ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

RED='\033[0;31m'
GREEN='\033[0;32m'
BOLD='\033[1m'
NC='\033[0m'

echo "iroh-http Test Suite"
echo "===================="

PASS=0
FAIL=0

if $RUN_NODE; then
  echo ""
  echo -e "${BOLD}── Node.js ──${NC}"
  cd "$ROOT_DIR"
  if node --test tests/runners/node.mjs; then
    echo -e "${GREEN}✓ Node PASSED${NC}"
    PASS=$((PASS + 1))
  else
    echo -e "${RED}✗ Node FAILED${NC}"
    FAIL=$((FAIL + 1))
  fi
fi

if $RUN_DENO; then
  echo ""
  echo -e "${BOLD}── Deno ──${NC}"
  cd "$ROOT_DIR"
  if deno test -A tests/runners/deno.ts; then
    echo -e "${GREEN}✓ Deno PASSED${NC}"
    PASS=$((PASS + 1))
  else
    echo -e "${RED}✗ Deno FAILED${NC}"
    FAIL=$((FAIL + 1))
  fi
fi

echo ""
echo "════════════════════════════════════════════════════════════"
echo -e "  ${GREEN}Passed: $PASS${NC}  ${RED}Failed: $FAIL${NC}"
echo ""
[[ $FAIL -eq 0 ]]
