#!/usr/bin/env bash
# libc++'s test suite with debug information (`-g`), built by kiln with
# `llc` (the control) or with hod in place of `llc` (hybrid, strict), for
# AArch64 natively or for x86-64 under qemu (WP-210;
# docs/status/libcxx-hod-debug.md, libcxx-hod-debug-x64.md). It is the
# recipes of docs/status/libcxx-hod.md and libcxx-hod-x64.md with
# scripts/libcxx-debug-kiln.sh in front of their compiler chains:
#
#   scripts/libcxx-hod-debug.sh <control|hybrid|strict> <run dir> [xtask libcxx args...]
#
# <run dir> receives <mode>.tsv (kiln's --results), <mode>.log (the
# runner's output, oom_kill and the date before and after) and, for hybrid
# and strict, <mode>/ (scripts/libcxx-hod-kiln.sh's HOD_LIBCXX_LOGDIR:
# args.tsv, cov/, phases/). Arguments after the run directory go to
# `cargo xtask libcxx`; a path among them replaces the default selection
# `std libcxx`; `--shard K/N` names the three outputs <mode>-K-of-N.
#
#   scripts/libcxx-hod-debug.sh merge <run dir> <mode>
#
# joins the shards' results into <mode>.tsv and their log directories into
# <mode>/ (the input of scripts/libcxx-hod-report.py).
#
# Environment:
#   HOD_DEBUG_TARGET  aarch64 (default: the host, natively) or x86_64
#                     (scripts/libcxx-x64-kiln.sh: cross, tests under qemu)
#   HOD_DEBUG_FLAGS   the flags added to every invocation (default when
#                     unset: -g; set and empty, HOD_DEBUG_FLAGS=, adds
#                     nothing: the suite without -g, WP-233)
#   HOD_SHIM          the shim directory, from `cargo xtask shim --kiln`
#                     (required for hybrid and strict)
#   HOD_KILN          kiln++ (default: <workspace>/.build/target-kiln/release/kiln++)
#   HOD_JOBS          -j (default 3, the baselines')
#   HOD_TIMEOUT       the run limit (default: 20 natively, 160 for x86-64,
#                     the baselines'; x86-64 also gets --run-timeout-long 1920)
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
w="$(cd "$here/.." && pwd)"
ws="$w/.."
[ -d "$ws/.build" ] || ws="$w/../../.."
ws="$(cd "$ws" && pwd)"
usage="usage: libcxx-hod-debug.sh <control|hybrid|strict|merge> <run dir> [args]"
mode="${1:?$usage}"
dir="${2:?$usage}"
shift 2
if [ "$mode" = merge ]; then
    m="${1:?merge needs the mode}"
    out="$dir/$m"
    first=1
    for t in "$dir/$m"-*-of-*.tsv; do
        if [ "$first" = 1 ]; then head -n 1 "$t" > "$dir/$m.tsv"; first=0; fi
        tail -n +2 "$t" >> "$dir/$m.tsv"
        d="${t%.tsv}"
        if [ -f "$d/args.tsv" ]; then
            mkdir -p "$out/cov" "$out/phases"
            cat "$d/args.tsv" >> "$out/args.tsv"
            find "$d/cov" -name '*.tsv' -exec cp -t "$out/cov/" {} +
            [ -d "$d/phases" ] && find "$d/phases" -name '*.tsv' -exec cp -t "$out/phases/" {} +
        fi
    done
    exit 0
fi
target="${HOD_DEBUG_TARGET:-$(uname -m)}"
kiln="${HOD_KILN:-$ws/.build/target-kiln/release/kiln++}"
name="$mode"
sel=(std libcxx)
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
unset HOD_STRICT HOD_CENSUS HOD_LIBCXX_LOGDIR
case "$mode" in
    control) ;;
    hybrid) export HOD_CENSUS=1 ;;
    strict) export HOD_STRICT=1 ;;
    *) echo "$usage" >&2; exit 2 ;;
esac
if [ "$mode" != control ]; then
    : "${HOD_SHIM:?set HOD_SHIM to the shim directory (cargo xtask shim ... --kiln)}"
    [ -x "$HOD_SHIM/kiln++-hod" ] || { echo "no kiln++-hod in $HOD_SHIM" >&2; exit 2; }
    export HOD_LIBCXX_KILN="$HOD_SHIM/kiln++-hod"
    export HOD_LIBCXX_LOGDIR="$dir/$name"
    hodc="$w/scripts/libcxx-hod-kiln.sh"
fi
case "$target" in
    aarch64)
        [ "$mode" = control ] && export HOD_DEBUG_NEXT="$kiln" || export HOD_DEBUG_NEXT="$hodc"
        limits=(--timeout "${HOD_TIMEOUT:-20}")
        ;;
    x86_64)
        export HOD_DEBUG_NEXT="$w/scripts/libcxx-x64-kiln.sh"
        [ "$mode" = control ] && export HOD_X64_KILN="$kiln" || export HOD_X64_KILN="$hodc"
        limits=(--timeout "${HOD_TIMEOUT:-160}" --run-timeout-long 1920)
        ;;
    *) echo "HOD_DEBUG_TARGET must be aarch64 or x86_64" >&2; exit 2 ;;
esac
mkdir -p "$dir"
log="$dir/$name.log"
{
    grep oom_kill /sys/fs/cgroup/memory.events || true
    date
    echo "target $target, flags '${HOD_DEBUG_FLAGS--g}', next $HOD_DEBUG_NEXT${HOD_X64_KILN:+ then $HOD_X64_KILN}${HOD_LIBCXX_KILN:+, hod $HOD_LIBCXX_KILN}"
} > "$log"
cd "$ws/kiln"
# shellcheck disable=SC1091
source scripts/env.sh
# The machine's pool, as the baseline runs drew from it (kiln's env.sh
# names kiln's own fifo, which this machine does not run).
jsf="${HOD_JOBSERVER_FIFO:-/run/hod/jobserver.fifo}"
if [ -p "$jsf" ]; then export MAKEFLAGS="--jobserver-auth=fifo:$jsf"; fi
set +e
cargo xtask libcxx --tag llvmorg-22.1.8 --compiler "$w/scripts/libcxx-debug-kiln.sh" \
    "${sel[@]}" "${limits[@]}" -j "${HOD_JOBS:-3}" --mem-limit 3072 \
    --no-fail --results "$dir/$name.tsv" "$@" >> "$log" 2>&1
st=$?
set -e
{
    date
    grep oom_kill /sys/fs/cgroup/memory.events || true
    echo "exit $st"
} >> "$log"
exit "$st"
