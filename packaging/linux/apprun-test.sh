#!/usr/bin/env bash
# Smoke test for packaging/linux/AppRun: the AppImage's desktop integration writes
# and refreshes the host .desktop entry and icons, is idempotent, honours the opt-out
# and an unwritable HOME, supports --cli, and always launches the app.
#
# Runs in seconds with no build (CI: .github/workflows/packaging-lint.yml).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
APP_ID=ai.storyteller.deckcraft

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Fake AppDir with the real AppRun, .desktop template, icons, and stub binaries.
APPDIR="$TMP/DeckCraft.AppDir"
mkdir -p "$APPDIR/usr/bin"
install -Dm755 "$HERE/AppRun" "$APPDIR/AppRun"
install -Dm644 "$HERE/$APP_ID.desktop" "$APPDIR/usr/share/applications/$APP_ID.desktop"
mkdir -p "$APPDIR/usr/share/icons"
cp -R "$ROOT/assets/app-icon/hicolor" "$APPDIR/usr/share/icons/"
printf '#!/bin/sh\necho "stub: $*"\n' >"$APPDIR/usr/bin/deckcraft"
chmod +x "$APPDIR/usr/bin/deckcraft"
printf '#!/bin/sh\necho "cli: $*"\n' >"$APPDIR/usr/bin/deckcraft-cli"
chmod +x "$APPDIR/usr/bin/deckcraft-cli"

APPIMAGE="$TMP/deckcraft test.AppImage" # spaces on purpose: Exec must stay quoted
: >"$APPIMAGE"
export APPIMAGE
export HOME="$TMP/home"
export XDG_DATA_HOME="$HOME/.local/share"
mkdir -p "$HOME"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# 1. --cli forwards to deckcraft-cli without desktop integration (#1606).
out="$("$APPDIR/AppRun" --cli mcp --sample)" || fail "--cli exited non-zero"
[[ "$out" = "cli: mcp --sample" ]] || fail "--cli not forwarded: $out"
[[ ! -e "$XDG_DATA_HOME" ]] || fail "--cli ran desktop integration"

# 2. ARGV0=deckcraft-cli forwards to deckcraft-cli.
out="$(ARGV0="deckcraft-cli" "$APPDIR/AppRun" run --sample)" || fail "ARGV0 exited non-zero"
[[ "$out" = "cli: run --sample" ]] || fail "ARGV0 not forwarded: $out"
[[ ! -e "$XDG_DATA_HOME" ]] || fail "ARGV0 ran desktop integration"

# 3. Launch works and passes arguments through to deckcraft.
out="$("$APPDIR/AppRun" open presentation.deckcraft)" || fail "AppRun exited non-zero"
[[ "$out" = "stub: open presentation.deckcraft" ]] || fail "arguments not forwarded: $out"

DESKTOP="$XDG_DATA_HOME/applications/$APP_ID.desktop"
[[ -f "$DESKTOP" ]] || fail "desktop entry not installed"

# 4. Exec rewritten to the AppImage (quoted, spaces intact), TryExec dropped,
#    everything else preserved from the packaged entry.
expected_exec="Exec=\"$(readlink -f "$APPIMAGE")\" %F"
grep -Fxq "$expected_exec" "$DESKTOP" || fail "Exec not rewritten: $(grep '^Exec=' "$DESKTOP")"
grep -q '^TryExec=' "$DESKTOP" && fail "TryExec kept, the entry would be hidden"
grep -Fxq "Icon=$APP_ID" "$DESKTOP" || fail "Icon line lost"
grep -Fxq "X-AppImage-Integrate=false" "$DESKTOP" || fail "X-AppImage-Integrate=false missing"
grep -Fxq "X-AppImage-Generated-By=deckcraft-AppRun" "$DESKTOP" || fail "X-AppImage-Generated-By missing"
grep -Fxq 'Categories=Office;Presentation;' "$DESKTOP" || fail "metadata lines lost"
if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$DESKTOP" || fail "generated desktop entry is invalid"
fi

