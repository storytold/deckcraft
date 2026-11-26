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
  # The generate example writes deckcraft-cli's man pages and shell completions below; a cross build
  # can't run it. It is built in its own cargo run so its dev-dependencies' features aren't unified
  # into the binaries.
  if [[ -z "${CARGO_BUILD_TARGET:-}" || "$CARGO_BUILD_TARGET" == "$(rustc -vV | sed -n 's/^host: //p')" ]]; then
    (cd "$ROOT" && cargo build --release --locked "${TARGET_ARGS[@]}" -p deckcraft-cli --example generate)
  fi
fi

# Refuse placeholder, wrong-arch or non-FreeBSD binaries (also with --skip-build). Bytes: ELF magic,
# 64-bit little-endian, OSABI SYSV or FreeBSD, e_machine.
case "$ARCH" in
  x86_64) MACHINE=3e00 ;;
  aarch64) MACHINE=b700 ;;
  *) echo "error: no ELF machine check for arch '$ARCH'" >&2; exit 1 ;;
esac
# FreeBSD's od pads its columns differently from GNU od, so compare the hex digits alone.
hex() { od -An -tx1 "$@" | tr -d ' \n'; }
for f in "$BIN/deckcraft" "$BIN/deckcraft-cli"; do
  [[ -f "$f" ]] || { echo "error: $f was not built" >&2; exit 1; }
  [[ "$(hex -N6 "$f")" == 7f454c460201 ]] || { echo "error: $f is not a 64-bit little-endian ELF (starts $(hex -N6 "$f"))" >&2; exit 1; }
  [[ "$(hex -j7 -N1 "$f")" =~ ^(00|09)$ ]] || { echo "error: $f is not a FreeBSD ELF (OSABI $(hex -j7 -N1 "$f"))" >&2; exit 1; }
  [[ "$(hex -j18 -N2 "$f")" == "$MACHINE" ]] || { echo "error: $f is not an ELF for $ARCH (e_machine $(hex -j18 -N2 "$f"))" >&2; exit 1; }
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
# deckcraft-cli's man pages and shell completions, from its clap definition. A cross build can't run
# the example, and a --skip-build run whose prebuilt binaries came without it has none: both leave
# them out.
CLI_DOCS="$BIN/examples/generate"
if [[ -n "${CARGO_BUILD_TARGET:-}" && "$CARGO_BUILD_TARGET" != "$(rustc -vV | sed -n 's/^host: //p')" ]]; then
  warn "cross-building $ARCH, so deckcraft-cli's man pages and completions are left out"
elif [[ ! -x "$CLI_DOCS" ]]; then
  warn "$CLI_DOCS is not built, so deckcraft-cli's man pages and completions are left out"
else
  "$CLI_DOCS" "$WORK/cli-docs"
  mkdir -p "$STAGE/man/man1" "$STAGE/share/bash-completion/completions" \
    "$STAGE/share/zsh/site-functions" "$STAGE/share/fish/vendor_completions.d"
  install -m 644 "$WORK"/cli-docs/man/*.1 "$STAGE/man/man1/"
  install -m 644 "$WORK/cli-docs/completions/deckcraft-cli.bash" "$STAGE/share/bash-completion/completions/deckcraft-cli"
  install -m 644 "$WORK/cli-docs/completions/_deckcraft-cli" "$STAGE/share/zsh/site-functions/_deckcraft-cli"
  install -m 644 "$WORK/cli-docs/completions/deckcraft-cli.fish" "$STAGE/share/fish/vendor_completions.d/deckcraft-cli.fish"
fi

tar -C "$WORK" -czf "$DIST/$NAME.tar.gz" "$NAME"
if [[ -z "${CARGO_BUILD_TARGET:-}" || "$CARGO_BUILD_TARGET" == "$(rustc -vV | sed -n 's/^host: //p')" ]]; then "$STAGE/bin/deckcraft-cli" --version; fi # a cross-built binary cannot run on the build host
echo "wrote $DIST/$NAME.tar.gz"
ls -lh "$DIST/$NAME.tar.gz"
