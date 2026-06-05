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
SIGNING_KEY="${SOOTIE_APT_SIGNING_KEY:-}"
PUBLIC_KEY_FILE="${SOOTIE_APT_PUBLIC_KEY_FILE:-}"
PUBLIC_KEY_NAME="${SOOTIE_APT_PUBLIC_KEY_NAME:-sootie-archive-keyring.gpg}"
KEYRING_PATH="${SOOTIE_APT_KEYRING_PATH:-/usr/share/keyrings/$PUBLIC_KEY_NAME}"
ALLOW_UNSIGNED="${SOOTIE_APT_ALLOW_UNSIGNED:-0}"
GPG_HOMEDIR="${SOOTIE_APT_GPG_HOMEDIR:-}"
GPG_PASSPHRASE_FILE="${SOOTIE_APT_GPG_PASSPHRASE_FILE:-}"

if [[ ! -f "$DEB_PATH" ]]; then
  echo "Debian package not found: $DEB_PATH" >&2
  exit 1
fi

SIGNED_APT=0
if [[ -n "$SIGNING_KEY" || -n "$PUBLIC_KEY_FILE" ]]; then
  if [[ -z "$SIGNING_KEY" || -z "$PUBLIC_KEY_FILE" ]]; then
    echo "Set both SOOTIE_APT_SIGNING_KEY and SOOTIE_APT_PUBLIC_KEY_FILE to build a signed apt repository." >&2
    exit 1
  fi
  if [[ ! -f "$PUBLIC_KEY_FILE" ]]; then
    echo "Apt public keyring file not found: $PUBLIC_KEY_FILE" >&2
    exit 1
  fi
  if ! command -v gpg >/dev/null 2>&1; then
    echo "gpg is required to build a signed apt repository." >&2
    exit 1
  fi
  SIGNED_APT=1
elif [[ "$ALLOW_UNSIGNED" != "1" ]]; then
  echo "A signed apt repository requires SOOTIE_APT_SIGNING_KEY and SOOTIE_APT_PUBLIC_KEY_FILE." >&2
  echo "Set SOOTIE_APT_ALLOW_UNSIGNED=1 only for local dry-run metadata." >&2
  exit 1
fi

hash_file() {
  local algorithm="$1"
  local file="$2"

  if command -v "$algorithm" >/dev/null 2>&1; then
    "$algorithm" "$file" | awk '{ print $1 }'
  elif [[ "$algorithm" == "sha256sum" ]]; then
    shasum -a 256 "$file" | awk '{ print $1 }'
  elif [[ "$algorithm" == "sha1sum" ]]; then
    shasum -a 1 "$file" | awk '{ print $1 }'
  elif [[ "$algorithm" == "md5sum" ]]; then
    md5 -q "$file"
  else
    echo "Unsupported hash algorithm: $algorithm" >&2
    exit 1
  fi
}

control_value() {
  local field="$1"
  awk -F': ' -v field="$field" '$1 == field { print $2; exit }' "$CONTROL_FILE"
}

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

CONTROL_FILE="$TMP_DIR/control"
if command -v dpkg-deb >/dev/null 2>&1; then
  dpkg-deb -f "$DEB_PATH" > "$CONTROL_FILE"
else
  ar -p "$DEB_PATH" control.tar.gz | tar -xzf - -C "$TMP_DIR"
  if [[ -f "$TMP_DIR/control" ]]; then
    CONTROL_FILE="$TMP_DIR/control"
  elif [[ -f "$TMP_DIR/./control" ]]; then
    CONTROL_FILE="$TMP_DIR/./control"
  else
    echo "control file not found in Debian package: $DEB_PATH" >&2
    exit 1
  fi
fi

ARCH="$(control_value Architecture)"
if [[ -z "$ARCH" ]]; then
  echo "Architecture field not found in Debian package control metadata" >&2
  exit 1
fi

POOL_DIR="$REPO/pool/$COMPONENT/s/sootie"
BINARY_DIR="$REPO/dists/$SUITE/$COMPONENT/binary-$ARCH"
DEB_NAME="$(basename "$DEB_PATH")"
DEB_REL="pool/$COMPONENT/s/sootie/$DEB_NAME"

rm -rf "$REPO"
mkdir -p "$POOL_DIR" "$BINARY_DIR"
cp "$DEB_PATH" "$POOL_DIR/"

cat "$CONTROL_FILE" > "$BINARY_DIR/Packages"
cat >> "$BINARY_DIR/Packages" <<EOF
Filename: $DEB_REL
Size: $(wc -c < "$DEB_PATH" | tr -d ' ')
MD5sum: $(hash_file md5sum "$DEB_PATH")
SHA1: $(hash_file sha1sum "$DEB_PATH")
SHA256: $(hash_file sha256sum "$DEB_PATH")
EOF
gzip -kf "$BINARY_DIR/Packages"

RELEASE_DIR="$REPO/dists/$SUITE"
PACKAGES_REL="$COMPONENT/binary-$ARCH/Packages"
PACKAGES_GZ_REL="$COMPONENT/binary-$ARCH/Packages.gz"
release_entry() {
  local algorithm="$1"
  local file="$2"
  local sum
  local size

  sum="$(hash_file "$algorithm" "$RELEASE_DIR/$file")"
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

if [[ "$SIGNED_APT" == "1" ]]; then
  GPG_ARGS=(gpg --batch --yes)
  if [[ -n "$GPG_HOMEDIR" ]]; then
    GPG_ARGS+=(--homedir "$GPG_HOMEDIR")
  fi
  if [[ -n "$GPG_PASSPHRASE_FILE" ]]; then
    GPG_ARGS+=(--pinentry-mode loopback --passphrase-file "$GPG_PASSPHRASE_FILE")
  fi

  "${GPG_ARGS[@]}" \
    --local-user "$SIGNING_KEY" \
    --digest-algo SHA256 \
    --clearsign \
    --output "$RELEASE_DIR/InRelease" \
    "$RELEASE_DIR/Release"
  "${GPG_ARGS[@]}" \
    --local-user "$SIGNING_KEY" \
    --digest-algo SHA256 \
    --detach-sign \
    --armor \
    --output "$RELEASE_DIR/Release.gpg" \
    "$RELEASE_DIR/Release"
  cp "$PUBLIC_KEY_FILE" "$REPO/$PUBLIC_KEY_NAME"
fi

if [[ -n "$BASE_URL" ]]; then
  if [[ "$SIGNED_APT" == "1" ]]; then
    cat > "$REPO/sootie.list" <<EOF
deb [signed-by=$KEYRING_PATH arch=$ARCH] $BASE_URL $SUITE $COMPONENT
EOF

    cat > "$REPO/sootie.sources" <<EOF
Types: deb
URIs: $BASE_URL
Suites: $SUITE
Components: $COMPONENT
Architectures: $ARCH
Signed-By: $KEYRING_PATH
EOF
  else
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
fi

cat <<EOF
repo=$REPO
suite=$SUITE
component=$COMPONENT
arch=$ARCH
signed=$SIGNED_APT
packages=$BINARY_DIR/Packages
release=$RELEASE_DIR/Release
EOF
