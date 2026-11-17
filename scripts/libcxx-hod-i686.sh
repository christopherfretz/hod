#!/usr/bin/env bash
# libc++'s test suite for i686 under qemu-i386, with clang 22 as the IR
# producer and hod as the back end (WP-330; design 0008 step I14;
# docs/status/libcxx-hod-i686.md). Modelled on scripts/libcxx-hod-x64.sh:
#
#   kiln's `cargo xtask libcxx --compiler scripts/libcxx-i686-clang.sh`
#     -> scripts/libcxx-i686-clang.sh  (clang -> IR -> hod/llc -> qemu launcher)
#
# The libc++ is the i686 one of scripts/build-libcxx-i686.sh (built by
# clang, the control library). The runner sees a clang and runs in its
# clang mode; its `target=` lit feature stays the host's (as on the x86-64
# lane).
#
# Usage:
#   scripts/libcxx-hod-i686.sh <hybrid|strict|control> <run dir> [xtask libcxx args...]
#
# hybrid: hod with llc as its fallback (HOD_CENSUS=1: every refusal of
# every module is counted, the run goes on); strict: hod alone; control:
# llc alone (the same IR, `-O0 -global-isel=0 -fast-isel=0`). <run dir>
# receives <mode>/ (HOD_LIBCXX_LOGDIR: args.tsv, cov/, time.tsv),
# <mode>.tsv (results) and <mode>.log. With no path among the remaining
# arguments the selection is `std libcxx`; with HOD_I686_SELECTION=<file>
# it is that file's lines (paths relative to libcxx/test). `--shard K/N`
# names all three <mode>-K-of-N, so the shards of one run can be run one
# after the other, and a run that died resumes at the first shard without
# a results file (an interrupted shard is rerun whole: delete its three
# entries first).
#
#   scripts/libcxx-hod-i686.sh merge <run dir> <mode>
#
# joins the shards' results into <mode>.tsv and their log directories into
# <mode>/, the input of scripts/libcxx-hod-report.py.
#
# Environment:
#   HOD_I686_HOD   the hod binary (default: <workspace>/.build/target-<worktree>/release/hod)
#   HOD_JOBS       -j (default 4)
#   and those of scripts/libcxx-i686-clang.sh.
set -euo pipefail
here="${BASH_SOURCE[0]%/*}"
[ "$here" = "${BASH_SOURCE[0]}" ] && here=.
cd "$here/.."
w="$PWD"
ws="$w/.."
[ -d "$ws/.build" ] || ws="$w/../../.."
mode="${1:?usage: libcxx-hod-i686.sh <hybrid|strict|control> <run dir> [args]}"
dir="${2:?usage: libcxx-hod-i686.sh <hybrid|strict|control> <run dir> [args]}"
shift 2
if [ "$mode" = merge ]; then
    m="${1:?merge needs the mode}"
    out="$dir/$m"
    mkdir -p "$out/cov"
    : > "$out/args.tsv"
    : > "$out/time.tsv"
    first=1
    for t in "$dir/$m"-*-of-*.tsv; do
        if [ "$first" = 1 ]; then head -n 1 "$t" > "$dir/$m.tsv"; first=0; fi
        tail -n +2 "$t" >> "$dir/$m.tsv"
        d="${t%.tsv}"
        cat "$d/args.tsv" >> "$out/args.tsv"
        [ -f "$d/time.tsv" ] && cat "$d/time.tsv" >> "$out/time.tsv"
        [ -d "$d/cov" ] && find "$d/cov" -name '*.tsv' -exec cp -t "$out/cov/" {} +
    done
    exit 0
fi
wt="${w##*/}"
hod="${HOD_I686_HOD:-$ws/.build/target-$wt/release/hod}"
case "$mode" in
    hybrid | strict) [ -x "$hod" ] || { echo "no hod at $hod" >&2; exit 2; } ;;
    control) ;;
    *) echo "mode must be hybrid, strict or control" >&2; exit 2 ;;
esac
root="${HOD_I686_LIBCXX_ROOT:-$ws/.toolchain/libcxx-22-i686-root}"
[ -f "$root/lib/libc++.a" ] || { echo "no i686 libc++ at $root (scripts/build-libcxx-i686.sh)" >&2; exit 2; }
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
if [ -n "${HOD_I686_SELECTION:-}" ]; then
    mapfile -t sel < "$HOD_I686_SELECTION"
elif [ "$has_pos" = 1 ]; then
    sel=()
fi
mkdir -p "$dir"
log="$dir/$name.log"
export HOD_I686_MODE="$mode"
export HOD_I686_HOD="$hod"
export HOD_I686_LIBCXX_ROOT="$root"
export HOD_LIBCXX_LOGDIR="$dir/$name"
unset HOD_STRICT HOD_CENSUS
{
    grep oom_kill /sys/fs/cgroup/memory.events || true
    date
} > "$log"
cd "$ws/kiln"
# shellcheck disable=SC1091
source scripts/env.sh
jsf="${HOD_JOBSERVER_FIFO:-/run/hod/jobserver.fifo}"
if [ -p "$jsf" ]; then export MAKEFLAGS="--jobserver-auth=fifo:$jsf"; fi
set +e
cargo xtask libcxx --tag llvmorg-22.1.8 --compiler "$w/scripts/libcxx-i686-clang.sh" \
    "${sel[@]}" --timeout 160 --run-timeout-long 1920 -j "${HOD_JOBS:-4}" --mem-limit 3072 \
    --no-fail --results "$dir/$name.tsv" "$@" >> "$log" 2>&1
st=$?
set -e
{
    date
    grep oom_kill /sys/fs/cgroup/memory.events || true
    echo "exit $st"
} >> "$log"
exit "$st"
