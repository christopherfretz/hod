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
# directory, default /usr/lib/llvm-22), SAMPLE (optional: a file of test
# paths relative to libcxx/test, one per line; then the sample is the
# hod-built modules of those tests instead of every 10th, so a later run
# can time the same modules again). Output: key, IR bytes, llc us,
# hod us (wall, best of 3 each; llc runs as kiln runs it,
# -O0 -relocation-model=pic -filetype=obj; hod runs without
# HOD_CENSUS, coverage log or phase report, as a build would).
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
pick() { # the coverage files of the sample
    if [ -n "${SAMPLE:-}" ]; then
        # keys whose compile command names a test of $SAMPLE
        awk -F'\t' 'NR == FNR { want[$0] = 1; next }
            / -c -o / { n = split($2, a, " "); for (i = 1; i <= n; i++)
                if (index(a[i], "/libcxx/test/") && a[i] ~ /\.cpp$/) {
                    t = substr(a[i], index(a[i], "/libcxx/test/") + 13)
                    if (t in want) print $1 } }' "$SAMPLE" "$logdir/args.tsv" |
            sort -u | while read -r k; do
                f="$logdir/cov/$k.tsv"
                if [ -f "$f" ] && grep -q '^hod	' "$f"; then echo "$f"; fi
            done
    else
        grep -l '^hod	' "$logdir"/cov/*.tsv | awk "NR % 10 == 1"
    fi
}
pick | head -n "$count" | while read -r f; do
    key=$(basename "$f" .tsv)
    argv=$(awk -F'\t' -v k="$key" '$1 == k && / -c -o / { print $2; exit }' "$logdir/args.tsv")
    [ -n "$argv" ] || continue
    ll="$work/$key.ll"
    # shellcheck disable=SC2086 # the recorded command line, split as recorded
    set -- $(printf '%s' "$argv" | sed "s| -c -o [^ ]*| -S -emit-llvm -o $ll|")
    KILN_LLVM_DIR=$LLVM "$KILN" "$@" 2>/dev/null || continue
    [ -s "$ll" ] || continue
    l=$(us "$LLVM/bin/llc" -O0 -relocation-model=pic -filetype=obj "$ll" -o "$work/$key.llc.o") || continue
    h=$(us env -u HOD_COVERAGE_LOG -u HOD_TIME_PHASES -u HOD_CENSUS -u HOD_FALLBACK_LLC \
        HOD_THREADS=1 HOD_STRICT=1 \
        "$HOD" -O0 -relocation-model=pic -filetype=obj "$ll" -o "$work/$key.hod.o") || continue
    printf '%s\t%s\t%s\t%s\n' "$key" "$(wc -c < "$ll")" "$l" "$h"
    rm -f "$ll" "$work/$key.llc.o" "$work/$key.hod.o"
done
