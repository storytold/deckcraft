#!/bin/sh
# The DeckCraft Nightly launcher every Linux format ships: the plain nightly tar's `deckcraft`, the
# distro tree's `bin/deckcraft-nightly`, the AppImage's `AppRun`, the Arch handoff's `bin/deckcraft`
# and the Flatpak's `/app/bin/deckcraft-nightly`. A nightly build refuses to start without
# `--launch-profile nightly-v1 --profile-root <dir>`, and the profile is its only data root, so
# this is what keeps a Nightly run out of the stable app's settings, prefs, logs and recovery.
set -eu

# Resolve this script's own folder from `$0`. A shell that finds it through PATH passes a bare
# name, so ask the shell where that is instead of using the working directory.
case "$0" in
  */*) self="$0" ;;
  *) self="$(command -v "$0" || printf '%s' "$0")" ;;
esac
case "$self" in
  /*) ;;
  *) self="$PWD/$self" ;;
esac
here="$(CDPATH='' cd "$(dirname "$self")" && pwd -P)"
self="$here/$(basename "$self")"

# The GUI each placement keeps inside its own tree, never from a parent folder: `deckcraft-bin`
# beside the launcher (the flat nightly tar, the distro tree, the Flatpak sandbox), the same name
# in an AppImage's `usr/bin`, and the hidden copy the Arch handoff keeps. A candidate that is this
# same file is never started, so a launcher cannot recurse into itself.
real=
for candidate in "$here/deckcraft-bin" "$here/usr/bin/deckcraft-bin" "$here/.deckcraft-nightly/deckcraft-bin"; do
  if [ -f "$candidate" ] && [ -x "$candidate" ] && [ ! "$candidate" -ef "$self" ]; then
    real="$candidate"
    break
  fi
done
# A distro package keeps the GUI in /usr/lib. Reach that fixed path only from /usr/bin, so a
# writable parent folder can never plant a program for this launcher to run.
if [ -z "$real" ] && [ "$here" = /usr/bin ] && [ -f /usr/lib/deckcraft-nightly/deckcraft ] && [ -x /usr/lib/deckcraft-nightly/deckcraft ]; then
  real=/usr/lib/deckcraft-nightly/deckcraft
fi
if [ -z "$real" ]; then
  echo "deckcraft-nightly: no DeckCraft binary beside $self" >&2
  exit 2
fi

if [ -z "${XDG_DATA_HOME:-}" ] && [ -z "${HOME:-}" ]; then
  echo "deckcraft-nightly: XDG_DATA_HOME and HOME are both unset, so there is no Nightly data folder" >&2
  exit 2
fi
# The same root as every other format's launcher: beside the fenced stable `deckcraft` folder,
# never inside it.
profile="${XDG_DATA_HOME:-$HOME/.local/share}/deckcraft-nightly/launcher/profiles/deckcraft"

# A bare version/help query and the Launcher's own protocol pass straight through: they are not
# file names, and a profile the caller already supplied is never wrapped twice.
case "${1-}" in
  --launch-profile | --profile-root | --version | -V | --help | -h) exec "$real" "$@" ;;
esac
# A caller-written `--` is the separator this launcher adds below. Drop it, so it can never reach
# the app as a file name.
if [ "${1-}" = "--" ]; then
  shift
fi
exec "$real" --launch-profile nightly-v1 --profile-root "$profile" -- "$@"
