#!/usr/bin/env bash
# Validate the staged Arch launcher without building or opening DeckCraft.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/source"
dst="$work/dist"
base=deckcraft-0.0.1-nightly.1-linux-x86_64
mkdir -p "$stage/bin" "$dst" "$work/extract"

# A local argv recorder stands in for the compiled binaries. It never opens a UI.
cat >"$stage/bin/deckcraft" <<'RECORDER'
#!/bin/sh
printf '<%s>\n' "$@"
RECORDER
chmod 755 "$stage/bin/deckcraft"
cp "$stage/bin/deckcraft" "$stage/bin/deckcraft-cli"

bash "$HERE/nightly-arch-tar.sh" "$stage" "$work/stage" "$dst" "$base"
tar -xzf "$dst/$base.tar.gz" -C "$work/extract"
bin="$work/extract/$base/bin"
# Simulate the existing Arch bin-tree copy and public launcher rename.
mkdir -p "$work/installed"
cp -R "$bin" "$work/installed/bin"
bin="$work/installed/bin"
mv "$bin/deckcraft" "$bin/deckcraft-nightly"
mv "$bin/deckcraft-cli" "$bin/deckcraft-nightly-cli"

[[ ! -e "$stage/bin/.deckcraft-nightly" ]]
cmp "$stage/bin/deckcraft" "$bin/.deckcraft-nightly/deckcraft"
cmp "$stage/bin/deckcraft-cli" "$bin/deckcraft-nightly-cli"
export HOME="$work/home"
mkdir -p "$HOME"
export XDG_CONFIG_HOME="$HOME/.config" XDG_DATA_HOME="$HOME/.local/share"
expect() {
  expected="$1"
  shift
  actual=$(PATH="$bin:$PATH" deckcraft-nightly "$@")
  if [[ "$actual" != "$expected" ]]; then
    printf 'expected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
    exit 1
  fi
}
# The launcher appends deckcraft-nightly to XDG_DATA_HOME before choosing the profile.
profile="$XDG_DATA_HOME/deckcraft-nightly/launcher/profiles/deckcraft"
expect "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$profile" -- 'deck with spaces.pptx' '--other-name.deckcraft')" \
  'deck with spaces.pptx' '--other-name.deckcraft'
# A bare version query stays a version query, not a file name.
expect '<--version>' --version
expect '<-V>' -V

echo "PASS: Arch Nightly handoff stages the hidden binary and preserves launch arguments"
