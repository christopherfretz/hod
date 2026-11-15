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
#   scripts/avx512-summary.sh --check <report>
#
# `--check` (WP-238, where an emulator runs what the processor cannot)
# prints and exits 1 unless the report has a line and every program
# executed, but the zmm grid's Knights Landing machine (ER and PF, which
# no processor or emulator at hand has), which may be skipped.
set -u
if [ "${1:-}" = "--check" ]; then
    report=${2:?usage: avx512-summary.sh --check <report>}
    [ -s "$report" ] || { echo "no AVX-512 report: nothing executed" >&2; exit 1; }
    bad=$(awk -F'\t' '$2 != "executed" && !($2 == "skipped" && $1 ~ /^abi zmm_grid [0-9]+ knl /)' \
        "$report")
    [ -z "$bad" ] || { printf 'not executed:\n%s\n' "$bad" >&2; exit 1; }
    echo "every AVX-512 program executed ($(wc -l < "$report" | tr -d ' ') lines)"
    exit 0
fi
report=${1:?usage: avx512-summary.sh <report> [<cpuinfo>]}
cpuinfo=${2:-/proc/cpuinfo}

model=$(awk -F': *' '/^model name/ { print $2; exit }' "$cpuinfo" 2>/dev/null)
flags=$(awk -F': *' '/^flags/ { print $2; exit }' "$cpuinfo" 2>/dev/null |
    tr ' ' '\n' | grep '^avx512' | tr '\n' ' ')

echo "### AVX-512 execution"
echo
echo "- processor: ${model:-unknown (no model name in $cpuinfo)}"
echo "- avx512 flags: ${flags:-none}"
# WP-238: the runner (an emulator) that runs what this processor cannot.
if [ -n "${HOD_X64_AVX512_RUNNER:-}" ]; then
    echo "- runner: \`$HOD_X64_AVX512_RUNNER\`"
    rcpu=${HOD_X64_AVX512_RUNNER_CPUINFO:-}
    if [ -n "$rcpu" ]; then
        rmodel=$(awk -F': *' '/^model name/ { print $2; exit }' "$rcpu" 2>/dev/null)
        rflags=$(awk -F': *' '/^flags/ { print $2; exit }' "$rcpu" 2>/dev/null |
            tr ' ' '\n' | grep '^avx512' | tr '\n' ' ')
        echo "- runner's processor: ${rmodel:-unknown}; avx512 flags: ${rflags:-none}"
    fi
fi
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
natively=$(awk -F'\t' '$2 == "executed" && $3 ~ /^native: /' "$report" | wc -l | tr -d ' ')
echo "- executed natively $natively, under the runner $((executed - natively))"
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
