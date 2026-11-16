#!/usr/bin/env bash
# Prints the pinned commit of the mortar linker, read from the one place
# the repository writes it: `hod_testutil::linkers::MORTAR_COMMIT` in
# crates/hod_testutil/src/linkers.rs (WP-310; docs/status/linkers.md
# "mortar"). The hosted lanes' mortar action builds and caches that
# commit. Exits 1, printing nothing on stdout, unless exactly one such
# line holds a 40-digit lowercase hexadecimal commit.
#
#   scripts/mortar-commit.sh [<checkout>, default: this script's]
set -u
root=${1:-"$(cd "$(dirname "$0")/.." && pwd)"}
source_file="$root/crates/hod_testutil/src/linkers.rs"
lines=$(grep -E '^pub const MORTAR_COMMIT: &str = "[^"]*";$' "$source_file") || {
    echo "mortar-commit.sh: no MORTAR_COMMIT in $source_file" >&2
    exit 1
}
if [ "$(printf '%s\n' "$lines" | wc -l)" -ne 1 ]; then
    echo "mortar-commit.sh: more than one MORTAR_COMMIT in $source_file" >&2
    exit 1
fi
commit=${lines#*\"}
commit=${commit%\"*}
case "$commit" in
    *[!0-9a-f]* | "")
        echo "mortar-commit.sh: MORTAR_COMMIT '$commit' is not a commit" >&2
        exit 1
        ;;
esac
if [ "${#commit}" -ne 40 ]; then
    echo "mortar-commit.sh: MORTAR_COMMIT '$commit' is not 40 digits" >&2
    exit 1
fi
printf '%s\n' "$commit"
