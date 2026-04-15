#!/usr/bin/env bash
# ── release:build ──────────────────────────────────────────────────────────────
# Build native binaries for a specific platform.
#
# Usage:
#   scripts/release/build.sh --platform=deno [--rebuild]
#   scripts/release/build.sh --platform=node [--rebuild]
#
# Guards: skips if binary artifacts exist AND are newer than Rust source.
#         Pass --rebuild to force a full recompile.
source "$(dirname "$0")/_common.sh"

PLATFORM=""
REBUILD=false

for arg in "$@"; do
  case "$arg" in
    --platform=*) PLATFORM="${arg#--platform=}" ;;
    --rebuild)    REBUILD=true ;;
  esac
done

[[ -z "$PLATFORM" ]] && die "Usage: $0 --platform=deno|node [--rebuild]"

section "Build  [$PLATFORM]"

# ── Staleness check ───────────────────────────────────────────────────────────
# Find the newest Rust source file across crates/ and packages/*/src/
newest_rust_source() {
  find "$ROOT/crates" "$ROOT/packages" \
    -name '*.rs' -not -path '*/target/*' \
    -exec stat -f '%m' {} + 2>/dev/null | sort -rn | head -1
}

# Find the oldest artifact for each platform
oldest_artifact() {
  local pattern="$1"
  # shellcheck disable=SC2086
  find $pattern -exec stat -f '%m' {} + 2>/dev/null | sort -n | head -1
}

check_stale() {
  local pattern="$1" label="$2"
  local src_time artifact_time

  if $REBUILD; then
    step "forced rebuild (--rebuild)"
    return 0  # proceed with build
  fi

  src_time=$(newest_rust_source)
  artifact_time=$(oldest_artifact "$pattern")

  if [[ -z "$artifact_time" ]]; then
    step "no existing $label binaries found — building"
    return 0
  fi

  if [[ "$src_time" -gt "$artifact_time" ]]; then
    step "Rust source is newer than $label binaries — rebuilding"
    return 0
  fi

  skip "$label binaries are up to date (use --rebuild to force)"
  return 1
}

# ── Build ──────────────────────────────────────────────────────────────────────

# Core + shared are always needed
build_core_shared() {
  step "build:core"
  npm run build:core || die "build:core failed"
  step "build:shared"
  npm run build:shared || die "build:shared failed"
}

case "$PLATFORM" in
  deno)
    if check_stale "$ROOT/packages/iroh-http-deno/lib/libiroh_http_deno.*" "Deno"; then
      build_core_shared
      step "build:deno:all (5 platforms)"
      npm run build:deno:all || die "build:deno:all failed"
      ok "Deno binaries built"
    fi
    ls -lh "$ROOT/packages/iroh-http-deno/lib/libiroh_http_deno."* 2>/dev/null \
      | awk '{print "    " $NF " (" $5 ")"}'
    ;;
  node)
    if check_stale "$ROOT/packages/iroh-http-node/*.node" "Node"; then
      build_core_shared
      step "build:node:all (4 platforms)"
      npm run build:node:all || die "build:node:all failed"
      ok "Node binaries built"
    fi
    ls -lh "$ROOT/packages/iroh-http-node/"*.node 2>/dev/null \
      | awk '{print "    " $NF " (" $5 ")"}'
    ;;
  *)
    die "Unknown platform: $PLATFORM (expected deno or node)"
    ;;
esac
