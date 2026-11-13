#!/bin/bash
# Configure one tree of the x86-64 cross build of LLVM (WP-180, from
# WP-178's scripts/oss/llvm/configure.sh): configure.sh hod|llc [hod]
#
#   hod  $HOD_OSS_ROOT/llvm-x64-hod: kiln with hod (fallback allowed, census on)
#   llc  $HOD_OSS_ROOT/llvm-x64-llc: kiln with its LLVM back end, the control
#
# Both from WP-178's source tree ($HOD_OSS_SRC, llvmorg-19.1.7, read
# only), with WP-178's flags (-O0, assertions, no -g, static libraries)
# and the cross settings: the compilers are the wrappers with
# --target=$HOD_X64_TARGET, the host tools (llvm-tblgen, llvm-min-tblgen)
# come from $HOD_X64_NATIVE_TOOLS (a frozen copy of WP-178's native tree's), and CMake runs
# target programs under $HOD_X64_QEMU -L $HOD_X64_SYSROOT. The three host
# tools WP-178's tree does not build (llvm-config, llvm-nm, llvm-readobj;
# without them CMake would configure a NATIVE sub-build with the host's
# default compiler) are the distribution's LLVM 19 ($HOD_X64_HOST_LLVM). For hod, the
# second argument is the hod executable to freeze into the tree's bin/
# (default: the release hod of this checkout's target directory).
set -euo pipefail
source "$(dirname "$0")/env.sh"
flavour="${1:?usage: configure.sh hod|llc [hod]}"
case "$flavour" in hod | llc) ;; *) echo "configure.sh: hod or llc" >&2; exit 2 ;; esac
tree="$HOD_OSS_ROOT/$HOD_X64_PREFIX$flavour"
mkdir -p "$tree/logs" "$tree/bin"
for n in kiln-$flavour-cxx kiln-$flavour-cc; do ln -sf "$HOD_OSS_SCRIPTS/wrap.sh" "$tree/bin/$n"; done
if [ "$flavour" = hod ]; then
  hod="${2:-${CARGO_TARGET_DIR:?source scripts/env.sh or name the hod executable}/release/hod}"
  cp "$hod" "$tree/bin/hod"
  "$tree/bin/hod" --version > "$tree/logs/hod-version.txt"
fi
"$HOD_OSS_KILN/kiln++" --version > "$tree/logs/kiln-version.txt"
mkdir -p "$HOD_X64_NATIVE_TOOLS"
for t in llvm-tblgen llvm-min-tblgen; do
  if [ ! -x "$HOD_X64_NATIVE_TOOLS/$t" ]; then
    [ -x "$HOD_X64_NATIVE_FROM/$t" ] || { echo "configure.sh: no $HOD_X64_NATIVE_FROM/$t" >&2; exit 2; }
    cp "$HOD_X64_NATIVE_FROM/$t" "$HOD_X64_NATIVE_TOOLS/$t"
  fi
done
export HOD_OSS_PHASE=configure
cd "$tree"
nice -n 10 cmake --fresh -S "$HOD_OSS_SRC/llvm" -B "$tree/build" -G Ninja \
  -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=x86_64 \
  -DCMAKE_CROSSCOMPILING_EMULATOR="$HOD_X64_QEMU;-L;$HOD_X64_SYSROOT" \
  -DCMAKE_FIND_ROOT_PATH="$HOD_X64_SYSROOT" \
  -DCMAKE_FIND_ROOT_PATH_MODE_PROGRAM=NEVER \
  -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=ONLY \
  -DCMAKE_FIND_ROOT_PATH_MODE_PACKAGE=ONLY \
  -DLLVM_HOST_TRIPLE="$HOD_X64_TARGET" \
  -DLLVM_DEFAULT_TARGET_TRIPLE="$HOD_X64_TARGET" \
  -DLLVM_NATIVE_TOOL_DIR="$HOD_X64_NATIVE_TOOLS" \
  -DLLVM_TABLEGEN="$HOD_X64_NATIVE_TOOLS/llvm-tblgen" \
  -DLLVM_CONFIG_PATH="$HOD_X64_HOST_LLVM/llvm-config" \
  -DLLVM_NM="$HOD_X64_HOST_LLVM/llvm-nm" -DLLVM_READOBJ="$HOD_X64_HOST_LLVM/llvm-readobj" \
  -DCMAKE_CXX_COMPILER="$tree/bin/kiln-$flavour-cxx" \
  -DCMAKE_C_COMPILER="$tree/bin/kiln-$flavour-cc" \
  -DCMAKE_CXX_STANDARD=17 \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_CXX_FLAGS_RELEASE=-O0 -DCMAKE_C_FLAGS_RELEASE=-O0 \
  -DLLVM_TARGETS_TO_BUILD="X86;AArch64" \
  -DLLVM_ENABLE_PROJECTS="" \
  -DLLVM_ENABLE_ASSERTIONS=ON \
  -DLLVM_INCLUDE_TESTS=ON \
  -DLLVM_INCLUDE_BENCHMARKS=OFF -DLLVM_INCLUDE_EXAMPLES=OFF \
  -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_LIBXML2=OFF \
  -DLLVM_ENABLE_LIBEDIT=OFF -DLLVM_ENABLE_LIBPFM=OFF \
  -DLLVM_USE_LINKER="${HOD_OSS_LINKER:-mold}" \
  -DLLVM_PARALLEL_LINK_JOBS=1 \
  -DLLVM_LIT_ARGS="-sv -j 4" \
  > "$tree/logs/configure.log" 2>&1
# The configure's own compiles (try_compile) are not the build's: keep
# their statistics and coverage lines apart (tu.jsonl has a phase field).
for f in stats.jsonl coverage.tsv; do
  if [ -f "$tree/logs/$f" ]; then cat "$tree/logs/$f" >> "$tree/logs/configure-$f" && rm -f "$tree/logs/$f"; fi
done
echo "configured $tree/build"
