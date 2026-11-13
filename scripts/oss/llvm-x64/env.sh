# Sourced by the WP-180 scripts (the x86-64 cross build of LLVM; copied
# from WP-178's scripts/oss/llvm): where things are. Every path can be
# overridden from the environment.
_here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_wt="$(cd "$_here/../../.." && pwd)"
# The workspace that holds hod (a submodule) and kiln beside it: the
# superproject of hod's main checkout.
_common="$(git -C "$_wt" rev-parse --path-format=absolute --git-common-dir)"
case "$_common" in
  */.git/modules/*) _ws="${_common%%/.git/modules/*}" ;;
  *) _ws="$(cd "$_common/../.." && pwd)" ;;
esac
unset _common
if [ ! -d "$_ws/kiln" ] && [ -z "${HOD_OSS_ROOT:-}" ]; then
  echo "env.sh: no kiln/ in $_ws: set HOD_OSS_ROOT, HOD_OSS_KILN, HOD_OSS_KILN_SCRIPTS" >&2
  return 2 2>/dev/null || exit 2
fi
export HOD_OSS_ROOT="${HOD_OSS_ROOT:-$_ws/.build/oss}"
export HOD_OSS_KILN="${HOD_OSS_KILN:-$_ws/.build/target-kiln/release}"
export HOD_OSS_KILN_SCRIPTS="${HOD_OSS_KILN_SCRIPTS:-$_ws/kiln/scripts/oss}"
export HOD_OSS_SCRIPTS="$_here"
export HOD_OSS_SRC="${HOD_OSS_SRC:-$HOD_OSS_ROOT/llvm-src}"
export HOD_OSS_FINDINGS="${HOD_OSS_FINDINGS:-$_ws/.build/findings/oss}"
LLVM_TAG=llvmorg-19.1.7
# The cross target: kiln is given --target=$HOD_X64_TARGET and finds the
# distribution's cross GCC installation (/usr/lib/gcc-cross) and its
# system root by itself; programs run under $HOD_X64_QEMU -L $HOD_X64_SYSROOT.
export HOD_X64_TARGET="${HOD_X64_TARGET:-x86_64-unknown-linux-gnu}"
export HOD_X64_SYSROOT="${HOD_X64_SYSROOT:-/usr/x86_64-linux-gnu}"
export HOD_X64_QEMU="${HOD_X64_QEMU:-$(command -v qemu-x86_64 || echo qemu-x86_64)}"
# The host tools of the build (llvm-tblgen, llvm-min-tblgen): a frozen
# copy (configure.sh makes it) of those of WP-178's native tree
# ($HOD_X64_NATIVE_FROM), which is rebuilt while it works.
export HOD_X64_NATIVE_FROM="${HOD_X64_NATIVE_FROM:-$HOD_OSS_ROOT/llvm-hod/build/bin}"
export HOD_X64_NATIVE_TOOLS="${HOD_X64_NATIVE_TOOLS:-$HOD_OSS_ROOT/${HOD_X64_PREFIX:-llvm-x64-}native/bin}"
export HOD_X64_HOST_LLVM="${HOD_X64_HOST_LLVM:-/usr/lib/llvm-19/bin}"
# The trees are $HOD_OSS_ROOT/llvm-x64-hod and llvm-x64-llc.
export HOD_X64_PREFIX="${HOD_X64_PREFIX:-llvm-x64-}"
unset _here _wt _ws
