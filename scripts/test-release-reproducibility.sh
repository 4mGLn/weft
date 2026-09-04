#!/bin/sh
# Prove deterministic release outputs from independent target directories.
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: $0 VERSION" >&2
    exit 2
fi

version=$1
case "$version" in
    v[0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "VERSION must be a v-prefixed semantic version" >&2; exit 2 ;;
esac

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
target=x86_64-unknown-linux-gnu
package="weft-${version#v}-${target}"

build_archive() {
    name=$1
    output="$work/$name/dist"
    CARGO_TARGET_DIR="$work/$name/target" "$root/scripts/package-release.sh" "$version" "$output" >/dev/null
}

build_archive first
build_archive second

for suffix in tar.gz tar.gz.sha256 cdx.json cdx.json.sha256; do
    cmp "$work/first/dist/$package.$suffix" "$work/second/dist/$package.$suffix" \
        || { echo "release output differs between independent builds: $package.$suffix" >&2; exit 1; }
done

printf '%s\n' "release archive reproducibility test passed"
