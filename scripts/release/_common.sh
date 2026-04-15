#!/usr/bin/env bash
# Shared helpers for release step scripts. Source this, don't execute it.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

# ── Colors ─────────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'; BLUE='\033[0;34m'
BOLD='\033[1m'; NC='\033[0m'

ok()      { echo -e "  ${GREEN}✓${NC} $1"; }
fail()    { echo -e "  ${RED}✗${NC} $1"; }
skip()    { echo -e "  ${YELLOW}⏭${NC}  $1 (skipped)"; }
section() { echo -e "\n${BOLD}${BLUE}═══ $1 ═══${NC}"; }
step()    { echo -e "  ${BLUE}→${NC} $1"; }
die()     { fail "$1"; exit 1; }

# ── Semver validation ──────────────────────────────────────────────────────────
require_version() {
  local v="${1:-}"
  if [[ -z "$v" ]]; then
    echo "Error: version argument required (e.g. 0.1.2)"
    exit 1
  fi
  if ! [[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
    echo "Error: '$v' is not valid semver (expected X.Y.Z or X.Y.Z-pre)"
    exit 1
  fi
}

# ── Current version from source of truth ───────────────────────────────────────
current_version() {
  grep '^version = ' "$ROOT/crates/iroh-http-core/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/'
}

# ── Publish helper: skip gracefully if already published ───────────────────────
try_publish() {
  local label="$1" cmd="$2"
  local out rc=0
  out=$(eval "$cmd" 2>&1) || rc=$?
  if [[ $rc -eq 0 ]]; then
    ok "$label"
  elif echo "$out" | grep -qiE "E403|previously published|already exists|EPUBLISHCONFLICT|already been published"; then
    ok "$label (already published — skipped)"
  else
    echo "$out"
    die "publish failed: $label"
  fi
}
