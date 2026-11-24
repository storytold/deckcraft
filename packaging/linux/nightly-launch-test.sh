#!/usr/bin/env bash
# Validate the nightly launcher in every Linux placement without building or opening DeckCraft:
# the flat nightly tar (`deckcraft` + `deckcraft-bin`), the AppImage (`AppRun` + `usr/bin/deckcraft`),
# the distro tree (`bin/deckcraft-nightly` + `bin/deckcraft-bin`) and the /usr/bin rule that reaches
# the fixed /usr/lib copy. A recorder binary stands in for the GUI and prints its argv.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
launcher="$HERE/nightly-launch.sh"
export HOME="$work/home"
mkdir -p "$HOME"
export XDG_CONFIG_HOME="$HOME/.config" XDG_DATA_HOME="$HOME/.local/share"
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

# ---- the flat nightly tar: `deckcraft` launcher, `deckcraft-bin` GUI ----
flat="$work/flat"
recorder "$flat/deckcraft-bin"
cp "$launcher" "$flat/deckcraft"
chmod 755 "$flat/deckcraft"
expect "$flat/deckcraft" "$(nightly_argv)"
expect "$flat/deckcraft" "$(nightly_argv; printf '<%s>\n' 'deck with spaces.pptx' '--other-name.pptx')" \
  'deck with spaces.pptx' '--other-name.pptx'
expect "$flat/deckcraft" "$(nightly_argv; printf '<%s>\n' 'deck.pptx')" -- deck.pptx
expect "$flat/deckcraft" '<--version>' --version
expect "$flat/deckcraft" '<-V>' -V
expect "$flat/deckcraft" '<--help>' --help
expect "$flat/deckcraft" "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$work/launcher" -- 'deck.pptx')" \
  --launch-profile nightly-v1 --profile-root "$work/launcher" -- 'deck.pptx'
test ! -e "$profile"

# ---- the AppImage: AppRun at the mount root, the GUI and the entry's copy in usr/bin ----
appdir="$work/DeckCraft.AppDir"
recorder "$appdir/usr/bin/deckcraft-bin"
cp "$launcher" "$appdir/AppRun"
chmod 755 "$appdir/AppRun"
expect "$appdir/AppRun" "$(nightly_argv; printf '<%s>\n' book.pptx)" book.pptx
cp "$launcher" "$appdir/usr/bin/deckcraft-nightly"
chmod 755 "$appdir/usr/bin/deckcraft-nightly"
expect "$appdir/usr/bin/deckcraft-nightly" "$(nightly_argv; printf '<%s>\n' book.pptx)" book.pptx
test ! -e "$profile"

# ---- the distro tree tarball: bin/deckcraft-nightly beside bin/deckcraft-bin ----
tree="$work/tree/bin"
recorder "$tree/deckcraft-bin"
cp "$launcher" "$tree/deckcraft-nightly"
chmod 755 "$tree/deckcraft-nightly"
actual=$(PATH="$tree:$PATH" deckcraft-nightly book.pptx)
[[ "$actual" == "$(nightly_argv; printf '<%s>\n' book.pptx)" ]] || { printf 'a PATH invocation printed:\n%s\n' "$actual" >&2; exit 1; }
test ! -e "$profile"

# ---- a missing GUI fails closed, and so does a tree with no data folder ----
empty="$work/empty"
mkdir -p "$empty"
cp "$launcher" "$empty/deckcraft"
chmod 755 "$empty/deckcraft"
status=0
"$empty/deckcraft" book.pptx >/dev/null 2>&1 || status=$?
[[ "$status" -eq 2 ]] || { printf 'a launcher with no GUI must exit 2, got %s\n' "$status" >&2; exit 1; }
recorder "$work/nodata/deckcraft-bin"
cp "$launcher" "$work/nodata/deckcraft"
chmod 755 "$work/nodata/deckcraft"
status=0
env -u XDG_DATA_HOME -u HOME "$work/nodata/deckcraft" book.pptx >/dev/null 2>&1 || status=$?
[[ "$status" -eq 2 ]] || { printf 'a launcher with no data folder must exit 2, got %s\n' "$status" >&2; exit 1; }

# ---- the /usr/bin rule: the fixed /usr/lib copy is named, and no parent path is ever used ----
grep -Fq '[ "$here" = /usr/bin ]' "$launcher"
grep -Fq '/usr/lib/deckcraft-nightly/deckcraft' "$launcher"
if grep -q '\.\.' "$launcher"; then
  echo "the launcher must not reach outside its own folder or the fixed /usr/lib path" >&2
  exit 1
fi

echo "PASS: nightly launcher starts the GUI with the profile in every layout and refuses the rest"
