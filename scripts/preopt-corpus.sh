#!/usr/bin/env bash
# WP-021: regenerates the kiln entries of the IR corpus BEFORE LLVM's -O0
# pipeline (`-Xclang -disable-llvm-passes`), with each entry's directives,
# so that `cargo xtask irdiff` and `hod --strict` can be run over them.
#
# usage: scripts/preopt-corpus.sh <out dir> [-j N] [-g] [--llvm 19|22] [--limit N]
#
# Reads <build root>/corpus/MANIFEST.tsv (the corpus is never written).
# Only single-module entries are regenerated. -g adds debug information.
# Environment: HOD_KILN (kiln++), HOD_KILN_REPO (kiln's checkout).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
main="$(cd "$(git -C "$here" rev-parse --path-format=absolute --git-common-dir)/.." && pwd)"
case "$main" in */.git/modules*) main="${main%%/.git/modules*}";; esac
root="${HOD_BUILD_ROOT:-$main/.build}"
corpus="$root/corpus"
kiln="${HOD_KILN:-$root/target-kiln/release/kiln++}"
repo="${HOD_KILN_REPO:-$main/kiln}"
out="${1:?usage: preopt-corpus.sh <out dir> [-j N] [-g] [--llvm M] [--limit N]}"
shift
jobs=4 debug= major= limit=0
while [ $# -gt 0 ]; do
  case "$1" in
    -j) jobs="$2"; shift ;;
    -g) debug=-g ;;
    --llvm) major="$2"; shift ;;
    --limit) limit="$2"; shift ;;
    *) echo "preopt-corpus.sh: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
[ "$jobs" -le 4 ] || { echo "preopt-corpus.sh: -j is at most 4 on a shared machine" >&2; exit 2; }
mkdir -p "$out"
export corpus kiln repo out debug
one() {
  local entry="$1" args="$2"
  local dest="$out/$entry"
  mkdir -p "$(dirname "$dest")"
  local tmp="$dest.tmp"
  # shellcheck disable=SC2086
  if (cd "$repo" && "$kiln" ${args//<cwd>/$repo} -O0 $debug -S -emit-llvm \
        -Xclang -disable-llvm-passes -o "$tmp") 2>"$dest.err"; then
    { sed -n '/^; ModuleID/q;p' "$corpus/$entry"; cat "$tmp"; } >"$dest"
    rm -f "$tmp" "$dest.err"
  else
    rm -f "$tmp"
  fi
}
export -f one
awk -F'\t' -v major="$major" -v limit="$limit" '
  NR > 1 && $2 == "kiln" && (major == "" || $3 == major) && index($7, "&&") == 0 {
    if (limit > 0 && ++seen[$3] > limit) next
    printf "%s\t%s\n", $1, $7
  }' "$corpus/MANIFEST.tsv" |
  tr '\n' '\0' | xargs -0 -P "$jobs" -I{} bash -c 'IFS=$'"'"'\t'"'"' read -r e a <<<"$1"; one "$e" "$a"' _ {}
echo "preopt-corpus.sh: $(find "$out" -name '*.ll' | wc -l) modules in $out, $(find "$out" -name '*.err' | wc -l) failures"
