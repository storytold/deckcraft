#!/bin/sh
set -eu

: "${XDG_CONFIG_HOME:=${HOME:?HOME must be set}/.config}"
: "${XDG_DATA_HOME:=$HOME/.local/share}"
export XDG_CONFIG_HOME="$XDG_CONFIG_HOME/deckcraft-nightly"
export XDG_DATA_HOME="$XDG_DATA_HOME/deckcraft-nightly"

real_bin="$(dirname "$0")/.deckcraft-nightly/deckcraft"
exec "$real_bin" "$@"
