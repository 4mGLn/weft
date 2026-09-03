#!/bin/sh
set -eu

prefix=${PREFIX:-"$HOME/.local"}
destination="$prefix/bin/weft"

rm -f "$destination"
printf '%s\n' "removed $destination"
printf '%s\n' "Weft state directories are retained; remove an explicit state directory separately if intended."
