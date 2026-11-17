#!/usr/bin/env bash
# clang++ 22 for i686-linux-gnu with hod (or llc) as the back end, under
# qemu-i386: the compiler kiln's `cargo xtask libcxx --compiler` runs in
# the i386 libc++ lane (WP-330; scripts/libcxx-hod-i686.sh,
# docs/status/libcxx-hod-i686.md). The runner sees a clang (`--version`
# passes through), so it runs in its clang mode. Every invocation:
#
# 1. gets `--target=i686-linux-gnu` (clang finds the cross GCC
#    installation and /usr/i686-linux-gnu by itself), the libc++ headers
#    of $HOD_I686_LIBCXX_ROOT in place of the runner's `-isystem
#    .../c++/v1`, and `-L<root>/lib` (the i686 static archives) in place
#    of the runner's `-L.../aarch64-linux-gnu`, or added when it links;
# 2. when it writes code from sources (no -fsyntax-only, -E, -M, -MM,
#    -S, -emit-llvm, --precompile, -x, standard input): each source
#    (.cpp .cc .cxx .c .pcm) is lowered to IR by clang (`-S -emit-llvm`,
#    the invocation's own flags) and the IR to an object by the back end
#    of $HOD_I686_MODE: `hybrid` hod with `llc` as its fallback and
#    HOD_CENSUS=1, `strict` hod alone, `control` `llc` alone (`-O0
#    -global-isel=0 -fast-isel=0`, the reference); both get
#    `-relocation-model=pic` when the module has a "PIC Level" flag
#    (clang's default PIE). The objects take the sources' places in the
#    link (or are the `-c` outputs);
# 3. when it writes an executable, it links the real binary to
#    `<out>.i686` and writes `<out>` as a script that `exec`s `qemu-i386
#    -cpu pentium2,-mmx -L /usr/i686-linux-gnu -R <guest space>` on it
#    (the CPU of crates/hod_testutil's I686_EMULATED_CPU: clang's default
#    `i686`, no SSE), as scripts/libcxx-x64-kiln.sh does for x86-64.
#
# Any other invocation goes to clang unchanged (plus 1), so LLVM writes
# its code; those are the runner's probes, never a test's objects.
#
# Bookkeeping with HOD_LIBCXX_LOGDIR (the layout of
# scripts/libcxx-hod-kiln.sh, read by scripts/libcxx-hod-report.py): the
# key is `<run dir>-<test dir>` of the working directory; "<key><TAB>argv"
# is appended to <dir>/args.tsv, hod's coverage lines go to
# <dir>/cov/<key>.tsv, and one line per step to <dir>/time.tsv:
# key, step (ir, hod, llc, link), microseconds, exit status, source.
#
# Environment:
#   HOD_I686_MODE         hybrid (default), strict or control
#   HOD_I686_HOD          the hod binary (required unless control)
#   HOD_I686_LIBCXX_ROOT  the i686 libc++ (default <workspace>/.toolchain/libcxx-22-i686-root)
#   HOD_I686_BIN          the LLVM 22 bin directory (default /usr/lib/llvm-22/bin)
#   HOD_I686_QEMU         the emulator (default qemu-i386 on PATH)
#   HOD_I686_GUEST        qemu's -R (default 0x40000000, 1 GiB)
#   HOD_I686_KEEP         1: keep the .ll files beside the objects
set -euo pipefail
here="${BASH_SOURCE[0]%/*}"
[ "$here" = "${BASH_SOURCE[0]}" ] && here=.
ws="$here/../.."
[ -d "$ws/.build" ] || ws="$here/../../../.."
bin="${HOD_I686_BIN:-/usr/lib/llvm-22/bin}"
clang="$bin/clang++"
llc="$bin/llc"
mode="${HOD_I686_MODE:-hybrid}"
root="${HOD_I686_LIBCXX_ROOT:-$ws/.toolchain/libcxx-22-i686-root}"
qemu="${HOD_I686_QEMU:-qemu-i386}"
guest="${HOD_I686_GUEST:-0x40000000}"
sysroot=/usr/i686-linux-gnu
cpu=pentium2,-mmx
triple=i686-unknown-linux-gnu

args=("--target=i686-linux-gnu")
srcs=()
src_at=()
out=""
links=1
codegen=1
prev=""
for a in "$@"; do
    if [ -n "$prev" ]; then
        case "$prev" in
            -o) out="$a" ;;
            -isystem)
                case "$a" in */c++/v1) a="$root/include/c++/v1" ;; esac ;;
        esac
        prev=""
        args+=("$a")
        continue
    fi
    case "$a" in
        -o | -I | -isystem | -iquote | -idirafter | -include | -imacros | -MF | -MT | -MQ | \
            -Xclang | -Xlinker | -Xpreprocessor | -Xassembler | -mllvm | -D | -U | -L | -l | \
            -isysroot | --sysroot | -target | -resource-dir | -include-pch | -arch | -F | -z | \
            -u | -T)
            prev="$a"; args+=("$a") ;;
        -x | -) codegen=0; args+=("$a") ;;
        -L*/aarch64-linux-gnu) args+=("-L$root/lib") ;;
        -c) links=0; args+=("$a") ;;
        -o?*) out="${a#-o}"; args+=("$a") ;;
        -fsyntax-only | -E | -M | -MM | -S | -emit-llvm | --precompile | -shared | -r | \
            -### | -v | --version | -dumpversion | -dumpmachine | -print-*)
            links=0; codegen=0; args+=("$a") ;;
        -*) args+=("$a") ;;
        *.cpp | *.cc | *.cxx | *.c | *.pcm)
            src_at+=("${#args[@]}"); srcs+=("$a"); args+=("$a") ;;
        *) args+=("$a") ;;
    esac
