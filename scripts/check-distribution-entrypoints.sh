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
EXPECTED_TOOL_COUNT="${SOOTIE_EXPECTED_TOOL_COUNT:-57}"
REQUIRE_SIGNED_APT="${SOOTIE_REQUIRE_SIGNED_APT:-1}"
APT_PUBLIC_KEY_NAME="${SOOTIE_APT_PUBLIC_KEY_NAME:-sootie-archive-keyring.gpg}"
APT_KEYRING_PATH="${SOOTIE_APT_KEYRING_PATH:-/usr/share/keyrings/$APT_PUBLIC_KEY_NAME}"
REQUIRED_TOOLS=(
  sootie_context
  sootie_browser_connect
  sootie_browser_launch
  sootie_cdp_send
  sootie_learn_status
)

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

check_sootie_binary_contract() {
  local binary="$1"
  local label="$2"
  local tools_json="$tmp_dir/$label-tools.json"
  local python

  "$binary" tools --raw > "$tools_json" \
    || fail "$label sootie binary cannot list tools"
  python="$(command -v python3 || command -v python || true)"
  [[ -n "$python" ]] || fail "python3 or python is required to validate the published tool JSON"
  "$python" - "$tools_json" "$EXPECTED_TOOL_COUNT" "${REQUIRED_TOOLS[@]}" <<'PY' \
    || fail "$label published tool contract does not match"
import json
import sys

path = sys.argv[1]
expected_count = int(sys.argv[2])
required = sys.argv[3:]

with open(path, "r", encoding="utf-8") as handle:
    tools = json.load(handle)

names = [tool.get("name") for tool in tools]
errors = []
if len(names) != expected_count:
    errors.append(f"exposes {len(names)} tools; expected {expected_count}")
for name in required:
    if name not in names:
        errors.append(f"missing {name}")
if errors:
    print("; ".join(errors), file=sys.stderr)
    raise SystemExit(1)
PY
  ok "$label exposes expected Sootie tool contract"
}

check_host_macos_tarball_contract() {
  [[ "$(uname -s)" == "Darwin" ]] || return 0

  local package_arch
  case "$(uname -m)" in
    arm64|aarch64) package_arch="arm64" ;;
    x86_64|amd64) package_arch="x64" ;;
    *) ok "skipping macOS tarball runtime smoke on unsupported host arch $(uname -m)"; return 0 ;;
  esac

  local tarball="$tmp_dir/sootie-macos-$package_arch.tar.gz"
  local extract_dir="$tmp_dir/macos-$package_arch"
  fetch_url \
    "$RELEASE_BASE/sootie-${VERSION}-macos-${package_arch}.tar.gz" \
    "$tarball"
  mkdir -p "$extract_dir"
  tar -xzf "$tarball" -C "$extract_dir"
  [[ -x "$extract_dir/bin/sootie" ]] || fail "macOS $package_arch tarball has no executable bin/sootie"
  check_sootie_binary_contract "$extract_dir/bin/sootie" "macOS $package_arch tarball"
}

extract_deb_control() {
  local deb="$1"
  local output="$2"

  if command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb -f "$deb" > "$output"
  else
    local control_dir="$tmp_dir/deb-control"
    mkdir -p "$control_dir"
    ar -p "$deb" control.tar.gz | tar -xzf - -C "$control_dir"
    if [[ -f "$control_dir/control" ]]; then
      cp "$control_dir/control" "$output"
    elif [[ -f "$control_dir/./control" ]]; then
      cp "$control_dir/./control" "$output"
    else
      fail "Debian package control file not found"
    fi
  fi
}

check_deb_metadata_and_optional_runtime_contract() {
  local deb_url="$1"
  local deb="$tmp_dir/sootie.deb"
  local control="$tmp_dir/deb-control.txt"
  local package_version

  fetch_url "$deb_url" "$deb"
  extract_deb_control "$deb" "$control"
  package_version="$(awk -F': ' '$1 == "Version" { print $2; exit }' "$control")"
  if [[ "$package_version" != "$VERSION" ]]; then
    fail "Debian package version is $package_version; expected $VERSION"
  fi
  if ! grep -q "python3 (>= 3.10)" "$control"; then
    fail "Debian package does not declare the Python 3.10+ setup dependency"
  fi
  ok "Debian package metadata matches version and setup dependencies"

  [[ "$(uname -s)" == "Linux" ]] || return 0
  case "$(uname -m)" in
    x86_64|amd64) ;;
    *) ok "skipping Debian runtime smoke on unsupported host arch $(uname -m)"; return 0 ;;
  esac

  local extract_dir="$tmp_dir/deb-runtime"
  mkdir -p "$extract_dir"
  if command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb -x "$deb" "$extract_dir"
  else
    ar -p "$deb" data.tar.gz | tar -xzf - -C "$extract_dir"
  fi
  [[ -x "$extract_dir/usr/bin/sootie" ]] || fail "Debian package has no executable usr/bin/sootie"
  check_sootie_binary_contract "$extract_dir/usr/bin/sootie" "Debian package"
}

check_apt_source_security() {
  local source_list="$tmp_dir/sootie.list"
  local source_deb822="$tmp_dir/sootie.sources"

  fetch_url "$APT_BASE_URL/sootie.list" "$source_list" \
    || fail "cannot fetch apt list source"
  fetch_url "$APT_BASE_URL/sootie.sources" "$source_deb822" \
    || fail "cannot fetch apt deb822 source"

  if grep -Eiq 'trusted[ =:]+yes' "$source_list" "$source_deb822"; then
    if [[ "$REQUIRE_SIGNED_APT" == "1" ]]; then
      fail "apt source uses trusted=yes; publish a signed repository instead"
    fi
    ok "apt source uses trusted=yes for explicit unsigned dry-run"
    return
  fi

  if ! grep -Fq "signed-by=$APT_KEYRING_PATH" "$source_list"; then
    fail "apt list source does not pin signed-by=$APT_KEYRING_PATH"
  fi
  if ! grep -Fq "Signed-By: $APT_KEYRING_PATH" "$source_deb822"; then
    fail "apt deb822 source does not pin Signed-By: $APT_KEYRING_PATH"
  fi

  check_url "$APT_BASE_URL/$APT_PUBLIC_KEY_NAME"
  check_url "$APT_BASE_URL/dists/stable/InRelease"
  check_url "$APT_BASE_URL/dists/stable/Release.gpg"
  ok "apt sources require signed metadata"
}

formula="$tmp_dir/sootie.rb"
fetch_url "$HOMEBREW_FORMULA_URL" "$formula" \
  || fail "cannot fetch Homebrew formula: $HOMEBREW_FORMULA_URL"
ok "fetched Homebrew formula"

if grep -q ':no_check' "$formula"; then
  fail "Homebrew formula still uses sha256 :no_check"
fi
if ! grep -q "version \"$VERSION\"" "$formula"; then
  fail "Homebrew formula does not declare version $VERSION"
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
check_apt_source_security

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
apt_version="$(awk -F': ' '$1 == "Version" { print $2; exit }' "$packages")"
if [[ "$apt_version" != "$VERSION" ]]; then
  fail "apt Packages metadata version is $apt_version; expected $VERSION"
fi
check_url "$APT_BASE_URL/$apt_deb_path"
check_deb_metadata_and_optional_runtime_contract "$APT_BASE_URL/$apt_deb_path"
check_host_macos_tarball_contract

ok "distribution entrypoints are reachable and installable for version $VERSION"
