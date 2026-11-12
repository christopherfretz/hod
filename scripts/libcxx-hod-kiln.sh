#!/bin/sh
# kiln++ through hod's shim, for kiln's `cargo xtask libcxx --compiler`
# (docs/status/libcxx-hod.md "Repeating the run").
#
# kiln's libc++ runner runs every compile and link step of a test with the
# test's scratch directory as the working directory and sets
# KILN_LLVM_DIR itself; the shim's `kiln++-hod` passes --llvm-dir= first,
# which beats the variable, so kiln's llc is hod. This wrapper adds the
# per-test bookkeeping the shared coverage log cannot give: it names the
# invocation after its scratch directory (`<run dir>/<test dir>`), records
# the command line, and points hod's coverage log and phase report at
# files of that name, so every coverage line can be traced to its test.
#
# Environment:
#   HOD_LIBCXX_KILN    the shim's kiln++-hod (required)
#   HOD_LIBCXX_LOGDIR  where the logs go (unset: no bookkeeping, plain
#                      kiln++-hod); creates <dir>/cov/<key>.tsv (coverage
#                      lines), <dir>/phases/<key>.tsv (HOD_TIME_PHASES) and
#                      appends "<key><TAB><argv>" to <dir>/args.tsv
# HOD_STRICT and HOD_CENSUS pass through to hod unchanged.
set -eu
: "${HOD_LIBCXX_KILN:?set HOD_LIBCXX_KILN to the shim's kiln++-hod}"
if [ -n "${HOD_LIBCXX_LOGDIR:-}" ]; then
    key="$(basename "$(dirname "$PWD")")-$(basename "$PWD")"
    mkdir -p "$HOD_LIBCXX_LOGDIR/cov" "$HOD_LIBCXX_LOGDIR/phases"
    printf '%s\t%s\n' "$key" "$*" >> "$HOD_LIBCXX_LOGDIR/args.tsv"
    HOD_COVERAGE_LOG="$HOD_LIBCXX_LOGDIR/cov/$key.tsv"
    HOD_TIME_PHASES="$HOD_LIBCXX_LOGDIR/phases/$key.tsv"
    export HOD_COVERAGE_LOG HOD_TIME_PHASES
fi
exec "$HOD_LIBCXX_KILN" "$@"
