#!/usr/bin/env bash
# ── release:tag ────────────────────────────────────────────────────────────────
# Commit version bump, create git tag, and push.
#
# Usage:
#   scripts/release/tag.sh 0.1.2
#
# Guards: skips if tag v0.1.2 already exists.
source "$(dirname "$0")/_common.sh"

VERSION="${1:-}"
require_version "$VERSION"

TAG="v$VERSION"

section "Git tag → $TAG"

# ── Guard: tag already exists ─────────────────────────────────────────────────
if git tag -l "$TAG" | grep -q "^${TAG}$"; then
  ok "tag $TAG already exists — skipping"
  exit 0
fi

# ── Commit any staged/unstaged version-bump changes ──────────────────────────
if [[ -n "$(git diff --stat)" ]] || [[ -n "$(git diff --cached --stat)" ]]; then
  step "committing version bump"
  git add -u
  git commit -m "chore: release $TAG"
  ok "committed"
else
  ok "nothing to commit"
fi

# ── Tag ──────────────────────────────────────────────────────────────────────
step "tagging $TAG"
git tag "$TAG" -m "Release $TAG"
ok "tagged $TAG"

# ── Push ──────────────────────────────────────────────────────────────────────
echo ""
echo -e "  ${YELLOW}Ready to push. Run:${NC}"
echo "    git push origin main --tags"
echo ""
