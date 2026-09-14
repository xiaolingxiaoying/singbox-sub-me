#!/bin/sh
# Install the sbtui client and its `ly` shortcut binary (the client-side
# counterpart of the server-side sbctl `ly`). Put the binaries next to this
# script, or point SBTUI_DIR at the directory that holds them.
set -eu
dir=${SBTUI_DIR:-$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)}
prefix=${PREFIX:-/usr/local}

install -d "$prefix/bin"
installed=0
for name in sbtui ly; do
  if [ -x "$dir/$name" ]; then
    install -m 0755 "$dir/$name" "$prefix/bin/$name"
    installed=$((installed + 1))
  fi
done

# Fall back to a symlink when only the sbtui binary was shipped.
if [ -x "$prefix/bin/sbtui" ] && [ ! -e "$prefix/bin/ly" ]; then
  ln -sf "$prefix/bin/sbtui" "$prefix/bin/ly"
fi

if [ "$installed" -eq 0 ] && [ ! -x "$prefix/bin/sbtui" ]; then
  echo "no sbtui/ly binary found next to install.sh; set SBTUI_DIR" >&2
  exit 2
fi

echo "installed: $prefix/bin/sbtui"
echo "shortcut:  $prefix/bin/ly"
"$prefix/bin/ly" --version
