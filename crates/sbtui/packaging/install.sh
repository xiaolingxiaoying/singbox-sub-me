#!/bin/sh
# Install sbtui and expose the `ly` shortcut (the client-side counterpart of
# the server-side sbctl `ly`). Run next to the downloaded binary, or point
# SBTUI_BINARY at it.
set -eu
dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
prefix=${PREFIX:-/usr/local}

binary=${SBTUI_BINARY:-}
if [ -z "$binary" ]; then
  if [ -x "$dir/sbtui" ]; then
    binary="$dir/sbtui"
  else
    binary=$(command -v sbtui || true)
  fi
fi

if [ -z "$binary" ] || [ ! -x "$binary" ]; then
  echo "sbtui binary not found; put it next to install.sh or set SBTUI_BINARY" >&2
  exit 2
fi

install -d "$prefix/bin"
install -m 0755 "$binary" "$prefix/bin/sbtui"
ln -sf "$prefix/bin/sbtui" "$prefix/bin/ly"
echo "installed: $prefix/bin/sbtui"
echo "shortcut:  $prefix/bin/ly"
"$prefix/bin/sbtui" --version
