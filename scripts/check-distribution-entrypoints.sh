#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${SOOTIE_VERSION:-$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")}"
REPO="${SOOTIE_REPO:-joe223/sootie}"
RELEASE_BASE="${SOOTIE_RELEASE_BASE_URL:-https://raw.githubusercontent.com/$REPO/release-assets/v$VERSION}"
HOMEBREW_FORMULA_URL="${SOOTIE_HOMEBREW_FORMULA_URL:-https://raw.githubusercontent.com/joe223/homebrew-sootie/HEAD/Formula/sootie.rb}"
APT_BASE_URL="${SOOTIE_APT_BASE_URL:-https://raw.githubusercontent.com/$REPO/apt}"
CURL_MAX_TIME="${SOOTIE_CHECK_MAX_TIME:-90}"
CURL_RETRIES="${SOOTIE_CHECK_RETRIES:-2}"

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
  curl -fsSL \
    --connect-timeout 15 \
    --max-time "$CURL_MAX_TIME" \
    --retry "$CURL_RETRIES" \
    --retry-delay 1 \
    "$url" \
    -o /dev/null || fail "not reachable: $url"
  ok "reachable: $url"
}

fetch_url() {
  local url="$1"
  local output="$2"

  curl -fsSL \
    --connect-timeout 15 \
    --max-time "$CURL_MAX_TIME" \
    --retry "$CURL_RETRIES" \
    --retry-delay 1 \
    "$url" \
    -o "$output"
}

formula="$tmp_dir/sootie.rb"
fetch_url "$HOMEBREW_FORMULA_URL" "$formula" \
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

packages="$tmp_dir/Packages"
packages_gz="$tmp_dir/Packages.gz"
fetch_url \
  "$APT_BASE_URL/dists/stable/main/binary-amd64/Packages.gz" \
  "$packages_gz" || fail "cannot fetch apt Packages metadata"
gzip -dc "$packages_gz" > "$packages"
ok "fetched apt Packages metadata"

apt_deb_path="$(awk -F': ' '$1 == "Filename" { print $2; exit }' "$packages")"
if [[ -z "$apt_deb_path" ]]; then
  fail "apt Packages metadata does not include a Filename field"
fi
check_url "$APT_BASE_URL/$apt_deb_path"

ok "distribution entrypoints are reachable for version $VERSION"
