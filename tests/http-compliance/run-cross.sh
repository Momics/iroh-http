#!/usr/bin/env bash
#
# iroh-http cross-runtime compliance test runner
#
# Runs four test configurations:
#   1. Node  server ↔ Node  client  (same-runtime baseline)
#   2. Deno  server ↔ Deno  client  (same-runtime baseline)
#   3. Node  server ↔ Deno  client  (cross-runtime)
#   4. Deno  server ↔ Node  client  (cross-runtime)
#
# Usage:
#   ./tests/run-cross.sh [--filter <pattern>]
#
# Prerequisites:
#   - node with @momics/iroh-http-node installed (cd node && npm i)
#   - deno available on PATH

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
EXTRA_ARGS="${*:-}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

PASS=0
FAIL=0

run_same_runtime() {
  local runtime=$1
  local label=$2

  echo ""
  echo "════════════════════════════════════════════════════════════"
  echo "  ${label}"
  echo "════════════════════════════════════════════════════════════"

  if [[ "$runtime" == "node" ]]; then
    cd "$ROOT_DIR/node"
    if node "$SCRIPT_DIR/run-node.mjs" $EXTRA_ARGS; then
      echo -e "${GREEN}✓ ${label} PASSED${NC}"
      PASS=$((PASS + 1))
    else
      echo -e "${RED}✗ ${label} FAILED${NC}"
      FAIL=$((FAIL + 1))
    fi
  else
    cd "$ROOT_DIR"
    if deno run -A "$SCRIPT_DIR/run-deno.ts" $EXTRA_ARGS; then
      echo -e "${GREEN}✓ ${label} PASSED${NC}"
      PASS=$((PASS + 1))
    else
      echo -e "${RED}✗ ${label} FAILED${NC}"
      FAIL=$((FAIL + 1))
    fi
  fi
}

run_cross_runtime() {
  local server_runtime=$1
  local client_runtime=$2
  local label=$3

  echo ""
  echo "════════════════════════════════════════════════════════════"
  echo "  ${label}"
  echo "════════════════════════════════════════════════════════════"

  local server_pid=""

  # Start server
  if [[ "$server_runtime" == "node" ]]; then
    cd "$ROOT_DIR/node"
    node "$SCRIPT_DIR/server-node.mjs" &
    server_pid=$!
  else
    cd "$ROOT_DIR"
    deno run -A "$SCRIPT_DIR/server-deno.ts" &
    server_pid=$!
  fi

  # Wait for READY line
  local server_key=""
  local wait_count=0
  while [[ -z "$server_key" ]] && [[ $wait_count -lt 30 ]]; do
    # Read from /proc if available, otherwise just wait and try
    sleep 1
    wait_count=$((wait_count + 1))
  done

  # In practice, the READY protocol needs pipe reading.
  # For simplicity, use a temp file approach:
  local ready_file=$(mktemp)

  # Kill and restart with output capture
  kill "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true

  if [[ "$server_runtime" == "node" ]]; then
    cd "$ROOT_DIR/node"
    node "$SCRIPT_DIR/server-node.mjs" > "$ready_file" 2>&1 &
    server_pid=$!
  else
    cd "$ROOT_DIR"
    deno run -A "$SCRIPT_DIR/server-deno.ts" > "$ready_file" 2>&1 &
    server_pid=$!
  fi

  # Wait for READY
  wait_count=0
  while ! grep -q "^READY " "$ready_file" 2>/dev/null && [[ $wait_count -lt 30 ]]; do
    sleep 1
    wait_count=$((wait_count + 1))
  done

  server_key=$(grep "^READY " "$ready_file" 2>/dev/null | head -1 | awk '{print $2}')

  if [[ -z "$server_key" ]]; then
    echo -e "${RED}✗ ${label} — server did not become ready${NC}"
    kill "$server_pid" 2>/dev/null || true
    rm -f "$ready_file"
    FAIL=$((FAIL + 1))
    return
  fi

  echo "Server key: $server_key"

  # Run client
  local client_exit=0
  if [[ "$client_runtime" == "node" ]]; then
    cd "$ROOT_DIR/node"
    node "$SCRIPT_DIR/client-node.mjs" "$server_key" $EXTRA_ARGS || client_exit=$?
  else
    cd "$ROOT_DIR"
    deno run -A "$SCRIPT_DIR/client-deno.ts" "$server_key" $EXTRA_ARGS || client_exit=$?
  fi

  # Cleanup
  kill "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
  rm -f "$ready_file"

  if [[ $client_exit -eq 0 ]]; then
    echo -e "${GREEN}✓ ${label} PASSED${NC}"
    PASS=$((PASS + 1))
  else
    echo -e "${RED}✗ ${label} FAILED${NC}"
    FAIL=$((FAIL + 1))
  fi
}

# ── Main ─────────────────────────────────────────────────────────────────────
echo "iroh-http Cross-Runtime Compliance Tests"
echo "========================================"

# Same-runtime tests
run_same_runtime "node" "Node ↔ Node"
run_same_runtime "deno" "Deno ↔ Deno"

# Cross-runtime tests
run_cross_runtime "node" "deno" "Node server ↔ Deno client"
run_cross_runtime "deno" "node" "Deno server ↔ Node client"

# Summary
echo ""
echo "════════════════════════════════════════════════════════════"
echo "  SUMMARY"
echo "════════════════════════════════════════════════════════════"
echo -e "  ${GREEN}Passed: $PASS${NC}"
echo -e "  ${RED}Failed: $FAIL${NC}"
echo ""

if [[ $FAIL -gt 0 ]]; then
  exit 1
fi
