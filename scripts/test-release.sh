#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: $0 ARCHIVE" >&2
    exit 2
fi

archive=$1
checksum="$archive.sha256"
sbom="${archive%.tar.gz}.cdx.json"
test -f "$archive"
test -f "$checksum"
test -f "$sbom"
test -f "$sbom.sha256"
archive_dir=$(dirname "$archive")
(cd "$archive_dir" && sha256sum -c "$(basename "$checksum")")
(cd "$archive_dir" && sha256sum -c "$(basename "$sbom.sha256")")
python3 -m json.tool "$sbom" >/dev/null
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT HUP INT TERM
corrupt="$root/corrupt.tar.gz"
cp "$archive" "$corrupt"
printf 'corrupt\n' >> "$corrupt"
corrupt_checksum="$root/corrupt.sha256"
printf '%s  %s\n' "$(cut -d ' ' -f 1 "$checksum")" "$corrupt" > "$corrupt_checksum"
if sha256sum -c "$corrupt_checksum" >/dev/null 2>&1; then
    echo "checksum verification accepted a corrupted archive" >&2
    exit 1
fi

tar -xzf "$archive" -C "$root"
package_dir=$(find "$root" -mindepth 1 -maxdepth 1 -type d -name 'weft-*' -print)
test -n "$package_dir"
test "$(printf '%s\n' "$package_dir" | wc -l)" -eq 1
test -f "$package_dir/docs/SBOM.cdx.json"
cmp "$sbom" "$package_dir/docs/SBOM.cdx.json"
(cd "$package_dir" && sha256sum -c MANIFEST.sha256 >/dev/null)
actual_files=$(cd "$package_dir" && find . -type f ! -name MANIFEST.sha256 | sort)
listed_files=$(sed 's/^[0-9a-f]*  //' "$package_dir/MANIFEST.sha256" | sort)
test "$actual_files" = "$listed_files"
python3 "$package_dir/scripts/check_docs.py" "$package_dir" >/dev/null

prefix="$root/install"
PREFIX="$prefix" "$package_dir/install.sh"
"$prefix/bin/weft" --version
"$prefix/bin/weft" --help >/dev/null
state="$root/state"
"$prefix/bin/weft" --format json --state-dir "$state" init >/dev/null
"$prefix/bin/weft" --format json --state-dir "$state" change create \
    --change-id release-smoke --operation-id release-smoke-create \
    --actor release-test --at 1 >/dev/null
"$prefix/bin/weft" --format json --state-dir "$state" change show \
    --change-id release-smoke >/dev/null
PREFIX="$prefix" "$package_dir/uninstall.sh"
test ! -e "$prefix/bin/weft"
test -f "$state/metadata.sqlite3"
printf '%s\n' "release archive smoke test passed"
