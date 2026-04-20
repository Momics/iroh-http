#!/usr/bin/env bash
# ── release:upload:deno ────────────────────────────────────────────────────────
# Upload Deno native binaries to the Momics/iroh-http-releases GitHub repo.
#
# Usage:
#   scripts/release/upload-deno.sh 0.1.2
#
# Guards: skips if the GitHub release already has all 5 expected assets.
# Requires: gh CLI authenticated with access to Momics/iroh-http-releases.
source "$(dirname "$0")/_common.sh"

VERSION="${1:-}"
require_version "$VERSION"

RELEASE_REPO="Momics/iroh-http"
TAG="v$VERSION"
LIB_DIR="$ROOT/packages/iroh-http-deno/lib"

section "Upload Deno binaries → $RELEASE_REPO@$TAG"

# ── Verify binaries exist locally ────────────────────────────────────────────
EXPECTED_FILES=(
  "libiroh_http_deno.darwin-aarch64.dylib"
  "libiroh_http_deno.darwin-x86_64.dylib"
  "libiroh_http_deno.linux-x86_64.so"
  "libiroh_http_deno.linux-aarch64.so"
  "libiroh_http_deno.windows-x86_64.dll"
)

MISSING=()
for f in "${EXPECTED_FILES[@]}"; do
  [[ -f "$LIB_DIR/$f" ]] || MISSING+=("$f")
done

if [[ ${#MISSING[@]} -gt 0 ]]; then
  for f in "${MISSING[@]}"; do
    fail "missing: $f"
  done
  die "build Deno binaries first (npm run release:build -- --platform=deno)"
fi
ok "all 5 binaries found locally"

# ── Check if release already has all assets ──────────────────────────────────
EXISTING_ASSETS=""
if gh release view "$TAG" --repo "$RELEASE_REPO" &>/dev/null; then
  EXISTING_ASSETS=$(gh release view "$TAG" --repo "$RELEASE_REPO" --json assets -q '.assets[].name')
  ALL_PRESENT=true
  for f in "${EXPECTED_FILES[@]}"; do
    if ! echo "$EXISTING_ASSETS" | grep -q "^${f}$"; then
      ALL_PRESENT=false
      break
    fi
  done
  if $ALL_PRESENT; then
    ok "GitHub release $TAG already has all 5 assets — skipping"
    exit 0
  fi
  step "release $TAG exists but is missing assets — uploading"
else
  step "creating GitHub release $TAG on $RELEASE_REPO"
  gh release create "$TAG" \
    --repo "$RELEASE_REPO" \
    --title "v$VERSION" \
    --notes "Native libraries for iroh-http-deno v$VERSION" \
    --latest
  ok "release $TAG created"
fi

# ── Upload ────────────────────────────────────────────────────────────────────
for f in "${EXPECTED_FILES[@]}"; do
  if echo "$EXISTING_ASSETS" | grep -q "^${f}$"; then
    skip "$f (already uploaded)"
  else
    step "uploading $f"
    gh release upload "$TAG" "$LIB_DIR/$f" --repo "$RELEASE_REPO" --clobber
    ok "$f uploaded"
  fi
done

ok "all Deno binaries uploaded to $RELEASE_REPO@$TAG"
