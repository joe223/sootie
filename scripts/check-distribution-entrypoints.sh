#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${SOOTIE_VERSION:-$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")}"
REPO="${SOOTIE_REPO:-joe223/sootie}"
RELEASE_BASE="${SOOTIE_RELEASE_BASE_URL:-https://raw.githubusercontent.com/$REPO/release-assets/v$VERSION}"
HOMEBREW_FORMULA_URL="${SOOTIE_HOMEBREW_FORMULA_URL:-https://raw.githubusercontent.com/joe223/homebrew-sootie/HEAD/Formula/sootie.rb}"
APT_BASE_URL="${SOOTIE_APT_BASE_URL:-https://raw.githubusercontent.com/$REPO/apt}"

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

ok() {
  printf '[ok] %s\n' "$1"
}

fail() {
  printf '[fail] %s\n' "$1" >&2
  exit 1
}

check_url() {
  local url="$1"
  curl -fsSLI --max-time 30 "$url" >/dev/null || fail "not reachable: $url"
  ok "reachable: $url"
}

formula="$tmp_dir/sootie.rb"
curl -fsSL --max-time 30 "$HOMEBREW_FORMULA_URL" -o "$formula" \
  || fail "cannot fetch Homebrew formula: $HOMEBREW_FORMULA_URL"
ok "fetched Homebrew formula"

if grep -q ':no_check' "$formula"; then
  fail "Homebrew formula still uses sha256 :no_check"
fi
if ! grep -q "sootie-${VERSION}-macos-arm64.tar.gz" "$formula"; then
  fail "Homebrew formula does not reference the macOS arm64 release tarball"
fi
if ! grep -q "sootie-${VERSION}-macos-x64.tar.gz" "$formula"; then
  fail "Homebrew formula does not reference the macOS x64 release tarball"
fi
ok "Homebrew formula references versioned tarballs with fixed checksums"

check_url "$RELEASE_BASE/sootie-${VERSION}-macos-arm64.tar.gz"
check_url "$RELEASE_BASE/sootie-${VERSION}-macos-x64.tar.gz"
check_url "$RELEASE_BASE/sootie_${VERSION}_amd64.deb"

check_url "$APT_BASE_URL/dists/stable/Release"
check_url "$APT_BASE_URL/dists/stable/main/binary-amd64/Packages.gz"
check_url "$APT_BASE_URL/sootie.list"
check_url "$APT_BASE_URL/sootie.sources"

ok "distribution entrypoints are reachable for version $VERSION"
