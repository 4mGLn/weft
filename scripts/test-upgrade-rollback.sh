#!/bin/sh
# Prove a supported archive-to-archive upgrade and rollback in an isolated prefix.
set -eu

if [ "$#" -ne 2 ]; then
    echo "usage: $0 FROM_ARCHIVE TO_ARCHIVE" >&2
    exit 2
fi

from_archive=$1
to_archive=$2

fail() { printf 'upgrade/rollback smoke test failed: %s\n' "$*" >&2; exit 1; }

verify_archive() {
    archive=$1
    checksum="$archive.sha256"
    sbom="${archive%.tar.gz}.cdx.json"
    test -f "$archive" || fail "archive is missing: $archive"
    test -f "$checksum" || fail "archive checksum is missing: $checksum"
    test -f "$sbom" || fail "SBOM is missing: $sbom"
    test -f "$sbom.sha256" || fail "SBOM checksum is missing: $sbom.sha256"
    archive_digest=$(awk 'NR == 1 { print $1 } END { if (NR != 1) exit 1 }' "$checksum") \
        || fail "archive checksum is malformed: $checksum"
    sbom_digest=$(awk 'NR == 1 { print $1 } END { if (NR != 1) exit 1 }' "$sbom.sha256") \
        || fail "SBOM checksum is malformed: $sbom.sha256"
    test "$archive_digest" = "$(sha256sum "$archive" | awk '{print $1}')" \
        || fail "archive checksum does not match: $archive"
    test "$sbom_digest" = "$(sha256sum "$sbom" | awk '{print $1}')" \
        || fail "SBOM checksum does not match: $sbom"
    python3 -m json.tool "$sbom" >/dev/null
}

extract_package() {
    archive=$1
    destination=$2
    mkdir -p "$destination"
    tar -xzf "$archive" -C "$destination"
    package_dir=$(find "$destination" -mindepth 1 -maxdepth 1 -type d -name 'weft-*' -print)
    test -n "$package_dir" || fail "archive has no runtime package: $archive"
    test "$(printf '%s\n' "$package_dir" | wc -l)" -eq 1 || fail "archive has multiple runtime packages: $archive"
    printf '%s\n' "$package_dir"
}

verify_archive "$from_archive"
verify_archive "$to_archive"

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT HUP INT TERM
from_package=$(extract_package "$from_archive" "$root/from")
to_package=$(extract_package "$to_archive" "$root/to")
from_version=$("$from_package/bin/weft" --version)
to_version=$("$to_package/bin/weft" --version)
test "$from_version" != "$to_version" || fail "archives must contain distinct runtime versions"

prefix="$root/install"
state="$root/state"
backup="$root/state-before-upgrade"
candidate_state="$root/state-after-candidate"

PREFIX="$prefix" "$from_package/install.sh" >/dev/null
test "$("$prefix/bin/weft" --version)" = "$from_version" || fail "previous runtime was not installed"
"$prefix/bin/weft" --format json --state-dir "$state" init >/dev/null
"$prefix/bin/weft" --format json --state-dir "$state" change create \
    --change-id upgrade-base --operation-id upgrade-base-create \
    --actor release-test --at 1 >/dev/null
"$prefix/bin/weft" --format json --state-dir "$state" change show --change-id upgrade-base >/dev/null
cp -a "$state" "$backup"

PREFIX="$prefix" "$to_package/install.sh" >/dev/null
test "$("$prefix/bin/weft" --version)" = "$to_version" || fail "candidate runtime was not installed"
"$prefix/bin/weft" --format json --state-dir "$state" change show --change-id upgrade-base >/dev/null
"$prefix/bin/weft" --format json --state-dir "$state" change create \
    --change-id upgrade-candidate --operation-id upgrade-candidate-create \
    --actor release-test --at 2 >/dev/null
"$prefix/bin/weft" --format json --state-dir "$state" change show --change-id upgrade-candidate >/dev/null

PREFIX="$prefix" "$from_package/install.sh" >/dev/null
test "$("$prefix/bin/weft" --version)" = "$from_version" || fail "previous runtime was not restored"
"$prefix/bin/weft" --format json --state-dir "$state" change show --change-id upgrade-base >/dev/null
"$prefix/bin/weft" --format json --state-dir "$state" change show --change-id upgrade-candidate >/dev/null

mv "$state" "$candidate_state"
cp -a "$backup" "$state"
"$prefix/bin/weft" --format json --state-dir "$state" change show --change-id upgrade-base >/dev/null
if "$prefix/bin/weft" --format json --state-dir "$state" change show --change-id upgrade-candidate >/dev/null 2>&1; then
    fail "restored state retained candidate-only data"
fi

PREFIX="$prefix" "$from_package/uninstall.sh" >/dev/null
test ! -e "$prefix/bin/weft" || fail "uninstall did not remove the restored binary"
test -f "$state/metadata.sqlite3" || fail "state was not retained after uninstall"
printf '%s\n' "upgrade/rollback archive smoke test passed"
