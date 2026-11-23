#!/usr/bin/env bash
# Build DeckCraft for this Mac and install it as /Applications/DeckCraft.app (ad-hoc signed, for
# local use). The release DMG is package.sh; this is the quick path for development builds.
#
# Usage: packaging/macos/install-local.sh [--skip-build] [--open]
set -euo pipefail
# shellcheck source=../common.sh
. "$(dirname "${BASH_SOURCE[0]}")/../common.sh"
HERE="$ROOT/packaging/macos"

SKIP_BUILD=0
OPEN=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --open) OPEN=1; shift ;;
    -h | --help) sed -n '2,5p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [[ "$SKIP_BUILD" = 0 ]]; then
  (cd "$ROOT" && cargo build --release -p deckcraft)
fi

SHORT_VERSION="${VERSION%%-*}"
WORK="$CARGO_TARGET_DIR/macos-local"
APP="$WORK/DeckCraft.app"
DEST="/Applications/DeckCraft.app"

rm -rf "$WORK"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$CARGO_TARGET_DIR/release/deckcraft" "$APP/Contents/MacOS/DeckCraft"
cp "$ROOT/assets/app-icon/deckcraft.icns" "$APP/Contents/Resources/DeckCraft.icns"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" \
  -e "s/@BUILD_SHA@/${DECKCRAFT_BUILD_SHA:-unknown}/g" \
  "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint -s "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"
codesign --force --sign - --timestamp=none --options runtime --entitlements "$HERE/entitlements.plist" "$APP"

# Quit a running copy before replacing it.
osascript -e 'tell application id "ai.storyteller.deckcraft" to quit' >/dev/null 2>&1 || true
rm -rf "$DEST"
ditto "$APP" "$DEST"
# Refresh Launch Services so Spotlight, the Dock and Open With pick up the new build.
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$DEST" || true
echo "==> installed $DEST ($VERSION, ${DECKCRAFT_BUILD_SHA:0:9})"

if [[ "$OPEN" = 1 ]]; then
  open "$DEST"
fi
