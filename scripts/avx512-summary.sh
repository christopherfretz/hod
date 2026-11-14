#!/usr/bin/env bash
# The summary of the AVX-512 execution tests (WP-229; spec/harness.md
# "CI" item 8, docs/design/0007-avx512.md section 4.4), in Markdown on
# stdout: the processor's model and AVX-512 flags, and from the report the
# tests wrote (HOD_AVX512_REPORT: one line per program, `program`,
# outcome, detail, separated by tabs) how many programs executed, were
# skipped, dry-run or failed, with each program's line. The hosted x86-64
# lane appends it to the `test x64` job's summary. Always exits 0: a
# failure is the tests' to report.
#
#   scripts/avx512-summary.sh <report> [<cpuinfo>, default /proc/cpuinfo]
set -u
report=${1:?usage: avx512-summary.sh <report> [<cpuinfo>]}
cpuinfo=${2:-/proc/cpuinfo}

model=$(awk -F': *' '/^model name/ { print $2; exit }' "$cpuinfo" 2>/dev/null)
flags=$(awk -F': *' '/^flags/ { print $2; exit }' "$cpuinfo" 2>/dev/null |
    tr ' ' '\n' | grep '^avx512' | tr '\n' ' ')

echo "### AVX-512 execution"
echo
echo "- processor: ${model:-unknown (no model name in $cpuinfo)}"
echo "- avx512 flags: ${flags:-none}"
if [ ! -s "$report" ]; then
    echo
    echo "**No report: the AVX-512 execution tests reached no decision (they" \
        "did not run, or failed before running a program). Nothing executed.**"
    exit 0
fi
count() { awk -F'\t' -v o="$1" '$2 == o' "$report" | wc -l | tr -d ' '; }
executed=$(count executed)
skipped=$(count skipped)
dryrun=$(count dry-run)
failed=$(count failed)
echo "- programs: executed $executed, skipped $skipped, dry-run $dryrun, failed $failed"
echo
if [ "$failed" -gt 0 ]; then
    echo "**$failed program(s) FAILED on this processor: see the test output.**"
    echo
elif [ "$executed" -eq 0 ]; then
    echo "**Nothing executed on this runner: this run is no evidence that" \
        "hod's AVX-512 lowering is correct.**"
    echo
fi
for outcome in failed skipped dry-run executed; do
    lines=$(awk -F'\t' -v o="$outcome" '$2 == o { print "- `" $1 "`: " $3 }' "$report")
    if [ -n "$lines" ]; then
        echo "#### $outcome"
        echo
        echo "$lines"
        echo
    fi
done
exit 0
