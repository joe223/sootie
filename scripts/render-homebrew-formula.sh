#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 5 ]]; then
  echo "Usage: scripts/render-homebrew-formula.sh <version> <macos-arm64-url> <macos-arm64-tarball-path> <macos-x64-url> <macos-x64-tarball-path>" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$1"
MACOS_ARM64_URL="$2"
MACOS_ARM64_TARBALL_PATH="$3"
MACOS_X64_URL="$4"
MACOS_X64_TARBALL_PATH="$5"
OUT_DIR="$ROOT/dist/homebrew"
OUT="$OUT_DIR/sootie.rb"

if [[ ! -f "$MACOS_ARM64_TARBALL_PATH" ]]; then
  echo "macOS arm64 tarball not found: $MACOS_ARM64_TARBALL_PATH" >&2
  exit 1
fi
if [[ ! -f "$MACOS_X64_TARBALL_PATH" ]]; then
  echo "macOS x64 tarball not found: $MACOS_X64_TARBALL_PATH" >&2
  exit 1
fi

MACOS_ARM64_SHA256="$(shasum -a 256 "$MACOS_ARM64_TARBALL_PATH" | awk '{ print $1 }')"
MACOS_X64_SHA256="$(shasum -a 256 "$MACOS_X64_TARBALL_PATH" | awk '{ print $1 }')"
mkdir -p "$OUT_DIR"
sed \
  -e "s|@VERSION@|$VERSION|g" \
  -e "s|@MACOS_ARM64_URL@|$MACOS_ARM64_URL|g" \
  -e "s|@MACOS_ARM64_SHA256@|$MACOS_ARM64_SHA256|g" \
  -e "s|@MACOS_X64_URL@|$MACOS_X64_URL|g" \
  -e "s|@MACOS_X64_SHA256@|$MACOS_X64_SHA256|g" \
  "$ROOT/packaging/homebrew/sootie.rb.in" > "$OUT"

cat <<EOF
formula=$OUT
version=$VERSION
macos_arm64_sha256=$MACOS_ARM64_SHA256
macos_x64_sha256=$MACOS_X64_SHA256
EOF
