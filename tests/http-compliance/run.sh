#!/usr/bin/env bash
# tests/http-compliance/run.sh
#
# Cross-runtime HTTP compliance test orchestrator.
#
# For each server/client pair defined below, this script:
#   1. Starts the server process and waits for a "READY:<json>" line on stdout.
#   2. Spawns the client process, passing the server's nodeId + addrs as JSON.
#   3. Waits for the client to exit and records pass/fail.
#   4. Kills the server.
#
# Each test pair exercises a DIFFERENT runtime combination, proving that the
# iroh-http protocol layer is interoperable across FFI boundaries — not just
# that each adapter works in isolation.
#
# Usage:
#   bash tests/http-compliance/run.sh
#   bash tests/http-compliance/run.sh --pairs node-deno     # single pair
#
# Prerequisites (all must be on PATH):
#   node (with lib.js compiled in packages/iroh-http-node)
#   deno (with native lib built in packages/iroh-http-deno/lib/)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

FILTER_PAIR="${1:-}"

PASS_TOTAL=0
FAIL_TOTAL=0
declare -a FAILURES

# ── Helpers ───────────────────────────────────────────────────────────────────

ok()   { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; }

# Start a server process, wait for "READY:<json>" on stdout, return the json.
# Usage: SERVER_JSON=$(start_server <cmd> <args...>)
# Side-effect: sets SERVER_PID to the process ID.
SERVER_PID=""
start_server() {
  local tmpfile
  tmpfile=$(mktemp)

  # Start the server; redirect stderr to /dev/null to avoid cluttering output.
  "$@" 2>/dev/null &
  SERVER_PID=$!

  # Read lines until we find READY: or the process dies.
  local deadline=$(( SECONDS + 30 ))
  local ready_json=""
  while [[ $SECONDS -lt $deadline ]]; do
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
      echo "  ERROR: server process died before signalling READY" >&2
      return 1
    fi
    # Check if the server has written to tmpfile... actually pipe via process
    # substitution isn't easy here. Use a named pipe instead.
    break
  done
  # Cleaner approach: have the server write to a temp file, then read it.
  rm -f "$tmpfile"
  echo "$ready_json"
}

# Run a single server/client pair.
# Usage: run_pair <label> <server_cmd> <client_cmd_template>
# The client command receives the READY JSON as its first argument.
run_pair() {
  local label="$1"
  shift
  local server_cmd=("$@")
  # Client command is everything after the -- separator.
  local sep_idx=0
  for i in "${!server_cmd[@]}"; do
    if [[ "${server_cmd[$i]}" == "--" ]]; then
      sep_idx=$i
      break
    fi
  done
  local srv=("${server_cmd[@]:0:$sep_idx}")
  local cli_template=("${server_cmd[@]:$(( sep_idx + 1 ))}")

  echo ""
  echo "  ┌─ $label"

  # Start server, capture stdout to get READY line.
  local ready_file
  ready_file=$(mktemp)
  "${srv[@]}" > "$ready_file" 2>/dev/null &
  local srv_pid=$!

  # Wait for READY line (up to 20s).
  local ready_json=""
  local deadline=$(( SECONDS + 20 ))
  while [[ $SECONDS -lt $deadline ]]; do
    if ! kill -0 "$srv_pid" 2>/dev/null; then
      echo "  │  ERROR: server died before READY" >&2
      rm -f "$ready_file"
      FAIL_TOTAL=$(( FAIL_TOTAL + 1 ))
      FAILURES+=("$label: server died before READY")
      return
    fi
    local line
    line=$(grep '^READY:' "$ready_file" 2>/dev/null | head -1 || true)
    if [[ -n "$line" ]]; then
      ready_json="${line#READY:}"
      break
    fi
    sleep 0.2
  done
  rm -f "$ready_file"

  if [[ -z "$ready_json" ]]; then
    kill "$srv_pid" 2>/dev/null || true
    echo "  │  ERROR: timed out waiting for server READY"
    FAIL_TOTAL=$(( FAIL_TOTAL + 1 ))
    FAILURES+=("$label: server READY timeout")
    return
  fi

  echo "  │  server ready: $(echo "$ready_json" | grep -o '"nodeId":"[^"]*"' | head -1)"

  # Build client command (substitute SERVER_JSON placeholder).
  local cli=()
  for arg in "${cli_template[@]}"; do
    cli+=("${arg/SERVER_JSON/$ready_json}")
  done

  # Run client.
  local client_output
  if client_output=$("${cli[@]}" 2>&1); then
    echo "$client_output" | sed 's/^/  │  /'
    PASS_TOTAL=$(( PASS_TOTAL + 1 ))
    ok "└─ $label"
  else
    local exit_code=$?
    echo "$client_output" | sed 's/^/  │  /'
    fail "└─ $label (exit $exit_code)"
    FAIL_TOTAL=$(( FAIL_TOTAL + 1 ))
    FAILURES+=("$label")
  fi

  kill "$srv_pid" 2>/dev/null || true
}

# ── Pair definitions ──────────────────────────────────────────────────────────
# Format: run_pair "<label>" <server_cmd...> -- <client_cmd with SERVER_JSON placeholder>

echo ""
echo "═══ Cross-runtime HTTP compliance ═══"

# Node server ↔ Deno client
if [[ -z "$FILTER_PAIR" || "$FILTER_PAIR" == "node-deno" ]]; then
  if command -v node &>/dev/null && command -v deno &>/dev/null; then
    run_pair "node → deno" \
      node tests/http-compliance/server.mjs \
      -- \
      deno run --allow-read --allow-ffi \
        tests/http-compliance/client.deno.ts \
        SERVER_JSON
  else
    echo "  ⏭ node → deno (node or deno not found)"
  fi
fi

# Deno server ↔ Node client  (reverse direction)
if [[ -z "$FILTER_PAIR" || "$FILTER_PAIR" == "deno-node" ]]; then
  if command -v deno &>/dev/null && command -v node &>/dev/null; then
    run_pair "deno → node" \
      deno run --allow-read --allow-ffi \
        tests/http-compliance/server.deno.ts \
      -- \
      node tests/http-compliance/client.mjs \
        SERVER_JSON
  else
    echo "  ⏭ deno → node (deno or node not found)"
  fi
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
echo "═══ Results ═══"
echo "  Pairs passed: $PASS_TOTAL"
echo "  Pairs failed: $FAIL_TOTAL"

if [[ ${#FAILURES[@]} -gt 0 ]]; then
  echo "  Failed pairs:"
  for f in "${FAILURES[@]}"; do
    echo "    - $f"
  done
  exit 1
fi
