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

if [[ "${1:-}" != "--skip-build" ]]; then
  (cd "$ROOT" && cargo build --release --locked -p deckcraft -p deckcraft-cli)
fi

release_arch() {
  local arch="$1"
  case "$arch" in
    amd64 | x86_64) echo x86_64 ;;
    arm64 | aarch64) echo aarch64 ;;
    *) echo "$arch" ;;
  esac
  return
}
# The release workflow cross-compiles aarch64 in an x86_64 VM with CARGO_BUILD_TARGET set to
# aarch64-unknown-freebsd (craft-common-actions#39): name the archive after the target.
case "${CARGO_BUILD_TARGET:-}" in
  "") ARCH="$(release_arch "$(uname -m)")" ;;
  aarch64-unknown-freebsd | x86_64-unknown-freebsd) ARCH="$(release_arch "${CARGO_BUILD_TARGET%%-*}")" ;;
  *) echo "CARGO_BUILD_TARGET=$CARGO_BUILD_TARGET is not a supported FreeBSD target" >&2; exit 2 ;;
esac
# Only binaries for this machine run here (the --version check at the end); a FreeBSD target built
# on another OS never does.
HOST_ARCH="$(release_arch "$(uname -m)" 2>/dev/null)" || HOST_ARCH="$(uname -m)"
if [[ -n "${CARGO_BUILD_TARGET:-}" ]] && [[ "$(uname -s)" != FreeBSD ]]; then
  HOST_ARCH="$(uname -s) $HOST_ARCH"
fi
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
if [[ "$ARCH" = "$HOST_ARCH" ]]; then
  "$STAGE/bin/deckcraft-cli" --version
else
  # They cannot run here, so check what they are instead: executables, not libraries or objects.
  # An older file(1) calls a PIE a shared object; its interpreter still marks it as a program.
  want="$ARCH"; [[ "$ARCH" != x86_64 ]] || want=x86-64
  for b in "$STAGE"/bin/*; do
    case "$(file -b "$b")" in
      ELF*" executable, "*"$want"*FreeBSD* | ELF*"$want"*FreeBSD*interpreter*) ;;
      *) echo "error: $b is not a FreeBSD $ARCH executable: $(file -b "$b")" >&2; exit 1 ;;
    esac
  done
  echo "skipping --version: an $ARCH binary does not run on this $HOST_ARCH machine"
fi
echo "wrote $DIST/$NAME.tar.gz"
ls -lh "$DIST/$NAME.tar.gz"
