#!/usr/bin/env bash
# libc++, libc++abi and libunwind 22.1.8 for i686-linux-gnu, as static
# archives, built by clang 22 (LLVM's own code generator: this is the
# CONTROL library of the i386 libc++ lane, never built by hod). WP-330;
# docs/status/libcxx-hod-i686.md, design 0008 section 4.5 (second route:
# the container has no i386 libc++ package and no i386 architecture).
#
# Usage:
#   scripts/build-libcxx-i686.sh [--force] <work dir>
#
# <work dir> receives src/ (a shallow, blob-filtered, sparse clone of
# llvm-project at llvmorg-22.1.8 holding the runtimes' sources: the test
# tree under .toolchain/libcxx-llvmorg-22.1.8 has libcxx/test and
# libcxx/utils only), build/ (the cmake build tree, which may be deleted
# once installed) and build.log. The install goes to $HOD_I686_LIBCXX_ROOT
# (default <workspace>/.toolchain/libcxx-22-i686-root): include/c++/v1
# (headers with the i686 __config_site) and lib/ (libc++.a,
# libc++abi.a, libc++experimental.a, libunwind.a).
#
# The configuration matches the __config_site of Debian's libc++-22-dev
# (ABI version 1, threads with pthreads, the time zone database, the
# std_thread PSTL back end, hardening none): every one of those is the
# cmake default for Linux and is written out below so that a changed
# default cannot drift it. libc++abi does not embed the unwinder (Debian's
# archive does not either): a program gets _Unwind_* from libgcc as the
# 64-bit lanes do, or from libunwind.a when it asks for it.
#
# A complete install (the four archives and __config_site) is not
# rebuilt unless --force is given.
#
# Environment:
#   HOD_I686_LIBCXX_ROOT  the install prefix (see above)
#   HOD_JOBS              ninja -j (default 4)
#   HOD_CLANG_BIN         the clang 22 bin directory (default /usr/lib/llvm-22/bin)
#   LLVM_REPO             the llvm-project remote (default the GitHub one)
set -euo pipefail
here="${BASH_SOURCE[0]%/*}"
[ "$here" = "${BASH_SOURCE[0]}" ] && here=.
# <workspace>/hod/scripts or <workspace>/hod/.worktrees/<wt>/scripts.
ws="$here/../.."
[ -d "$ws/.build" ] || ws="$here/../../../.."
force=0
if [ "${1:-}" = --force ]; then
    force=1
    shift
fi
work="${1:?usage: build-libcxx-i686.sh [--force] <work dir>}"
tag=llvmorg-22.1.8
prefix="${HOD_I686_LIBCXX_ROOT:-$ws/.toolchain/libcxx-22-i686-root}"
bin="${HOD_CLANG_BIN:-/usr/lib/llvm-22/bin}"
repo="${LLVM_REPO:-https://github.com/llvm/llvm-project.git}"
jobs="${HOD_JOBS:-4}"
triple=i686-linux-gnu

complete=1
for f in lib/libc++.a lib/libc++abi.a lib/libc++experimental.a lib/libunwind.a \
    include/c++/v1/__config_site; do
    [ -f "$prefix/$f" ] || complete=0
done
if [ "$complete" = 1 ] && [ "$force" = 0 ]; then
    echo "$prefix already holds a complete build (--force rebuilds it)" >&2
    exit 1
fi
for tool in cmake ninja git; do
    command -v "$tool" > /dev/null || { echo "no $tool on PATH" >&2; exit 2; }
done
for tool in clang clang++ llvm-ar llvm-ranlib; do
    [ -x "$bin/$tool" ] || { echo "no $bin/$tool" >&2; exit 2; }
done
[ -d /usr/$triple/include ] || { echo "no /usr/$triple (libc6-dev-i386-cross)" >&2; exit 2; }

mkdir -p "$work"
src="$work/src"
if [ ! -d "$src/.git" ]; then
    [ -e "$src" ] && { echo "$src exists and is not a clone: remove it first" >&2; exit 2; }
    git clone --quiet --depth 1 --filter=blob:none --no-checkout --branch "$tag" "$repo" "$src"
fi
# libc++'s charconv includes llvm-libc's shared floating-point code.
git -C "$src" sparse-checkout set --no-cone /libcxx/ /libcxxabi/ /libunwind/ /libc/ \
    /runtimes/ /cmake/ /llvm/cmake/ /llvm/utils/llvm-lit/ /llvm/utils/lit/ /third-party/
git -C "$src" checkout --quiet "$tag"

build="$work/build"
log="$work/build.log"
start=$SECONDS
{
    date
    nice cmake -G Ninja -S "$src/runtimes" -B "$build" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$prefix" \
        -DCMAKE_SYSTEM_NAME=Linux \
        -DCMAKE_SYSTEM_PROCESSOR=i686 \
        -DCMAKE_C_COMPILER="$bin/clang" \
        -DCMAKE_CXX_COMPILER="$bin/clang++" \
        -DCMAKE_ASM_COMPILER="$bin/clang" \
        -DCMAKE_C_COMPILER_TARGET=$triple \
        -DCMAKE_CXX_COMPILER_TARGET=$triple \
        -DCMAKE_ASM_COMPILER_TARGET=$triple \
        -DCMAKE_AR="$bin/llvm-ar" \
        -DCMAKE_RANLIB="$bin/llvm-ranlib" \
        -DLLVM_DEFAULT_TARGET_TRIPLE=$triple \
        -DLLVM_ENABLE_RUNTIMES="libcxx;libcxxabi;libunwind" \
        -DLLVM_ENABLE_PER_TARGET_RUNTIME_DIR=OFF \
        -DLLVM_INCLUDE_TESTS=OFF \
        -DLIBCXX_INCLUDE_TESTS=OFF \
        -DLIBCXX_INCLUDE_BENCHMARKS=OFF \
        -DLIBCXXABI_INCLUDE_TESTS=OFF \
        -DLIBUNWIND_INCLUDE_TESTS=OFF \
        -DLIBCXX_ENABLE_SHARED=OFF \
        -DLIBCXXABI_ENABLE_SHARED=OFF \
        -DLIBUNWIND_ENABLE_SHARED=OFF \
        -DLIBCXX_ENABLE_STATIC=ON \
        -DLIBCXXABI_ENABLE_STATIC=ON \
        -DLIBUNWIND_ENABLE_STATIC=ON \
        -DLIBCXX_ENABLE_EXCEPTIONS=ON \
        -DLIBCXX_ENABLE_RTTI=ON \
        -DLIBCXXABI_ENABLE_EXCEPTIONS=ON \
        -DLIBCXX_ABI_VERSION=1 \
        -DLIBCXX_ENABLE_THREADS=ON \
        -DLIBCXX_HAS_PTHREAD_API=OFF \
        -DLIBCXX_ENABLE_TIME_ZONE_DATABASE=ON \
        -DLIBCXX_PSTL_BACKEND=std_thread \
        -DLIBCXX_HARDENING_MODE=none \
        -DLIBCXX_ENABLE_STATIC_ABI_LIBRARY=OFF \
        -DLIBCXX_CXX_ABI=libcxxabi \
        -DLIBCXXABI_USE_LLVM_UNWINDER=ON \
        -DLIBCXXABI_STATICALLY_LINK_UNWINDER_IN_STATIC_LIBRARY=OFF \
        -DLIBCXX_INSTALL_MODULES=OFF
    nice ninja -C "$build" -j "$jobs" install
    date
} > "$log" 2>&1
echo "built and installed into $prefix in $((SECONDS - start)) s (log: $log)"
du -sh "$prefix" "$build"
