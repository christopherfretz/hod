#!/bin/sh
# Back-end time of hod against plain llc on a sample of libc++ test
# modules hod built (docs/status/libcxx-hod.md "Time"). The suite run's
# coverage log gives llc's time only for the modules that fell back; for
# the others this regenerates the module with kiln (-S -emit-llvm, the
# test's own compile command from args.tsv) and times both back ends on
# the same text, one after the other, single-threaded.
#
#   scripts/libcxx-hod-time.sh <logdir> <workdir> <count> > times.tsv
#
# Environment: HOD (the hod binary), KILN (kiln++), LLVM (the LLVM
# directory, default /usr/lib/llvm-22). Output: key, IR bytes, llc us,
# hod us (wall, best of 3 each; llc runs as kiln runs it,
# -O0 -relocation-model=pic -filetype=obj).
set -eu
logdir=$1 work=$2 count=$3
: "${HOD:?}" "${KILN:?}"
LLVM=${LLVM:-/usr/lib/llvm-22}
mkdir -p "$work"
us() { # microseconds of the best of three runs of "$@"
    best=
    for _ in 1 2 3; do
        s=$(date +%s%N); "$@" >/dev/null 2>&1 || return 1; e=$(date +%s%N)
        t=$(( (e - s) / 1000 ))
        if [ -z "$best" ] || [ "$t" -lt "$best" ]; then best=$t; fi
    done
    echo "$best"
}
grep -l '^hod	' "$logdir"/cov/*.tsv | awk "NR % 10 == 1" | head -n "$count" | while read -r f; do
    key=$(basename "$f" .tsv)
    argv=$(awk -F'\t' -v k="$key" '$1 == k && / -c -o / { print $2; exit }' "$logdir/args.tsv")
    [ -n "$argv" ] || continue
    ll="$work/$key.ll"
    # shellcheck disable=SC2086 # the recorded command line, split as recorded
    set -- $(printf '%s' "$argv" | sed "s| -c -o [^ ]*| -S -emit-llvm -o $ll|")
    KILN_LLVM_DIR=$LLVM "$KILN" "$@" 2>/dev/null || continue
    [ -s "$ll" ] || continue
    l=$(us "$LLVM/bin/llc" -O0 -relocation-model=pic -filetype=obj "$ll" -o "$work/$key.llc.o") || continue
    h=$(us env -u HOD_COVERAGE_LOG -u HOD_TIME_PHASES -u HOD_CENSUS HOD_THREADS=1 HOD_STRICT=1 \
        "$HOD" -O0 -relocation-model=pic -filetype=obj "$ll" -o "$work/$key.hod.o") || continue
    printf '%s\t%s\t%s\t%s\n' "$key" "$(wc -c < "$ll")" "$l" "$h"
    rm -f "$ll" "$work/$key.llc.o" "$work/$key.hod.o"
done
