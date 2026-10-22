#!/usr/bin/env bash
# Build and package DeckCraft for FreeBSD:
#
#   $DIST/deckcraft-<version>-freebsd-<arch>.tar.gz   bin/, share/ (desktop entry, icons, AppStream)
#
# Usage: packaging/freebsd/package.sh [--skip-build]
#
# Run on FreeBSD (CI: a FreeBSD 14 VM via vmactions/freebsd-vm). Runtime needs: libxkbcommon,
# wayland or libX11, mesa (Vulkan or EGL) and alsa-lib:
# `pkg install libxkbcommon libX11 mesa-libs vulkan-loader alsa-lib`.
set -euo pipefail
# shellcheck source=../common.sh
. "$(dirname "${BASH_SOURCE[0]}")/../common.sh"
LINUX="$ROOT/packaging/linux"
APP_ID=ai.storyteller.deckcraft

if [ "${1:-}" != "--skip-build" ]; then
  (cd "$ROOT" && cargo build --release --locked -p deckcraft -p deckcraft-cli)
fi

release_arch() {
  case "$1" in
    amd64 | x86_64) echo x86_64 ;;
    arm64 | aarch64) echo aarch64 ;;
    *) echo "$1" ;;
  esac
}
HOST_ARCH="$(release_arch "$(uname -m)")"
# The release workflow cross-compiles aarch64 in an x86_64 VM with CARGO_BUILD_TARGET set to
# aarch64-unknown-freebsd (craft-common-actions#39): name the archive after the target.
ARCH="$HOST_ARCH"
if [ -n "${CARGO_BUILD_TARGET:-}" ]; then ARCH="$(release_arch "${CARGO_BUILD_TARGET%%-*}")"; fi
# Cargo puts a cross build under target/<triple>/release.
BIN="$CARGO_TARGET_DIR/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release"
NAME="deckcraft-$VERSION-freebsd-$ARCH"
WORK="$CARGO_TARGET_DIR/freebsd-package"
STAGE="$WORK/$NAME"
rm -rf "$WORK"
mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$STAGE/share/metainfo" "$STAGE/share/mime/packages" "$STAGE/share/icons" "$STAGE/share/doc/deckcraft"

install -m 755 "$BIN/deckcraft" "$BIN/deckcraft-cli" "$STAGE/bin/"
strip "$STAGE/bin/deckcraft" "$STAGE/bin/deckcraft-cli" 2>/dev/null || true
install -m 644 "$LINUX/$APP_ID.desktop" "$STAGE/share/applications/"
install -m 644 "$LINUX/$APP_ID.mime.xml" "$STAGE/share/mime/packages/$APP_ID.xml"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$DECKCRAFT_BUILD_DATE/g" \
  "$LINUX/$APP_ID.metainfo.xml.in" >"$STAGE/share/metainfo/$APP_ID.metainfo.xml"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/share/icons/"
copy_docs "$STAGE/share/doc/deckcraft"

tar -C "$WORK" -czf "$DIST/$NAME.tar.gz" "$NAME"
if [ "$ARCH" = "$HOST_ARCH" ]; then
  "$STAGE/bin/deckcraft-cli" --version
else
  echo "skipping --version: an $ARCH binary does not run on this $HOST_ARCH machine"
fi
echo "wrote $DIST/$NAME.tar.gz"
ls -lh "$DIST/$NAME.tar.gz"
