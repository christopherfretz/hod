#!/usr/bin/env bash
# kiln++ for x86-64 under qemu-user, for kiln's `cargo xtask libcxx
# --compiler` on an AArch64 host (docs/status/libcxx-baseline-x64.md,
# "Repeating the run"). kiln's runner has no target or executor option: it
# compiles for the host and runs `t.exe` directly. This wrapper makes every
# kiln++ invocation of the runner an x86-64 one:
#
# 1. it adds `--target=<HOD_X64_TARGET>` to every invocation (compiles,
#    links, the runner's flag probes); kiln++ then finds the cross GCC
#    installation and its sysroot by itself;
# 2. it replaces the runner's `-L<libc++ tree>/usr/lib/aarch64-linux-gnu`
#    with `-L<HOD_X64_LIBDIR>`, a directory holding only the x86-64 static
#    archives (`libc++.a`, `libc++abi.a`, `libc++experimental.a`). The
#    libc++ headers are the same files for both targets (Debian's
#    libc++-22-dev, identical trees), so the runner's `-isystem` stays;
# 3. when the invocation writes an executable (`-o <out>` without `-c`,
#    `-S`, `-E`, `-fsyntax-only`, `--precompile`, `-shared` or `-r`), it
#    links the real binary to `<out>.x86_64` and writes `<out>` as a
#    two-line `sh` script that `exec`s `qemu-x86_64 -L <sysroot>` on it,
#    with the script's own name as the program's `argv[0]`. The runner's
#    run step (and a `.sh.cpp` test's `%{exec} %t.exe`) then runs the
#    x86-64 program without binfmt_misc. `exec` keeps qemu in the runner's
#    process group, so its timeout kills qemu.
#
# What it cannot do: the runner's `target=<triple>` lit feature stays the
# host's (`aarch64-unknown-linux-gnu`); see the status page for the one
# test whose verdict that changes.
#
# Environment (all optional):
#   HOD_X64_KILN     the real kiln++ (default: <workspace>/.build/target-kiln/release/kiln++)
#   HOD_X64_TARGET   the target triple (default: x86_64-unknown-linux-gnu)
#   HOD_X64_LIBDIR   the x86-64 static libc++ archives (default: <workspace>/.build/libcxx-x64/lib)
#   HOD_X64_SYSROOT  qemu's -L (default: /usr/x86_64-linux-gnu)
#   HOD_X64_QEMU     the emulator (default: qemu-x86_64 on PATH)
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# <workspace>/hod/scripts or <workspace>/hod/.worktrees/<wt>/scripts.
ws="$here/../.."
[ -d "$ws/.build" ] || ws="$here/../../../.."
kiln="${HOD_X64_KILN:-$ws/.build/target-kiln/release/kiln++}"
target="${HOD_X64_TARGET:-x86_64-unknown-linux-gnu}"
libdir="${HOD_X64_LIBDIR:-$ws/.build/libcxx-x64/lib}"
sysroot="${HOD_X64_SYSROOT:-/usr/x86_64-linux-gnu}"
qemu="${HOD_X64_QEMU:-$(command -v qemu-x86_64 || echo qemu-x86_64)}"

args=("--target=$target")
out=""
links=1
expect_out=0
for a in "$@"; do
    if [ "$expect_out" = 1 ]; then
        out="$a"
        expect_out=0
        args+=("$a")
        continue
    fi
    case "$a" in
        -L*/aarch64-linux-gnu) args+=("-L$libdir") ;;
        -o) expect_out=1; args+=("$a") ;;
        -o?*) out="${a#-o}"; args+=("$a") ;;
        -c | -S | -E | -fsyntax-only | --precompile | -shared | -r | -M | -MM)
            links=0; args+=("$a") ;;
        *) args+=("$a") ;;
    esac
done

if [ "$links" = 0 ] || [ -z "$out" ] || [ "$out" = /dev/null ]; then
    exec "$kiln" "${args[@]}"
fi

# An executable: link it beside <out>, then make <out> the qemu launcher.
case "$out" in
    /*) abs="$out" ;;
    *) abs="$PWD/$out" ;;
esac
real="$abs.x86_64"
for i in "${!args[@]}"; do
    if [ "${args[$i]}" = "-o" ]; then
        args[$((i + 1))]="$real"
    elif [ "${args[$i]}" = "-o$out" ]; then
        args[i]="-o$real"
    fi
done
rm -f "$abs"
"$kiln" "${args[@]}"
q() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }
printf '#!/bin/sh\nexec %s -L %s -0 "$0" %s "$@"\n' "$(q "$qemu")" "$(q "$sysroot")" "$(q "$real")" > "$abs"
chmod +x "$abs"
