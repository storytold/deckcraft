#!/usr/bin/env bash
# Validate both Nightly Linux launchers without building or opening DeckCraft.
#
# The flat nightly tar's `deckcraft` is the canonical shared template
# (`packaging/linux/nightly-launch.sh`, rendered from craft-common-actions's
# `scripts/nightly-launcher.sh.in`), which the nightly producer accepts byte for byte: it starts
# `deckcraft-bin` beside itself on the nightly-v1 profile under the Nightly package identity.
#
# The distro formats keep `packaging/linux/nightly-package-launch.sh` and their own profile folder:
# the AppImage (`AppRun` + `usr/bin/deckcraft-bin`), the distro tree (`bin/deckcraft-nightly` +
# `bin/deckcraft-bin`) and the /usr/bin rule that reaches the fixed /usr/lib copy. A recorder binary
# stands in for the GUI and prints its argv.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
launcher="$HERE/nightly-launch.sh"
package_launcher="$HERE/nightly-package-launch.sh"
export HOME="$work/home"
mkdir -p "$HOME"
export XDG_CONFIG_HOME="$HOME/.config" XDG_DATA_HOME="$HOME/.local/share"
identity=ai.storyteller.deckcraft.nightly
tar_profile="$XDG_DATA_HOME/$identity"
profile="$XDG_DATA_HOME/deckcraft-nightly/launcher/profiles/deckcraft"

recorder() {
  mkdir -p "$(dirname "$1")"
  cat >"$1" <<'RECORDER'
#!/bin/sh
printf '<%s>\n' "$@"
RECORDER
  chmod 755 "$1"
}

expect() {
  binary="$1"
  expected="$2"
  shift 2
  actual=$("$binary" "$@")
  if [[ "$actual" != "$expected" ]]; then
    printf '%s\nexpected:\n%s\nactual:\n%s\n' "$binary" "$expected" "$actual" >&2
    exit 1
  fi
}
nightly_argv() {
  printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$profile" --
}
tar_argv() {
  printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$tar_profile" --
}

# ---- the flat nightly tar: the canonical template `deckcraft`, `deckcraft-bin` GUI ----
flat="$work/flat"
recorder "$flat/deckcraft-bin"
cp "$launcher" "$flat/deckcraft"
chmod 755 "$flat/deckcraft"
expect "$flat/deckcraft" "$(tar_argv)"
expect "$flat/deckcraft" "$(tar_argv; printf '<%s>\n' 'deck with spaces.pptx' '--other-name.pptx')" \
  'deck with spaces.pptx' '--other-name.pptx'
# Both version spellings reach the GUI as one plain `--version`.
expect "$flat/deckcraft" "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$tar_profile" --version)" --version
expect "$flat/deckcraft" "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$tar_profile" --version)" -V
# The ArtCraft Launcher starts the entry point with its own profile pair; the template hands that
# line to the GUI unchanged.
launcher_line=(--launch-profile nightly-v1 --profile-root "$work/launcher root" -- deck.pptx)
expect "$flat/deckcraft" "$(printf '<%s>\n' "${launcher_line[@]}")" "${launcher_line[@]}"
# Anything else is a file argument with the template's own profile, including a query the template
# does not answer (`--help`) and a profile line that does not open the arguments.
expect "$flat/deckcraft" "$(tar_argv; printf '<%s>\n' --help)" --help
expect "$flat/deckcraft" "$(tar_argv; printf '<%s>\n' --profile-root "$work/launcher" --)" --profile-root "$work/launcher" --
test ! -e "$tar_profile"
# The launcher resolves its own folder physically and starts only the GUI beside it.
linked="$work/linked/bin"
mkdir -p "$(dirname "$linked")"
ln -s "$flat" "$linked"
expect "$linked/deckcraft" "$(tar_argv; printf '<%s>\n' book.pptx)" book.pptx
mkdir -p "$work/flat-empty"
cp "$launcher" "$work/flat-empty/deckcraft"
chmod 755 "$work/flat-empty/deckcraft"
status=0
"$work/flat-empty/deckcraft" book.pptx >/dev/null 2>&1 || status=$?
[[ "$status" -ne 0 ]] || { echo 'the tar launcher started without its GUI beside it' >&2; exit 1; }
status=0
env -u XDG_DATA_HOME -u HOME "$flat/deckcraft" book.pptx >/dev/null 2>&1 || status=$?
[[ "$status" -ne 0 ]] || { echo 'the tar launcher started without a data folder' >&2; exit 1; }

