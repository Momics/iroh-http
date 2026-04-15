#!/usr/bin/env bash
# ── release:version ────────────────────────────────────────────────────────────
# Bump all manifests to a given version.
#
# Usage:
#   scripts/release/version.sh 0.1.2
#
# Guards: no-op if manifests already at this version.
source "$(dirname "$0")/_common.sh"

VERSION="${1:-}"
require_version "$VERSION"

section "Version bump → $VERSION"

CURRENT=$(current_version)

if [[ "$CURRENT" == "$VERSION" ]]; then
  ok "already at v$VERSION — skipping"
  exit 0
fi

step "bumping $CURRENT → $VERSION"
bash "$ROOT/scripts/version.sh" "$VERSION"
ok "all manifests updated to $VERSION"
