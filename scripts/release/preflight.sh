#!/usr/bin/env bash
# ── release:preflight ──────────────────────────────────────────────────────────
# Check tools, Rust targets, and registry auth.
#
# Usage:
#   scripts/release/preflight.sh [--scope=all|node|deno]
#
# Guards: none — always runs.
source "$(dirname "$0")/_common.sh"

SCOPE="all"
for arg in "$@"; do
  case "$arg" in
    --scope=*) SCOPE="${arg#--scope=}" ;;
  esac
done

section "Preflight checks  [scope=$SCOPE]"

# ── Required CLI tools ────────────────────────────────────────────────────────
TOOLS=(cargo rustup node zig npx)
[[ "$SCOPE" != "node" ]] && TOOLS+=(deno)
[[ "$SCOPE" != "deno" ]] && TOOLS+=(gh)  # gh needed for upload later

for cmd in "${TOOLS[@]}"; do
  command -v "$cmd" &>/dev/null || die "$cmd not found"
done
ok "all required tools found"

# ── Rust targets ──────────────────────────────────────────────────────────────
REQUIRED_TARGETS=(
  aarch64-apple-darwin x86_64-apple-darwin
  x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
)
[[ "$SCOPE" != "deno" ]] && REQUIRED_TARGETS+=(x86_64-pc-windows-msvc)
[[ "$SCOPE" != "node" ]] && REQUIRED_TARGETS+=(x86_64-pc-windows-gnu)

INSTALLED_TARGETS=$(rustup target list --installed)
for t in "${REQUIRED_TARGETS[@]}"; do
  echo "$INSTALLED_TARGETS" | grep -q "^${t}$" \
    || die "missing rustup target: $t (run: rustup target add $t)"
done
ok "all required Rust targets installed"

# ── Cross-compile tools ───────────────────────────────────────────────────────
command -v cargo-zigbuild &>/dev/null \
  || die "cargo-zigbuild not found (run: cargo install cargo-zigbuild)"
ok "cargo-zigbuild available"

if [[ "$SCOPE" != "deno" ]]; then
  command -v cargo-xwin &>/dev/null \
    || die "cargo-xwin not found (run: cargo install cargo-xwin)"
  ok "cargo-xwin available"
fi

# ── Registry auth ─────────────────────────────────────────────────────────────
if [[ "$SCOPE" != "deno" ]]; then
  npm whoami &>/dev/null || die "not logged in to npm (run: npm adduser)"
  ok "npm authenticated"
fi

ok "preflight passed"
