#!/bin/sh
set -eu

prefix=${PREFIX:-"$HOME/.local"}
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
destination="$prefix/bin/weft"

mkdir -p "$prefix/bin"
temporary="$destination.tmp.$$"
trap 'rm -f "$temporary"' EXIT HUP INT TERM
install -m 0755 "$source_dir/bin/weft" "$temporary"
mv -f "$temporary" "$destination"
trap - EXIT HUP INT TERM
printf '%s\n' "installed $destination"
