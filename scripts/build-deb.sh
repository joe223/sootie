#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")"
UNAME_ARCH="$(uname -m)"
TARGET="${SOOTIE_TARGET:-}"

case "${SOOTIE_DEB_ARCH:-$UNAME_ARCH}" in
  x86_64|amd64) DEB_ARCH="amd64" ;;
  aarch64|arm64) DEB_ARCH="arm64" ;;
  *) echo "Unsupported Debian architecture: ${SOOTIE_DEB_ARCH:-$UNAME_ARCH}" >&2; exit 1 ;;
esac

BUILD="${SOOTIE_BUILD:-1}"
DIST="$ROOT/dist"
STAGE="$DIST/deb/sootie_${VERSION}_${DEB_ARCH}"
DEB="$DIST/sootie_${VERSION}_${DEB_ARCH}.deb"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "dpkg-deb is required to build the Debian package" >&2
  exit 1
fi

if [[ "$BUILD" == "1" ]]; then
  CARGO_ARGS=(build --release --locked --manifest-path "$ROOT/Cargo.toml")
  if [[ -n "$TARGET" ]]; then
    CARGO_ARGS+=(--target "$TARGET")
  fi
  cargo "${CARGO_ARGS[@]}"
fi

if [[ -n "$TARGET" ]]; then
  BIN="$ROOT/target/$TARGET/release/sootie"
else
  BIN="$ROOT/target/release/sootie"
fi
if [[ ! -x "$BIN" ]]; then
  echo "Missing release binary: $BIN" >&2
  exit 1
fi

rm -rf "$STAGE"
mkdir -p \
  "$STAGE/DEBIAN" \
  "$STAGE/usr/bin" \
  "$STAGE/usr/share/doc/sootie" \
  "$STAGE/usr/share/sootie/vision-sidecar"

sed \
  -e "s/@VERSION@/$VERSION/g" \
  -e "s/@ARCH@/$DEB_ARCH/g" \
  "$ROOT/packaging/debian/control" > "$STAGE/DEBIAN/control"
cp "$ROOT/packaging/debian/postinst" "$STAGE/DEBIAN/postinst"
chmod 0755 "$STAGE/DEBIAN/postinst"

cp "$BIN" "$STAGE/usr/bin/sootie"
chmod 0755 "$STAGE/usr/bin/sootie"
cp "$ROOT/README.md" "$STAGE/usr/share/doc/sootie/README.md"
cp "$ROOT/vision-sidecar/server.py" "$STAGE/usr/share/sootie/vision-sidecar/server.py"
cp "$ROOT/vision-sidecar/requirements.txt" "$STAGE/usr/share/sootie/vision-sidecar/requirements.txt"
cp "$ROOT/vision-sidecar/download_model.py" "$STAGE/usr/share/sootie/vision-sidecar/download_model.py"
chmod 0755 "$STAGE/usr/share/sootie/vision-sidecar/server.py" "$STAGE/usr/share/sootie/vision-sidecar/download_model.py"

dpkg-deb --build --root-owner-group "$STAGE" "$DEB"
SHA256="$(sha256sum "$DEB" | awk '{ print $1 }')"

cat <<EOF
deb=$DEB
version=$VERSION
arch=$DEB_ARCH
sha256=$SHA256
EOF
