#!/bin/sh
# Download a verified supported runtime release, then invoke its installer.
set -eu

repository=${WEFT_REPOSITORY:-4mGLn/weft}
version=${WEFT_VERSION:-}
prefix=${PREFIX:-"$HOME/.local"}

command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "python3 is required to read GitHub release metadata" >&2; exit 1; }
command -v tar >/dev/null 2>&1 || { echo "tar is required" >&2; exit 1; }
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || { echo "sha256sum or shasum is required" >&2; exit 1; }

case "$(uname -s)" in
    Linux) target=x86_64-unknown-linux-musl ;;
    Darwin)
        case "$(uname -m)" in
            arm64) target=aarch64-apple-darwin ;;
            x86_64) target=x86_64-apple-darwin ;;
            *) echo "unsupported macOS architecture: $(uname -m)" >&2; exit 1 ;;
        esac ;;
    *) echo "unsupported platform; see USAGE.md for Windows source installation" >&2; exit 1 ;;
esac

api="https://api.github.com/repos/$repository/releases/latest"
if [ -n "$version" ]; then api="https://api.github.com/repos/$repository/releases/tags/$version"; fi
metadata=$(curl --proto '=https' --tlsv1.2 -fsSL "$api")
asset=$(printf '%s' "$metadata" | python3 -c '
import json, sys
r=json.load(sys.stdin); target=sys.argv[1]; version=r["tag_name"].removeprefix("v")
name=f"weft-{version}-{target}.tar.gz"
for item in r.get("assets", []):
    if item.get("name") == name and str(item.get("digest", "")).startswith("sha256:"):
        print(item["browser_download_url"]); print(item["digest"].split(":", 1)[1]); break
else: raise SystemExit(f"release {r.get('tag_name')} has no verified asset {name}")
' "$target")
url=$(printf '%s\n' "$asset" | sed -n '1p')
expected=$(printf '%s\n' "$asset" | sed -n '2p')
test -n "$url" && test -n "$expected"

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT HUP INT TERM
archive="$root/runtime.tar.gz"
curl --proto '=https' --tlsv1.2 -fsSL "$url" -o "$archive"
if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$archive" | awk '{print $1}'); else actual=$(shasum -a 256 "$archive" | awk '{print $1}'); fi
test "$actual" = "$expected" || { echo "GitHub asset digest mismatch" >&2; exit 1; }
tar -xzf "$archive" -C "$root"
package=$(find "$root" -mindepth 1 -maxdepth 1 -type d -name 'weft-*' -print)
test "$(printf '%s\n' "$package" | wc -l)" -eq 1
PREFIX="$prefix" "$package/install.sh"
