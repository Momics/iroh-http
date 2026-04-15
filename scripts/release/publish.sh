#!/usr/bin/env bash
# ── release:publish ────────────────────────────────────────────────────────────
# Publish packages to npm/JSR for a specific platform.
#
# Usage:
#   scripts/release/publish.sh --platform=deno
#   scripts/release/publish.sh --platform=node
#
# Guards: gracefully skips any package whose version is already published.
#         Shared is always published first.
source "$(dirname "$0")/_common.sh"

PLATFORM=""

for arg in "$@"; do
  case "$arg" in
    --platform=*) PLATFORM="${arg#--platform=}" ;;
  esac
done

[[ -z "$PLATFORM" ]] && die "Usage: $0 --platform=deno|node"

section "Publish  [$PLATFORM]"

# Shared always goes first (dependency for everything).
try_publish "@momics/iroh-http-shared → npm" "npm run publish:shared"
try_publish "@momics/iroh-http-shared → JSR" "npm run publish:shared:jsr"

case "$PLATFORM" in
  node)
    try_publish "@momics/iroh-http-node → npm" "npm run publish:node"
    ;;
  deno)
    try_publish "@momics/iroh-http-deno → JSR" "npm run publish:deno"
    ;;
  *)
    die "Unknown platform: $PLATFORM (expected deno or node)"
    ;;
esac

ok "publish complete"
