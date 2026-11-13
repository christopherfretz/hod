#!/usr/bin/env bash
# libc++'s test suite for x86-64 under qemu, built by kiln with hod as its
# back end (WP-135; docs/status/libcxx-hod-x64.md, spec/harness.md "libc++
# on x86-64 under qemu"). It chains the two wrappers that already exist:
#
#   kiln's `cargo xtask libcxx --compiler scripts/libcxx-x64-kiln.sh`
#     -> scripts/libcxx-x64-kiln.sh   (x86-64 target, archives, qemu launcher)
#     -> scripts/libcxx-hod-kiln.sh   (per-test coverage log, args.tsv)
#     -> <shim>/kiln++-hod            (kiln++ whose llc is hod, llc fallback)
#
# Usage:
#   scripts/libcxx-hod-x64.sh <hybrid|strict> <run dir> [xtask libcxx args...]
#
# <run dir> receives <mode>/ (HOD_LIBCXX_LOGDIR), <mode>.tsv (results) and
# <mode>.log (the runner's output plus oom_kill before and after). The
# remaining arguments go to `cargo xtask libcxx` after the fixed ones; with
# no path among them, the selection is `std libcxx`. `--shard K/N` among
# them names all three <mode>-K-of-N, so the shards of one run can be run
# one after the other, and a run that died resumes at the first shard
# without a results file (an interrupted shard is rerun whole: delete its
# three entries first; kiln's runner cannot resume inside a run).
#
#   scripts/libcxx-hod-x64.sh merge <run dir> <mode>
#
# joins the shards' results into <mode>.tsv and their log directories into
# <mode>/ (args.tsv concatenated, cov/ and phases/ files copied; keys are
# the runner's scratch directories, so they do not collide), the input of
# scripts/libcxx-hod-report.py.
#
# Environment:
#   HOD_SHIM   the shim directory (default: <workspace>/.build/target-<worktree>/llvm-22-shim)
#   HOD_JOBS   -j (default 3, the baseline's)
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
w="$(cd "$here/.." && pwd)"
ws="$w/.."
[ -d "$ws/.build" ] || ws="$w/../../.."
ws="$(cd "$ws" && pwd)"
mode="${1:?usage: libcxx-hod-x64.sh <hybrid|strict> <run dir> [args]}"
dir="${2:?usage: libcxx-hod-x64.sh <hybrid|strict> <run dir> [args]}"
shift 2
if [ "$mode" = merge ]; then
    m="${1:?merge needs the mode}"
    out="$dir/$m"
    mkdir -p "$out/cov" "$out/phases"
    : > "$out/args.tsv"
    first=1
    for t in "$dir/$m"-*-of-*.tsv; do
        if [ "$first" = 1 ]; then head -n 1 "$t" > "$dir/$m.tsv"; first=0; fi
        tail -n +2 "$t" >> "$dir/$m.tsv"
        d="${t%.tsv}"
        cat "$d/args.tsv" >> "$out/args.tsv"
        find "$d/cov" -name '*.tsv' -exec cp {} "$out/cov/" +
        [ -d "$d/phases" ] && find "$d/phases" -name '*.tsv' -exec cp {} "$out/phases/" +
    done
    exit 0
fi
shim="${HOD_SHIM:-$ws/.build/target-$(basename "$w")/llvm-22-shim}"
[ -x "$shim/kiln++-hod" ] || { echo "no shim at $shim (cargo xtask shim ... --kiln)" >&2; exit 2; }
name="$mode"
sel=(std libcxx)
# Positional arguments (paths) replace the default selection.
has_pos=0
opt=""
for a in "$@"; do
    if [ -n "$opt" ]; then
        [ "$opt" = --shard ] && name="$mode-${a%/*}-of-${a#*/}"
        opt=""
        continue
    fi
    case "$a" in
        --shard=*) s="${a#--shard=}"; name="$mode-${s%/*}-of-${s#*/}" ;;
        --shard | --filter | --timeout | --run-timeout-long | -j | --jobs | --mem-limit | --compile-timeout) opt="$a" ;;
        -*) ;;
        *) has_pos=1 ;;
    esac
done
[ "$has_pos" = 1 ] && sel=()
case "$mode" in
    hybrid) export HOD_CENSUS=1; unset HOD_STRICT ;;
    strict) export HOD_STRICT=1; unset HOD_CENSUS ;;
    *) echo "mode must be hybrid or strict" >&2; exit 2 ;;
esac
mkdir -p "$dir"
log="$dir/$name.log"
export HOD_X64_KILN="$w/scripts/libcxx-hod-kiln.sh"
export HOD_LIBCXX_KILN="$shim/kiln++-hod"
export HOD_LIBCXX_LOGDIR="$dir/$name"
{
    grep oom_kill /sys/fs/cgroup/memory.events || true
    date
} > "$log"
cd "$ws/kiln"
# shellcheck disable=SC1091
source scripts/env.sh
set +e
cargo xtask libcxx --tag llvmorg-22.1.8 --compiler "$w/scripts/libcxx-x64-kiln.sh" \
    "${sel[@]}" --timeout 160 --run-timeout-long 1920 -j "${HOD_JOBS:-3}" --mem-limit 3072 \
    --no-fail --results "$dir/$name.tsv" "$@" >> "$log" 2>&1
st=$?
set -e
{
    date
    grep oom_kill /sys/fs/cgroup/memory.events || true
    echo "exit $st"
} >> "$log"
exit "$st"
