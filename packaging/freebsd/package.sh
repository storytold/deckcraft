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

TARGET_ARGS=()
BIN="$CARGO_TARGET_DIR/release"
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  # host-tuple means the build host; resolve it so the target dir path is a real triple.
  if [[ "$CARGO_BUILD_TARGET" == host-tuple ]]; then
    CARGO_BUILD_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
  fi
  # The target triple, not the build host's uname, names the arch of a cross build.
  case "$CARGO_BUILD_TARGET" in
    x86_64-unknown-freebsd) ARCH=x86_64 ;;
    aarch64-unknown-freebsd) ARCH=aarch64 ;;
    *) echo "error: unsupported CARGO_BUILD_TARGET '$CARGO_BUILD_TARGET' (expected x86_64-unknown-freebsd or aarch64-unknown-freebsd)" >&2; exit 1 ;;
  esac
  TARGET_ARGS=(--target "$CARGO_BUILD_TARGET")
  BIN="$CARGO_TARGET_DIR/$CARGO_BUILD_TARGET/release"
else
  ARCH="$(uname -m)"
  case "$ARCH" in
    amd64) ARCH=x86_64 ;;
    arm64) ARCH=aarch64 ;;
    *) ;;
  esac
fi

if [[ "${1:-}" != "--skip-build" ]]; then
  (cd "$ROOT" && cargo build --release --locked "${TARGET_ARGS[@]}" -p deckcraft -p deckcraft-cli)
fi

# Refuse placeholder, wrong-arch or non-FreeBSD binaries (also with --skip-build). Bytes: ELF magic,
# 64-bit little-endian, OSABI SYSV or FreeBSD, e_machine.
case "$ARCH" in
  x86_64) MACHINE=" 3e 00" ;;
  aarch64) MACHINE=" b7 00" ;;
  *) echo "error: no ELF machine check for arch '$ARCH'" >&2; exit 1 ;;
esac
for f in "$BIN/deckcraft" "$BIN/deckcraft-cli"; do
  [[ "$(od -An -tx1 -N6 "$f" 2>/dev/null)" == " 7f 45 4c 46 02 01" ]] || { echo "error: $f is not a 64-bit little-endian ELF" >&2; exit 1; }
  [[ "$(od -An -tx1 -j7 -N1 "$f")" =~ ^\ (00|09)$ ]] || { echo "error: $f is not a FreeBSD ELF (OSABI)" >&2; exit 1; }
  [[ "$(od -An -tx1 -j18 -N2 "$f")" == "$MACHINE" ]] || { echo "error: $f is not an ELF for $ARCH" >&2; exit 1; }
done

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
if [[ -z "${CARGO_BUILD_TARGET:-}" || "$CARGO_BUILD_TARGET" == "$(rustc -vV | sed -n 's/^host: //p')" ]]; then "$STAGE/bin/deckcraft-cli" --version; fi # a cross-built binary cannot run on the build host
echo "wrote $DIST/$NAME.tar.gz"
ls -lh "$DIST/$NAME.tar.gz"
