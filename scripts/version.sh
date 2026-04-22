#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/version.sh 0.2.0
#
# Bumps the version across ALL package manifests:
#   - 5 Cargo.toml  (2 crates + 3 packages, excluding examples)
#   - 3 package.json (node, tauri, shared)
#   - 1 deno.jsonc
#   - 1 jsr.jsonc
#
# Also updates inter-crate dependency versions and the Deno import map.
# Does NOT touch lockfiles or examples — those follow naturally.

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <new-version>"
  echo "  e.g. $0 0.2.0"
  exit 1
fi

NEW="$1"

# Validate semver-ish format
if ! [[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
  echo "Error: '$NEW' doesn't look like a valid version (expected X.Y.Z or X.Y.Z-pre)"
  exit 1
fi

# Detect current version from the source of truth (iroh-http-core)
OLD=$(grep '^version = ' "$ROOT/crates/iroh-http-core/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "Bumping $OLD → $NEW"

# ── Cargo.toml files ──────────────────────────────────────────────────────────
CARGO_FILES=(
  crates/iroh-http-core/Cargo.toml
  crates/iroh-http-discovery/Cargo.toml
  packages/iroh-http-node/Cargo.toml
  packages/iroh-http-deno/Cargo.toml
  packages/iroh-http-tauri/Cargo.toml
)

for f in "${CARGO_FILES[@]}"; do
  filepath="$ROOT/$f"
  if [[ ! -f "$filepath" ]]; then
    echo "  SKIP (not found): $f"
    continue
  fi
  # Replace the package version line
  sed -i '' "s/^version = \"$OLD\"/version = \"$NEW\"/" "$filepath"
  # Replace internal dependency versions (e.g. iroh-http-framing = { path = "...", version = "0.1.0" })
  sed -i '' "s/\(iroh-http-[a-z]*.*version = \"\)$OLD\"/\1$NEW\"/" "$filepath"
  echo "  ✓ $f"
done

# ── package.json files ────────────────────────────────────────────────────────
JSON_FILES=(
  packages/iroh-http-shared/package.json
  packages/iroh-http-node/package.json
  packages/iroh-http-tauri/package.json
)

for f in "${JSON_FILES[@]}"; do
  filepath="$ROOT/$f"
  if [[ ! -f "$filepath" ]]; then
    echo "  SKIP (not found): $f"
    continue
  fi
  # Only replace the top-level "version" field (line must start with  "version")
  sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" "$filepath"
  echo "  ✓ $f"
done

# ── platform package.json files (iroh-http-node npm/ split) ──────────────────
PLATFORM_DIRS=(
  packages/iroh-http-node/npm/darwin-arm64
  packages/iroh-http-node/npm/darwin-x64
  packages/iroh-http-node/npm/linux-x64-gnu
  packages/iroh-http-node/npm/linux-arm64-gnu
  packages/iroh-http-node/npm/win32-x64-msvc
)

for dir in "${PLATFORM_DIRS[@]}"; do
  filepath="$ROOT/$dir/package.json"
  if [[ ! -f "$filepath" ]]; then
    echo "  SKIP (not found): $dir/package.json"
    continue
  fi
  sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" "$filepath"
  # Update pinned optionalDependency version in the main node package.json
  pkg=$(basename "$dir")
  sed -i '' "s/\"@momics\/iroh-http-node-$pkg\": \"$OLD\"/\"@momics\/iroh-http-node-$pkg\": \"$NEW\"/" "$ROOT/packages/iroh-http-node/package.json"
  echo "  ✓ $dir/package.json"
done

# ── deno.jsonc ────────────────────────────────────────────────────────────────
DENO_JSON="$ROOT/packages/iroh-http-deno/deno.jsonc"
if [[ -f "$DENO_JSON" ]]; then
  sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" "$DENO_JSON"
  # Update the shared import version range (^0.1 → ^0.2, etc.)
  MAJOR_MINOR=$(echo "$NEW" | sed 's/\([0-9]*\.[0-9]*\).*/\1/')
  sed -i '' "s|@momics/iroh-http-shared@\^[0-9.]*|@momics/iroh-http-shared@^$MAJOR_MINOR|" "$DENO_JSON"
  echo "  ✓ packages/iroh-http-deno/deno.jsonc"
fi

# ── shared deno.json (for JSR publish) ────────────────────────────────────────
SHARED_DENO="$ROOT/packages/iroh-http-shared/deno.json"
if [[ -f "$SHARED_DENO" ]]; then
  sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" "$SHARED_DENO"
  echo "  ✓ packages/iroh-http-shared/deno.json"
fi

# ── Deno adapter VERSION constant ────────────────────────────────────────────
ADAPTER_TS="$ROOT/packages/iroh-http-deno/src/adapter.ts"
if [[ -f "$ADAPTER_TS" ]]; then
  sed -i '' "s/^const VERSION = \"$OLD\";/const VERSION = \"$NEW\";/" "$ADAPTER_TS"
  echo "  ✓ packages/iroh-http-deno/src/adapter.ts (VERSION)"
fi

# ── npm consumer dependency ranges ──────────────────────────────────────────
# iroh-http-node and iroh-http-tauri declare @momics/iroh-http-shared as a
# semver range (^MAJOR.MINOR).  When the minor version crosses a boundary the
# old range can no longer resolve the new version, so we update it here.
MAJOR_MINOR=$(echo "$NEW" | sed 's/\([0-9]*\.[0-9]*\).*/\1/')

NODE_PKG="$ROOT/packages/iroh-http-node/package.json"
if [[ -f "$NODE_PKG" ]]; then
  sed -i '' "s|\"@momics/iroh-http-shared\": \"\^[0-9.]*\"|\"@momics/iroh-http-shared\": \"^$MAJOR_MINOR\"|" "$NODE_PKG"
  echo "  ✓ packages/iroh-http-node/package.json (@momics/iroh-http-shared dep range → ^$MAJOR_MINOR)"
fi

TAURI_PKG="$ROOT/packages/iroh-http-tauri/package.json"
if [[ -f "$TAURI_PKG" ]]; then
  sed -i '' "s|\"@momics/iroh-http-shared\": \"\^[0-9.]*\"|\"@momics/iroh-http-shared\": \"^$MAJOR_MINOR\"|" "$TAURI_PKG"
  echo "  ✓ packages/iroh-http-tauri/package.json (@momics/iroh-http-shared dep range → ^$MAJOR_MINOR)"
fi

# ── Regenerate lock files ─────────────────────────────────────────────────────
echo ""
echo "Regenerating Cargo.lock …"
cargo generate-lockfile --manifest-path "$ROOT/Cargo.toml"
echo "  ✓ Cargo.lock"

echo "Regenerating package-lock.json …"
(cd "$ROOT" && npm install --package-lock-only --ignore-scripts)
echo "  ✓ package-lock.json"

echo "Regenerating deno.lock …"
(cd "$ROOT" && deno install --frozen=false --quiet 2>/dev/null || true)
echo "  ✓ deno.lock"

echo ""
echo "Done. All manifests and lock files updated to $NEW."
echo "Review:  git diff --stat"
echo "Next:    npm run release:tag -- $NEW"
