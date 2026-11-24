#!/usr/bin/env bash
# App-owned GUI-only nightly tar: no FUSE/AppImage, CLI, association or install hooks.
set -euo pipefail
ARCH=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) [[ $# -ge 2 ]] || { echo 'missing --arch value' >&2; exit 2; }; ARCH="$2"; shift 2 ;;
    -h|--help) echo 'Usage: nightly.sh --arch x86_64'; exit 0 ;;
    *) echo "unsupported nightly argument: $1" >&2; exit 2 ;;
  esac
done
[[ "$ARCH" == x86_64 && "$(uname -s)" == Linux ]] || { echo 'only native Linux x86_64 nightly packaging is supported' >&2; exit 2; }
# shellcheck source=../nightly-common.sh
. "$(dirname "${BASH_SOURCE[0]}")/../nightly-common.sh"
nightly_init linux "$ARCH" x86_64-unknown-linux-gnu
OUTPUT="$DIST/deckcraft-$VERSION-linux-x86_64-nightly.tar.gz"
[[ ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] || nightly_die 'refusing to replace an existing nightly archive'
nightly_build_gui
STAGE="$NIGHTLY_WORK/stage"
mkdir -p "$STAGE"
cp "$NIGHTLY_GUI" "$STAGE/deckcraft"
chmod 755 "$STAGE/deckcraft"
# The archive's entry point: an unpacked tar is isolated too, because the launcher passes the
# launch profile the app requires (the bare binary refuses to start without one).
cat >"$STAGE/deckcraft-nightly" <<'LAUNCHER'
#!/bin/sh
# DeckCraft Nightly launcher for the plain tar (no install, no Launcher required).
set -eu
here="$(dirname "$(readlink -f "$0")")"
: "${XDG_CONFIG_HOME:=${HOME:?HOME must be set}/.config}"
: "${XDG_DATA_HOME:=$HOME/.local/share}"
export XDG_CONFIG_HOME="$XDG_CONFIG_HOME/deckcraft-nightly"
export XDG_DATA_HOME="$XDG_DATA_HOME/deckcraft-nightly"
case "${1-}" in
  --version | -V) exec "$here/deckcraft" "$@" ;;
esac
exec "$here/deckcraft" --launch-profile nightly-v1 \
  --profile-root "$XDG_DATA_HOME/launcher/profiles/deckcraft" -- "$@"
LAUNCHER
chmod 755 "$STAGE/deckcraft-nightly"
ICON_SRC="$ROOT/assets/app-icon/deckcraft-nightly.png"
[[ -f "$ICON_SRC" ]] || ICON_SRC="$ROOT/assets/app-icon/deckcraft-1024.png"
cp "$ICON_SRC" "$STAGE/deckcraft-nightly.png"
nightly_copy_notices "$STAGE"
# The private mktemp cwd is the helper authority root, not a CLI path parent.
(cd "$NIGHTLY_WORK" && python3 "$ROOT/packaging/nightly-archive.py" linux stage archive.tar.gz)
nightly_publish_archive "$NIGHTLY_WORK/archive.tar.gz" "$OUTPUT"
