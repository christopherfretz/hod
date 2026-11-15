# Sourced by the WP-178 scripts: where things are. Every path can be
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
# Builds and tests draw from the real-project pool (WP-222; the brief's
# "Two token pools"): wrap.sh, lit.sh and unittests.sh take their tokens
# with lane-token.py from the fifo MAKEFLAGS names.
_lanes="${HOD_OSS_JOBSERVER_FIFO:-/run/hod/lanes.fifo}"
[ -p "$_lanes" ] && export MAKEFLAGS="--jobserver-auth=fifo:$_lanes"
unset _here _wt _ws _lanes