done
[ "$links" = 1 ] && args+=("-L$root/lib")

key=""
logdir="${HOD_LIBCXX_LOGDIR:-}"
if [ -n "$logdir" ]; then
    p="$PWD"
    d="${p%/*}"
    key="${d##*/}-${p##*/}"
    mkdir -p "$logdir/cov"
    printf '%s\t%s\n' "$key" "$*" >> "$logdir/args.tsv"
fi
note() { # step, microseconds, status, source
    [ -n "$logdir" ] && printf '%s\t%s\t%s\t%s\t%s\n' "$key" "$1" "$2" "$3" "$4" >> "$logdir/time.tsv"
    return 0
}

if [ "$codegen" = 1 ] && [ "${#srcs[@]}" -gt 0 ]; then
    # The flags of the IR step: everything but the output, the sources,
    # the link-only words and -c.
    irflags=()
    skip=0
    for i in "${!args[@]}"; do
        a="${args[$i]}"
        if [ "$skip" = 1 ]; then skip=0; continue; fi
        is_src=0
        for j in "${src_at[@]}"; do [ "$i" = "$j" ] && is_src=1; done
        [ "$is_src" = 1 ] && continue
        case "$a" in
            -o | -Xlinker | -l | -L | -z | -u | -T) skip=1 ;;
            -c | -l* | -L* | -Wl,* | -static | -pie | -no-pie | -rdynamic | *.o | *.a | *.so) ;;
            *) irflags+=("$a") ;;
        esac
    done
    n=0
    for s in "${srcs[@]}"; do
        base="${s##*/}"
        base="${base%.*}"
        if [ "$links" = 0 ] && [ -n "$out" ] && [ "${#srcs[@]}" = 1 ]; then
            obj="$out"
        elif [ "$links" = 0 ]; then
            obj="$base.o"
        else
            obj="$PWD/hod-i686-$$-$n-$base.o"
        fi
        ll="$obj.ll"
        t0="${EPOCHREALTIME/./}"
        set +e
        "$clang" "${irflags[@]}" -Wno-unused-command-line-argument -S -emit-llvm -o "$ll" "$s"
        st=$?
        set -e
        t1="${EPOCHREALTIME/./}"
        note ir "$((t1 - t0))" "$st" "$s"
        [ "$st" = 0 ] || exit "$st"
        be=(-O0 -filetype=obj "-mtriple=$triple")
        grep -q '"PIC Level"' "$ll" && be+=(-relocation-model=pic)
        set +e
        case "$mode" in
            control)
                "$llc" "${be[@]}" -global-isel=0 -fast-isel=0 "$ll" -o "$obj"
                st=$?; step=llc ;;
            hybrid)
                HOD_CENSUS=1 HOD_COVERAGE_LOG="${logdir:+$logdir/cov/$key.tsv}" \
                    "${HOD_I686_HOD:?set HOD_I686_HOD}" "${be[@]}" "--fallback=$llc" "$ll" -o "$obj"
                st=$?; step=hod ;;
            strict)
                HOD_COVERAGE_LOG="${logdir:+$logdir/cov/$key.tsv}" \
                    "${HOD_I686_HOD:?set HOD_I686_HOD}" "${be[@]}" --strict "$ll" -o "$obj"
                st=$?; step=hod ;;
            *) echo "HOD_I686_MODE must be hybrid, strict or control" >&2; exit 2 ;;
        esac
        set -e
        t2="${EPOCHREALTIME/./}"
        note "$step" "$((t2 - t1))" "$st" "$s"
        [ "${HOD_I686_KEEP:-0}" = 1 ] || rm -f "$ll"
        [ "$st" = 0 ] || exit "$st"
        args[${src_at[$n]}]="$obj"
        n=$((n + 1))
    done
    [ "$links" = 0 ] && exit 0
    args+=("-Wno-unused-command-line-argument")
fi

if [ "$links" = 0 ] || [ -z "$out" ] || [ "$out" = /dev/null ]; then
    exec "$clang" "${args[@]}"
fi

# An executable: link it beside <out>, then make <out> the qemu launcher.
case "$out" in
    /*) abs="$out" ;;
    *) abs="$PWD/$out" ;;
esac
case "$abs$qemu" in
    *"'"*) echo "a quote in $abs or $qemu" >&2; exit 2 ;;
esac
real="$abs.i686"
for i in "${!args[@]}"; do
    if [ "${args[$i]}" = "-o" ]; then
        args[$((i + 1))]="$real"
    elif [ "${args[$i]}" = "-o$out" ]; then
        args[i]="-o$real"
    fi
done
rm -f "$abs"
t0="${EPOCHREALTIME/./}"
set +e
"$clang" "${args[@]}"
st=$?
set -e
t1="${EPOCHREALTIME/./}"
note link "$((t1 - t0))" "$st" "-"
if [ "$codegen" = 1 ]; then
    for i in "${src_at[@]}"; do
        case "${args[$i]}" in "$PWD"/hod-i686-*.o) rm -f "${args[$i]}" ;; esac
    done
fi
[ "$st" = 0 ] || exit "$st"
printf '#!/bin/sh\nexec %s -cpu %s -L %s -R %s -0 "$0" %s "$@"\n' \
    "'$qemu'" "$cpu" "'$sysroot'" "$guest" "'$real'" > "$abs"
chmod +x "$abs"