# ---- the AppImage: AppRun at the mount root, the GUI and the entry's copy in usr/bin ----
appdir="$work/DeckCraft.AppDir"
recorder "$appdir/usr/bin/deckcraft-bin"
cp "$package_launcher" "$appdir/AppRun"
chmod 755 "$appdir/AppRun"
expect "$appdir/AppRun" "$(nightly_argv; printf '<%s>\n' book.pptx)" book.pptx
cp "$package_launcher" "$appdir/usr/bin/deckcraft-nightly"
chmod 755 "$appdir/usr/bin/deckcraft-nightly"
expect "$appdir/usr/bin/deckcraft-nightly" "$(nightly_argv; printf '<%s>\n' book.pptx)" book.pptx
test ! -e "$profile"

# ---- the distro tree tarball: bin/deckcraft-nightly beside bin/deckcraft-bin ----
tree="$work/tree/bin"
recorder "$tree/deckcraft-bin"
cp "$package_launcher" "$tree/deckcraft-nightly"
chmod 755 "$tree/deckcraft-nightly"
actual=$(PATH="$tree:$PATH" deckcraft-nightly book.pptx)
[[ "$actual" == "$(nightly_argv; printf '<%s>\n' book.pptx)" ]] || { printf 'a PATH invocation printed:\n%s\n' "$actual" >&2; exit 1; }
test ! -e "$profile"

# ---- a missing GUI fails closed, and so does a tree with no data folder ----
empty="$work/empty"
mkdir -p "$empty"
cp "$package_launcher" "$empty/deckcraft"
chmod 755 "$empty/deckcraft"
status=0
"$empty/deckcraft" book.pptx >/dev/null 2>&1 || status=$?
[[ "$status" -eq 2 ]] || { printf 'a launcher with no GUI must exit 2, got %s\n' "$status" >&2; exit 1; }
recorder "$work/nodata/deckcraft-bin"
cp "$package_launcher" "$work/nodata/deckcraft"
chmod 755 "$work/nodata/deckcraft"
status=0
env -u XDG_DATA_HOME -u HOME "$work/nodata/deckcraft" book.pptx >/dev/null 2>&1 || status=$?
[[ "$status" -eq 2 ]] || { printf 'a launcher with no data folder must exit 2, got %s\n' "$status" >&2; exit 1; }
# A query is not a launch: it answers without a data folder instead of becoming that error.
actual=$(env -u XDG_DATA_HOME -u HOME "$work/nodata/deckcraft" --version) \
  || { printf 'a no-data-folder --version must still answer\n' >&2; exit 1; }
[[ "$actual" == '<--version>' ]] || { printf 'a no-data-folder --version printed:\n%s\n' "$actual" >&2; exit 1; }

# ---- the /usr/bin rule stays with the distro launcher: the fixed /usr/lib copy, never a parent path ----
# The launcher's text is searched literally, so `$here` must not expand here.
# shellcheck disable=SC2016
grep -Fq '[ "$here" = /usr/bin ]' "$package_launcher"
grep -Fq '/usr/lib/deckcraft-nightly/deckcraft' "$package_launcher"
if grep -q '\.\.' "$package_launcher"; then
  echo "the distro launcher must not reach outside its own folder or the fixed /usr/lib path" >&2
  exit 1
fi
# ---- the tar launcher is the shared template, unedited ----
grep -Fq '# Nightly launcher generated by craft-common-actions scripts/nightly-launcher.sh.in.' "$launcher"
# shellcheck disable=SC2016
grep -Fq 'target="$dir/deckcraft-bin"' "$launcher"
# shellcheck disable=SC2016
grep -Fq '  --launch-profile) exec "$target" "$@" ;;' "$launcher"
grep -Fq "}/$identity\"" "$launcher"

echo "PASS: the nightly tar's template launcher and the distro launcher start the GUI with their profile"
