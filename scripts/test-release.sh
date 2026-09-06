#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: $0 ARCHIVE" >&2
    exit 2
fi

archive=$1
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$script_dir/hash-utils.sh"
checksum="$archive.sha256"
sbom="${archive%.tar.gz}.cdx.json"
test -f "$archive"
test -f "$checksum"
test -f "$sbom"
test -f "$sbom.sha256"
archive_dir=$(dirname "$archive")
verify_checksum "$checksum" "$archive" || { echo "archive checksum verification failed" >&2; exit 1; }
verify_checksum "$sbom.sha256" "$sbom" || { echo "SBOM checksum verification failed" >&2; exit 1; }
python3 -m json.tool "$sbom" >/dev/null
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT HUP INT TERM
corrupt="$root/corrupt.tar.gz"
cp "$archive" "$corrupt"
printf 'corrupt\n' >> "$corrupt"
corrupt_checksum="$root/corrupt.sha256"
printf '%s  %s\n' "$(cut -d ' ' -f 1 "$checksum")" "$corrupt" > "$corrupt_checksum"
if verify_checksum "$corrupt_checksum" "$corrupt"; then
    echo "checksum verification accepted a corrupted archive" >&2
    exit 1
fi

tar -xzf "$archive" -C "$root"
package_dir=$(find "$root" -mindepth 1 -maxdepth 1 -type d -name 'weft-*' -print)
test -n "$package_dir"
test "$(printf '%s\n' "$package_dir" | wc -l)" -eq 1
test -f "$package_dir/SBOM.cdx.json"
cmp "$sbom" "$package_dir/SBOM.cdx.json"
test -f "$package_dir/README.md"
test -f "$package_dir/GETTING_STARTED.md"
test -f "$package_dir/MANUAL.md"
test -f "$package_dir/USAGE.md"
test -f "$package_dir/LICENSE"
test ! -e "$package_dir/docs"
test ! -e "$package_dir/scripts"
verify_manifest "$package_dir" "$package_dir/MANIFEST.sha256"
actual_files=$(cd "$package_dir" && find . -type f ! -name MANIFEST.sha256 | sort)
listed_files=$(sed 's/^[0-9a-f]*  //' "$package_dir/MANIFEST.sha256" | sort)
test "$actual_files" = "$listed_files"

prefix="$root/install"
PREFIX="$prefix" "$package_dir/install.sh"
"$prefix/bin/weft" --version
"$prefix/bin/weft" --help >/dev/null
state="$root/state"
"$prefix/bin/weft" --format json --state-dir "$state" init >/dev/null
project="$root/project"
mkdir "$project"
printf '%s\n' '# Existing project rules' > "$project/AGENTS.md"
"$prefix/bin/weft" --format json --state-dir "$state" setup \
    --project-dir "$project" --runtime codex,claude-code,gemini-cli,paseo >/dev/null
test -f "$project/.weft/runtime-bridge.json"
grep -q '<!-- weft:runtime-wiring:start -->' "$project/AGENTS.md"
test -f "$project/CLAUDE.md"
test -f "$project/GEMINI.md"
"$prefix/bin/weft" --format json --state-dir "$state" doctor \
    --project-dir "$project" >/dev/null
"$prefix/bin/weft" --format json --state-dir "$state" change create \
    --change-id release-smoke --operation-id release-smoke-create \
    --actor release-test --at 1 >/dev/null
"$prefix/bin/weft" --format json --state-dir "$state" change show \
    --change-id release-smoke >/dev/null
PREFIX="$prefix" "$package_dir/uninstall.sh"
test ! -e "$prefix/bin/weft"
test -f "$state/metadata.sqlite3"
printf '%s\n' "release archive smoke test passed"
