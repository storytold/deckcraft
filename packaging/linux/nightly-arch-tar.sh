#!/usr/bin/env bash
# Stage the DeckCraft Nightly-only handoff for the Arch packager.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
source_tree="$1"
work="$2"
destination="$3"
basename="$4"
stage="$work/$basename"

mkdir -p "$work"
cp -R "$source_tree" "$stage"
mkdir "$stage/bin/.deckcraft-nightly"
mv "$stage/bin/deckcraft" "$stage/bin/.deckcraft-nightly/deckcraft-bin"
cp "$here/nightly-launch.sh" "$stage/bin/deckcraft"
chmod 755 "$stage/bin/deckcraft"
tar -C "$work" -czf "$destination/$basename.tar.gz" "$basename"
echo "wrote $destination/$basename.tar.gz"
