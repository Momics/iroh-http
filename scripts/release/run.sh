#!/usr/bin/env bash
# ── release:run ────────────────────────────────────────────────────────────────
# Composed release runner — calls each step in order for a given platform.
#
# Usage:
#   scripts/release/run.sh 0.1.2 --platform=deno [--rebuild] [--dry-run]
#   scripts/release/run.sh 0.1.2 --platform=node [--rebuild] [--dry-run]
#
# Each step is idempotent. If a previous run failed partway through, re-running
# will pick up where it left off (build skips if binaries are current, version
# skips if already bumped, publish skips if already published, etc.).
#
# Equivalent to running each step individually:
#   npm run release:preflight -- --scope=deno
#   npm run release:fmt
#   npm run release:build -- --platform=deno
#   npm run release:test -- --platform=deno
#   npm run release:version -- 0.1.2
#   npm run release:upload:deno -- 0.1.2
#   npm run release:publish -- --platform=deno
#   npm run release:tag -- 0.1.2
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"

VERSION=""
PLATFORM=""
REBUILD=""
DRY_RUN=false

for arg in "$@"; do
  case "$arg" in
    --platform=*) PLATFORM="${arg#--platform=}" ;;
    --rebuild)    REBUILD="--rebuild" ;;
    --dry-run)    DRY_RUN=true ;;
    -*)           echo "Unknown flag: $arg"; exit 1 ;;
    *)            VERSION="$arg" ;;
  esac
done

if [[ -z "$VERSION" ]] || [[ -z "$PLATFORM" ]]; then
  echo "Usage: $0 <version> --platform=deno|node [--rebuild] [--dry-run]"
  exit 1
fi

echo ""
echo "═══════════════════════════════════════════════════════"
echo "  Release v$VERSION  [$PLATFORM]${DRY_RUN:+  (dry-run)}"
echo "═══════════════════════════════════════════════════════"

# 1. Preflight
bash "$DIR/preflight.sh" --scope="$PLATFORM"

# 2. Format
bash "$DIR/fmt.sh"

# 3. Build
bash "$DIR/build.sh" --platform="$PLATFORM" $REBUILD

# 4. Test
bash "$DIR/test.sh" --platform="$PLATFORM"

# 5. Version bump
bash "$DIR/version.sh" "$VERSION"

# 6. Upload (Deno only — binaries to GitHub releases)
if [[ "$PLATFORM" == "deno" ]]; then
  if $DRY_RUN; then
    echo -e "\n  \033[0;33m⏭\033[0m  upload:deno (dry-run)"
  else
    bash "$DIR/upload-deno.sh" "$VERSION"
  fi
fi

# 7. Publish
if $DRY_RUN; then
  echo -e "\n  \033[0;33m⏭\033[0m  publish (dry-run)"
else
  bash "$DIR/publish.sh" --platform="$PLATFORM"
fi

# 8. Tag + push
if $DRY_RUN; then
  echo -e "\n  \033[0;33m⏭\033[0m  tag (dry-run)"
  # Undo version bump
  git checkout -- . 2>/dev/null || true
  echo -e "  \033[0;32m✓\033[0m reverted version bump (dry-run)"
else
  bash "$DIR/tag.sh" "$VERSION"
fi

echo ""
echo "═══════════════════════════════════════════════════════"
echo "  Done — iroh-http v$VERSION [$PLATFORM]"
echo "═══════════════════════════════════════════════════════"
echo ""
