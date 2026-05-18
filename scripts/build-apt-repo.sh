#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: scripts/build-apt-repo.sh <deb-file> [suite] [component]" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEB_PATH="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
SUITE="${2:-stable}"
COMPONENT="${3:-main}"
REPO="$ROOT/dist/apt"
BASE_URL="${SOOTIE_APT_BASE_URL:-}"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "dpkg-deb is required to read package metadata" >&2
  exit 1
fi
if ! command -v dpkg-scanpackages >/dev/null 2>&1; then
  echo "dpkg-scanpackages is required to build apt repository metadata" >&2
  exit 1
fi
if [[ ! -f "$DEB_PATH" ]]; then
  echo "Debian package not found: $DEB_PATH" >&2
  exit 1
fi

ARCH="$(dpkg-deb -f "$DEB_PATH" Architecture)"
POOL_DIR="$REPO/pool/$COMPONENT/s/sootie"
BINARY_DIR="$REPO/dists/$SUITE/$COMPONENT/binary-$ARCH"

mkdir -p "$POOL_DIR" "$BINARY_DIR"
cp "$DEB_PATH" "$POOL_DIR/"

(
  cd "$REPO"
  dpkg-scanpackages "pool/$COMPONENT" /dev/null > "$BINARY_DIR/Packages"
  gzip -kf "$BINARY_DIR/Packages"
)

RELEASE_DIR="$REPO/dists/$SUITE"
PACKAGES_REL="$COMPONENT/binary-$ARCH/Packages"
PACKAGES_GZ_REL="$COMPONENT/binary-$ARCH/Packages.gz"
release_entry() {
  local algorithm="$1"
  local file="$2"
  local sum
  local size

  sum="$("$algorithm" "$RELEASE_DIR/$file" | awk '{ print $1 }')"
  size="$(wc -c < "$RELEASE_DIR/$file" | tr -d ' ')"
  printf ' %s %s %s\n' "$sum" "$size" "$file"
}

cat > "$RELEASE_DIR/Release" <<EOF
Origin: Sootie
Label: Sootie
Suite: $SUITE
Codename: $SUITE
Architectures: $ARCH
Components: $COMPONENT
Description: Sootie apt repository
Date: $(date -Ru)
MD5Sum:
$(release_entry md5sum "$PACKAGES_REL")
$(release_entry md5sum "$PACKAGES_GZ_REL")
SHA1:
$(release_entry sha1sum "$PACKAGES_REL")
$(release_entry sha1sum "$PACKAGES_GZ_REL")
SHA256:
$(release_entry sha256sum "$PACKAGES_REL")
$(release_entry sha256sum "$PACKAGES_GZ_REL")
EOF

if [[ -n "$BASE_URL" ]]; then
  cat > "$REPO/sootie.list" <<EOF
deb [trusted=yes arch=$ARCH] $BASE_URL $SUITE $COMPONENT
EOF

  cat > "$REPO/sootie.sources" <<EOF
Types: deb
URIs: $BASE_URL
Suites: $SUITE
Components: $COMPONENT
Architectures: $ARCH
Trusted: yes
EOF
fi

cat <<EOF
repo=$REPO
suite=$SUITE
component=$COMPONENT
arch=$ARCH
packages=$BINARY_DIR/Packages
release=$RELEASE_DIR/Release
EOF
