#!/bin/sh
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "usage: $0 VERSION [OUTPUT_DIRECTORY]" >&2
    exit 2
fi

version=$1
output_dir=${2:-dist}
target=${WEFT_RELEASE_TARGET:-x86_64-unknown-linux-musl}
build_root=${CARGO_TARGET_DIR:-target}

if ! printf '%s\n' "$version" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "VERSION must be a v-prefixed semantic version" >&2
    exit 2
fi

case "$target" in
    x86_64-unknown-linux-musl|x86_64-apple-darwin|aarch64-apple-darwin|x86_64-pc-windows-msvc) ;;
    *) echo "unsupported release target: $target" >&2; exit 2 ;;
esac

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$script_dir/.." && pwd)
. "$script_dir/hash-utils.sh"
package="weft-${version#v}-${target}"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM

cd "$root"
binary=weft
case "$target" in x86_64-pc-windows-msvc) binary=weft.exe ;; esac
cargo build --locked --release --target "$target" --target-dir "$build_root" -p weft-cli
expected_version="weft ${version#v}"
actual_version=$("$build_root/$target/release/$binary" --version)
if [ "$actual_version" != "$expected_version" ]; then
    echo "binary version mismatch: expected $expected_version, got $actual_version" >&2
    exit 1
fi
mkdir -p "$stage/$package/bin"
install -m 0755 "$build_root/$target/release/$binary" "$stage/$package/bin/$binary"
case "$target" in
    x86_64-pc-windows-msvc)
        install -m 0644 packaging/install.ps1 packaging/uninstall.ps1 "$stage/$package/"
        ;;
    *) install -m 0755 packaging/install.sh packaging/uninstall.sh "$stage/$package/" ;;
esac
install -m 0644 RUNTIME_README.md "$stage/$package/README.md"
install -m 0644 GETTING_STARTED.md MANUAL.md USAGE.md LICENSE "$stage/$package/"

mkdir -p "$output_dir"
archive="$output_dir/$package.tar.gz"
sbom="$output_dir/$package.cdx.json"
./scripts/generate-sbom.py "$sbom"
install -m 0644 "$sbom" "$stage/$package/SBOM.cdx.json"
write_manifest "$stage/$package" "$stage/$package/MANIFEST.sha256"
python3 "$script_dir/create-deterministic-archive.py" "$stage" "$package" "$archive"
archive_name=$(basename "$archive")
sbom_name=$(basename "$sbom")
(
    cd "$output_dir"
    printf '%s  %s\n' "$(sha256_digest "$archive_name")" "$archive_name" > "$archive_name.sha256"
    printf '%s  %s\n' "$(sha256_digest "$sbom_name")" "$sbom_name" > "$sbom_name.sha256"
)
printf '%s\n' "$archive"