# 5. Icons landed under the name the .desktop entry references.
for f in "16x16/apps/$APP_ID.png" "256x256/apps/$APP_ID.png"; do
  [[ -f "$XDG_DATA_HOME/icons/hicolor/$f" ]] || fail "icon missing: $f"
done

# 6. Idempotent: a second launch must not rewrite anything (inode unchanged).
before="$(stat -c %i "$DESKTOP")"
out="$("$APPDIR/AppRun")" || fail "second launch exited non-zero"
[[ "$out" = "stub: " ]] || fail "second launch output: $out"
after="$(stat -c %i "$DESKTOP")"
[[ "$before" = "$after" ]] || fail "desktop entry rewritten although unchanged"

# 7. Moving the AppImage refreshes Exec.
mv "$APPIMAGE" "$TMP/moved.AppImage"
export APPIMAGE="$TMP/moved.AppImage"
"$APPDIR/AppRun" >/dev/null || fail "launch after move exited non-zero"
grep -Fxq "Exec=\"$(readlink -f "$APPIMAGE")\" %F" "$DESKTOP" || fail "Exec not refreshed after move"

# 8. External AppImage integrator: if another .desktop file in $XDG_DATA_HOME/applications
#    already launches this AppImage (e.g. AppImageLauncher's appimagekit-*.desktop), AppRun removes
#    its own duplicate .desktop entry while keeping the hicolor icons installed.
ext_desktop="$XDG_DATA_HOME/applications/appimagekit-deckcraft.desktop"
printf '[Desktop Entry]\nType=Application\nName=DeckCraft\nExec="%s" %%F\nIcon=%s\n' "$(readlink -f "$APPIMAGE")" "$APP_ID" >"$ext_desktop"
"$APPDIR/AppRun" >/dev/null || fail "launch with external integrator desktop entry exited non-zero"
[[ ! -e "$DESKTOP" ]] || fail "duplicate desktop entry kept alongside external integrator entry"
[[ -f "$ext_desktop" ]] || fail "external integrator desktop entry removed"
[[ -f "$XDG_DATA_HOME/icons/hicolor/256x256/apps/$APP_ID.png" ]] || fail "icons missing with external integrator"
rm -f "$ext_desktop"

# 9. AppImageLauncher exports DESKTOPINTEGRATION even after X-AppImage-Integrate=false
#    made it skip the AppImage. That is not an existing launcher, so AppRun still
#    writes its own entry and a later launch must not delete it.
rm -rf "$XDG_DATA_HOME"
DESKTOPINTEGRATION=AppImageLauncher "$APPDIR/AppRun" >/dev/null || fail "DESKTOPINTEGRATION launch exited non-zero"
[[ -f "$DESKTOP" ]] || fail "DESKTOPINTEGRATION suppressed the desktop entry"
grep -Fxq "Exec=\"$(readlink -f "$APPIMAGE")\" %F" "$DESKTOP" || fail "Exec missing under DESKTOPINTEGRATION"
grep -Fxq "X-AppImage-Integrate=false" "$DESKTOP" || fail "X-AppImage-Integrate=false missing under DESKTOPINTEGRATION"
[[ -f "$XDG_DATA_HOME/icons/hicolor/256x256/apps/$APP_ID.png" ]] || fail "icons not installed under DESKTOPINTEGRATION"
before="$(stat -c %i "$DESKTOP")"
DESKTOPINTEGRATION=AppImageLauncher "$APPDIR/AppRun" >/dev/null || fail "second DESKTOPINTEGRATION launch exited non-zero"
[[ -f "$DESKTOP" ]] || fail "DESKTOPINTEGRATION removed the desktop entry on a later launch"
[[ "$before" = "$(stat -c %i "$DESKTOP")" ]] || fail "desktop entry rewritten under DESKTOPINTEGRATION although unchanged"

# 10. Opt-out installs nothing.
rm -rf "$XDG_DATA_HOME"
DECKCRAFT_NO_DESKTOP_INTEGRATION=1 "$APPDIR/AppRun" >/dev/null || fail "opt-out launch exited non-zero"
[[ ! -e "$XDG_DATA_HOME" ]] || fail "opt-out still integrated"

