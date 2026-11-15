#!/bin/bash
# The compiler wrapper of the LLVM build (WP-178), installed under four
# names (symlinks to this file) that choose the flavour and the language:
#
#   kiln-hod-cxx, kiln-hod-cc   kiln++ / kiln with hod as the back end
#                               (KILN_BACKEND=hod; a decline or a failure
#                               falls back to llc unless KILN_HOD_STRICT=1)
#   kiln-llc-cxx, kiln-llc-cc   kiln++ / kiln with its LLVM back end: the
#                               control
#
# It runs kiln's own logging wrapper (kiln's scripts/oss/kiln-cxx and
# kiln-cc: address-space cap, timeout, one JSON line per invocation in
# $KILN_OSS_LOG) with these settings:
#
#   KILN_OSS_LOG      $HOD_OSS_ROOT/llvm-<flavour>/logs/tu.jsonl
#   KILN_STATS_LOG    $HOD_OSS_ROOT/llvm-<flavour>/logs/stats.jsonl
#   HOD_COVERAGE_LOG  $HOD_OSS_ROOT/llvm-hod/logs/coverage.tsv (hod only)
#   HOD_CENSUS        1 (hod only): every refusal name of a module
#   KILN_HOD          $HOD_OSS_ROOT/llvm-hod/bin/hod (a frozen copy)
#
# Environment: HOD_OSS_ROOT (required: the directory holding llvm-src,
# llvm-hod, llvm-llc), HOD_OSS_KILN (required: the directory of kiln++
# and kiln), HOD_OSS_KILN_SCRIPTS (required: kiln's scripts/oss),
# HOD_OSS_PHASE (configure | build | test; recorded). KILN_HOD_STRICT
# passes through.
set -eu -o pipefail
: "${HOD_OSS_ROOT:?set HOD_OSS_ROOT}"
: "${HOD_OSS_KILN:?set HOD_OSS_KILN to the directory of kiln++}"
: "${HOD_OSS_KILN_SCRIPTS:?set HOD_OSS_KILN_SCRIPTS to kiln/scripts/oss}"
name=$(basename "$0")
case "$name" in
  kiln-hod-*) flavour=hod ;;
  kiln-llc-*) flavour=llc ;;
  *) echo "wrap.sh: run me as kiln-{hod,llc}-{cxx,cc}, not $name" >&2; exit 2 ;;
esac
tree="$HOD_OSS_ROOT/llvm-$flavour"
# One token of the lanes pool per compile or link (WP-222): ninja is not
# a jobserver client, so the wrapper takes it (lane-token.py, MAKEFLAGS
# from env.sh) and hod inside counts it as its implicit one.
if [ -z "${HOD_OSS_TOKEN:-}" ]; then
  export HOD_OSS_TOKEN=1
  exec python3 "$(dirname "$(readlink -f "$0")")/lane-token.py" "$0" "$@"
fi
mkdir -p "$tree/logs"
export KILN_OSS_LOG="$tree/logs/tu.jsonl"
export KILN_OSS_PHASE="${HOD_OSS_PHASE:-build}"
# kiln's compile statistics log (kiln spec/driver.md): one JSON line per
# front-end job with its opt triggers and what the back end did
# (`backend`: llvm | hod | hod-declined | hod-failed; `hod_reason`).
export KILN_STATS_LOG="$tree/logs/stats.jsonl"
export KILN_OSS_CXX="$HOD_OSS_KILN/kiln++" KILN_OSS_CC="$HOD_OSS_KILN/kiln"
if [ "$flavour" = hod ]; then
  export KILN_BACKEND=hod KILN_HOD="$tree/bin/hod"
  export HOD_COVERAGE_LOG="$tree/logs/coverage.tsv" HOD_CENSUS=1
else
  export KILN_BACKEND=llvm
  unset KILN_HOD HOD_COVERAGE_LOG HOD_CENSUS
fi
# A link maps every archive it reads (mold maps the whole of each -O0
# static library at once): kiln's 3 GB address-space cap is for
# compiles. Links get 16 GB of address space (they run one at a time).
is_link=1
for a in "$@"; do
  case "$a" in -c | -S | -E | -fsyntax-only) is_link=0; break ;; esac
done
[ "$is_link" = 1 ] && export KILN_OSS_VMEM_KB="${HOD_OSS_LINK_VMEM_KB:-16000000}"
case "$name" in
  *-cxx) inner="$HOD_OSS_KILN_SCRIPTS/kiln-cxx" ;;
  *-cc) inner="$HOD_OSS_KILN_SCRIPTS/kiln-cc" ;;
  *) echo "wrap.sh: unknown language in $name" >&2; exit 2 ;;
esac
# Strict (HOD_OSS_STRICT=1, hod tree, compiles): KILN_HOD_STRICT=1 makes
# only a hod *failure* an error; a decline (hod's status 2) still falls
# back to llc silently, and a job kiln does not offer to hod never meets
# it. So the job's own statistics line decides: every front-end job of
# the invocation must say backend `hod`, else the compile fails here
# (after kiln wrote its object, which is removed) naming the backend and
# the reason. The lines are appended to the shared statistics log.
if [ "${HOD_OSS_STRICT:-0}" != 1 ] || [ "$flavour" != hod ] || [ "$is_link" = 1 ]; then
  exec "$inner" "$@"
fi
export KILN_HOD_STRICT=1
shared="$KILN_STATS_LOG"
mine="$(mktemp "$tree/logs/stats-XXXXXX")"
export KILN_STATS_LOG="$mine"
rc=0
"$inner" "$@" || rc=$?
cat "$mine" >> "$shared"
bad=$(python3 "$HOD_OSS_SCRIPTS/strict-check.py" "$mine")
rm -f "$mine"
if [ "$rc" = 0 ] && [ -n "$bad" ]; then
  echo "wrap.sh: strict: not compiled by hod: $bad" >&2
  for ((i = 1; i <= $#; i++)); do
    if [ "${!i}" = -o ]; then j=$((i + 1)); rm -f "${!j}"; fi
  done
  exit 1
fi
exit $rc
