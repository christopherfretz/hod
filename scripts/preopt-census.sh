#!/usr/bin/env bash
# WP-021: first refusals of hod --strict on the pre-opt modules that
# scripts/preopt-corpus.sh wrote, next to the post-opt corpus entry of the
# same test.
#
# usage: scripts/preopt-census.sh <pre-opt dir> <hod> [-j N]
# Prints one line per module: entry, post-opt status, pre-opt status, the
# pre-opt first reason; then the summary table.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
main="$(cd "$(git -C "$here" rev-parse --path-format=absolute --git-common-dir)/.." && pwd)"
case "$main" in */.git/modules*) main="${main%%/.git/modules*}";; esac
corpus="${HOD_BUILD_ROOT:-$main/.build}/corpus"
pre="${1:?usage: preopt-census.sh <pre-opt dir> <hod> [-j N]}"
hod="${2:?usage: preopt-census.sh <pre-opt dir> <hod> [-j N]}"
jobs=4
if [ "${3:-}" = -j ]; then jobs="${4:?-j needs a number}"; fi
[ "$jobs" -le 4 ] || { echo "preopt-census.sh: -j is at most 4" >&2; exit 2; }
export corpus pre hod
one() {
  local rel="${1#"$pre"/}" s1 s2 why
  "$hod" --strict -O0 -relocation-model=pic -filetype=obj -o /dev/null "$corpus/$rel" 2>/dev/null && s1=0 || s1=$?
  why="$("$hod" --strict -O0 -relocation-model=pic -filetype=obj -o /dev/null "$1" 2>&1 >/dev/null)" && s2=0 || s2=$?
  why="$(printf '%s' "$why" | sed -n "s/.*construct '\([^']*\)'.*/\1/p;s/.*malformed input.*/malformed-input/p;s/.*internal error.*/internal-error/p" | head -1)"
  printf '%s\t%s\t%s\t%s\n' "$rel" "$s1" "$s2" "${why:--}"
}
export -f one
find "$pre" -name '*.ll' -print0 | sort -z | xargs -0 -P "$jobs" -I{} bash -c 'one "$1"' _ {} | sort