# 11. An unwritable data dir never blocks the launch.
mkdir -p "$TMP/ro" && chmod a-w "$TMP/ro"
out="$(XDG_DATA_HOME="$TMP/ro/data" "$APPDIR/AppRun" out.deckcraft)" || fail "launch blocked by unwritable data dir"
[[ "$out" = "stub: out.deckcraft" ]] || fail "stub not run against unwritable data dir: $out"
chmod u+w "$TMP/ro"

# 12. No $APPIMAGE (extracted AppDir): launch only, nothing installed.
rm -rf "$XDG_DATA_HOME"
env -u APPIMAGE "$APPDIR/AppRun" >/dev/null || fail "extracted-AppDir launch exited non-zero"
[[ ! -e "$XDG_DATA_HOME" ]] || fail "integrated without $APPIMAGE"

# 13. A path that would need Exec escaping (here a `$`): launch only, nothing installed.
rm -rf "$XDG_DATA_HOME"
odd="$TMP/odd\$(id).AppImage"
: >"$odd"
out="$(APPIMAGE="$odd" "$APPDIR/AppRun" x)" || fail "launch with an odd path exited non-zero"
[[ "$out" = "stub: x" ]] || fail "odd path: $out"
[[ ! -e "$XDG_DATA_HOME" ]] || fail "integrated an AppImage path that needs escaping"

# 14. AM / AppMan: a deckcraft-AM.desktop launcher in the user's applications
#     dir owns the menu entry. AppRun removes its own entry left from an earlier run,
#     writes no second one and still installs the icons.
rm -rf "$XDG_DATA_HOME"
"$APPDIR/AppRun" >/dev/null || fail "launch before AM install exited non-zero"
[[ -f "$DESKTOP" ]] || fail "no desktop entry before AM install"
am_desktop="$XDG_DATA_HOME/applications/deckcraft-AM.desktop"
printf '[Desktop Entry]
Type=Application
Name=DeckCraft
Exec=deckcraft %%F
Icon=%s
' "$APP_ID" >"$am_desktop"
"$APPDIR/AppRun" >/dev/null || fail "launch with AM launcher exited non-zero"
[[ ! -e "$DESKTOP" ]] || fail "duplicate desktop entry kept alongside AM launcher"
[[ -f "$am_desktop" ]] || fail "AM launcher was removed"
[[ -f "$XDG_DATA_HOME/icons/hicolor/256x256/apps/$APP_ID.png" ]] || fail "icons missing with AM launcher"

# 15. Once AM removes its launcher, AppRun writes its own entry again.
rm -f "$am_desktop"
"$APPDIR/AppRun" >/dev/null || fail "launch after AM removal exited non-zero"
[[ -f "$DESKTOP" ]] || fail "no desktop entry after the AM launcher was removed"

# 16. Custom unmarked desktop entry is preserved and not overwritten or deleted.
printf '[Desktop Entry]\nType=Application\nName=MyCustomDeckCraft\nExec=/custom/deckcraft %%F\nIcon=%s\n' "$APP_ID" >"$DESKTOP"
"$APPDIR/AppRun" >/dev/null || fail "launch with custom entry exited non-zero"
grep -Fxq "Name=MyCustomDeckCraft" "$DESKTOP" || fail "custom entry was overwritten"
# Even when an external integrator is detected, custom entry must not be deleted:
ext_desktop="$XDG_DATA_HOME/applications/appimagekit-deckcraft.desktop"
printf '[Desktop Entry]\nType=Application\nName=DeckCraft\nExec="%s" %%F\nIcon=%s\n' "$(readlink -f "$APPIMAGE")" "$APP_ID" >"$ext_desktop"
"$APPDIR/AppRun" >/dev/null || fail "launch with custom entry and external integrator exited non-zero"
[[ -f "$DESKTOP" ]] || fail "custom entry was deleted by external integrator deduplication"
grep -Fxq "Name=MyCustomDeckCraft" "$DESKTOP" || fail "custom entry modified"
rm -f "$ext_desktop" "$DESKTOP"

echo "AppRun test ok"
