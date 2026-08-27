#!/bin/sh
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "usage: $0 VERSION [OUTPUT_DIRECTORY]" >&2
    exit 2
fi

version=$1
output_dir=${2:-dist}
target=${WEFT_RELEASE_TARGET:-x86_64-unknown-linux-gnu}

if ! printf '%s\n' "$version" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "VERSION must be a v-prefixed semantic version" >&2
    exit 2
fi

case "$target" in
    x86_64-unknown-linux-gnu) ;;
    *) echo "unsupported release target: $target" >&2; exit 2 ;;
esac

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
package="weft-${version#v}-${target}"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM

cd "$root"
cargo build --locked --release --target "$target" -p weft-cli
expected_version="weft ${version#v}"
actual_version=$("target/$target/release/weft" --version)
if [ "$actual_version" != "$expected_version" ]; then
    echo "binary version mismatch: expected $expected_version, got $actual_version" >&2
    exit 1
fi
mkdir -p "$stage/$package/bin" "$stage/$package/docs" "$stage/$package/scripts"
install -m 0755 "target/$target/release/weft" "$stage/$package/bin/weft"
install -m 0755 packaging/install.sh packaging/uninstall.sh "$stage/$package/"
install -m 0644 README.md GOAL.md DOMAIN.md ROADMAP.md AGENTS.md CONTRIBUTING.md SECURITY.md CHANGELOG.md "$stage/$package/"
cp -R docs/. "$stage/$package/docs/"
install -m 0755 scripts/check_docs.py "$stage/$package/scripts/"

mkdir -p "$output_dir"
archive="$output_dir/$package.tar.gz"
sbom="$output_dir/$package.cdx.json"
./scripts/generate-sbom.py "$sbom"
install -m 0644 "$sbom" "$stage/$package/docs/SBOM.cdx.json"
(cd "$stage/$package" && find . -type f ! -name MANIFEST.sha256 -print0 | sort -z | xargs -0 sha256sum > MANIFEST.sha256)
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='UTC 2026-01-01' -C "$stage" -czf "$archive" "$package"
sha256sum "$archive" > "$archive.sha256"
sha256sum "$sbom" > "$sbom.sha256"
printf '%s\n' "$archive"
