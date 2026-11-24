#!/bin/sh
set -eu

real_bin="$(dirname "$0")/.deckcraft-nightly/deckcraft"
exec "$real_bin" "$@"
