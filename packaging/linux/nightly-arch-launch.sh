#!/bin/sh
# DeckCraft Nightly launcher for the Arch handoff tar. The launch profile is the data root the
# app accepts; the XDG exports keep a stable build (and the CLI's shared state) beside it.
set -eu

: "${XDG_CONFIG_HOME:=${HOME:?HOME must be set}/.config}"
: "${XDG_DATA_HOME:=$HOME/.local/share}"
export XDG_CONFIG_HOME="$XDG_CONFIG_HOME/deckcraft-nightly"
export XDG_DATA_HOME="$XDG_DATA_HOME/deckcraft-nightly"

real_bin="$(dirname "$(readlink -f "$0")")/.deckcraft-nightly/deckcraft"
case "${1-}" in
  --version | -V) exec "$real_bin" "$@" ;;
esac
exec "$real_bin" --launch-profile nightly-v1 \
  --profile-root "$XDG_DATA_HOME/launcher/profiles/deckcraft" -- "$@"
