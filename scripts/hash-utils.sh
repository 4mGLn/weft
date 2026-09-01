#!/bin/sh

sha256_digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

write_manifest() {
    root=$1
    output=$2
    (
        cd "$root"
        find . -type f ! -name MANIFEST.sha256 -print | sort | while IFS= read -r path; do
            printf '%s  %s\n' "$(sha256_digest "$path")" "$path"
        done
    ) > "$output"
}

verify_manifest() {
    root=$1
    manifest=$2
    while IFS='  ' read -r expected path; do
        test -n "$expected" && test -n "$path" || return 1
        actual=$(sha256_digest "$root/${path#./}") || return 1
        test "$actual" = "$expected" || return 1
    done < "$manifest"
}

verify_checksum() {
    checksum=$1
    artifact=$2
    expected=$(awk 'NR == 1 { print $1 } END { if (NR != 1) exit 1 }' "$checksum") || return 1
    test "$expected" = "$(sha256_digest "$artifact")"
}
