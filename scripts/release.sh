#!/usr/bin/env bash
set -euo pipefail
#
# Interactive release workflow.
#
# Usage:
#   scripts/release.sh [VERSION] [--skip-ci]
#   npm run release
#   npm run release -- 0.4.0
#
# Steps:
#   1. Determine target version
#   2. Show unreleased commits since last tag
#   3. Run npm run ci — exits immediately on any failure
#   4. Bump all manifests (Cargo.toml, package.json, deno.jsonc, adapter.ts)
#   5. Show diff and ask to confirm
#   6. Commit and tag vX.Y.Z
#   7. Ask whether to push (pushing triggers GitHub Actions build + publish)

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'; BLUE='\033[0;34m'
BOLD='\033[1m'; NC='\033[0m'

ok()      { echo -e "  ${GREEN}✓${NC}  $1"; }
fail()    { echo -e "  ${RED}✗${NC}  $1"; }
warn()    { echo -e "  ${YELLOW}!${NC}  $1"; }
section() { echo -e "\n${BOLD}${BLUE}── $1 ──${NC}"; }
ask()     { printf "\n  ${YELLOW}?${NC}  $1"; }

VERSION=""
SKIP_CI=false

for arg in "$@"; do
  case "$arg" in
    --skip-ci) SKIP_CI=true ;;
    -*)        echo "Unknown flag: $arg"; exit 1 ;;
    *)         VERSION="$arg" ;;
  esac
done

# ── 1. Version ────────────────────────────────────────────────────────────────

section "Release"

CURRENT=$(grep '^version = ' "$ROOT/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "  Current version: ${BOLD}v$CURRENT${NC}"

if [[ -z "$VERSION" ]]; then
  ask "New version (e.g. 0.4.0): "
  read -r VERSION
fi

VERSION="${VERSION// /}"  # trim accidental spaces

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
  fail "'$VERSION' is not valid semver (expected X.Y.Z or X.Y.Z-pre)"
  exit 1
fi

if [[ "$VERSION" == "$CURRENT" ]]; then
  fail "Already at v$VERSION — nothing to bump"
  exit 1
fi

TAG="v$VERSION"
echo "  Target:          ${BOLD}$TAG${NC}"

# ── 2. Unreleased commits ─────────────────────────────────────────────────────

section "Unreleased commits"

LAST_TAG=$(git tag --sort=-version:refname | head -1)
if [[ -n "$LAST_TAG" ]]; then
  git log "$LAST_TAG"..HEAD --oneline | head -20
  COUNT=$(git log "$LAST_TAG"..HEAD --oneline | wc -l | tr -d ' ')
  [[ "$COUNT" -gt 20 ]] && warn "…and $((COUNT - 20)) more"
  echo ""
  echo "  $COUNT commit(s) since $LAST_TAG"
else
  git log --oneline | head -10
fi

# ── 3. CI ─────────────────────────────────────────────────────────────────────

section "CI"

if $SKIP_CI; then
  warn "Skipping CI (--skip-ci)"
else
  if ! npm run ci; then
    echo ""
    fail "CI failed. Fix the issues above and run again."
    exit 1
  fi
  ok "All checks passed"
fi

# ── 4. Version bump ───────────────────────────────────────────────────────────

section "Version bump → $VERSION"
bash "$ROOT/scripts/version.sh" "$VERSION"

# ── 5. Review ─────────────────────────────────────────────────────────────────

echo ""
git diff --stat
echo ""
ask "Commit and tag $TAG? [y/N] "
read -r CONFIRM

if [[ ! "$CONFIRM" =~ ^[Yy]$ ]]; then
  warn "Aborted — reverting version bump"
  git checkout -- .
  exit 0
fi

# ── 6. Commit + tag ───────────────────────────────────────────────────────────

section "Commit + tag"

git add -u
git commit -m "chore: release $TAG"
ok "Committed"

git tag "$TAG" -m "Release $TAG"
ok "Tagged $TAG"

# ── 7. Push ───────────────────────────────────────────────────────────────────

echo ""
ask "Push main + $TAG to origin now? [y/N] "
read -r PUSH

if [[ "$PUSH" =~ ^[Yy]$ ]]; then
  git push origin main --tags
  ok "Pushed"
  echo ""
  echo -e "  ${GREEN}${BOLD}✓ Release $TAG is underway${NC}"
  echo "  GitHub Actions will build all targets and publish."
  echo "  https://github.com/Momics/iroh-http/actions"
else
  echo ""
  echo "  When ready:"
  echo "    git push origin main --tags"
fi

echo ""
