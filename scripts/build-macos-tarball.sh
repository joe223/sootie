#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")"
ARCH="${SOOTIE_PACKAGE_ARCH:-$(uname -m)}"
TARGET="${SOOTIE_TARGET:-}"

case "$ARCH" in
  arm64|aarch64) PACKAGE_ARCH="arm64" ;;
  x64|x86_64|amd64) PACKAGE_ARCH="x64" ;;
  *) echo "Unsupported macOS architecture: $ARCH" >&2; exit 1 ;;
esac

BUILD="${SOOTIE_BUILD:-1}"
DIST="$ROOT/dist"
STAGE="$DIST/sootie-${VERSION}-macos-${PACKAGE_ARCH}"
TARBALL="$DIST/sootie-${VERSION}-macos-${PACKAGE_ARCH}.tar.gz"

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
mkdir -p "$STAGE/bin" "$STAGE/share/sootie/vision-sidecar"
cp "$BIN" "$STAGE/bin/sootie"
cp "$ROOT/vision-sidecar/server.py" "$STAGE/share/sootie/vision-sidecar/server.py"
cp "$ROOT/vision-sidecar/requirements.txt" "$STAGE/share/sootie/vision-sidecar/requirements.txt"
cp "$ROOT/vision-sidecar/download_model.py" "$STAGE/share/sootie/vision-sidecar/download_model.py"
chmod 0755 "$STAGE/bin/sootie"
chmod 0755 "$STAGE/share/sootie/vision-sidecar/server.py" "$STAGE/share/sootie/vision-sidecar/download_model.py"

rm -f "$TARBALL"
tar -C "$STAGE" -czf "$TARBALL" .
SHA256="$(shasum -a 256 "$TARBALL" | awk '{ print $1 }')"

cat <<EOF
tarball=$TARBALL
version=$VERSION
arch=$PACKAGE_ARCH
sha256=$SHA256
EOF
