#!/bin/sh
# kiln++ with debug information, for kiln's `cargo xtask libcxx --compiler`
# (WP-210; docs/status/libcxx-hod-debug.md). kiln's runner has no option
# for extra compile flags; this wrapper puts HOD_DEBUG_FLAGS (default `-g`)
# before the runner's own arguments on every invocation (compiles, links,
# the runner's flag probes) and runs the next compiler of the chain, so a
# flag a test sets itself still wins. It only adds flags: the chains it
# fronts are the existing ones, unchanged.
#
#   control, AArch64   libcxx-debug-kiln.sh -> kiln++
#   hod, AArch64       libcxx-debug-kiln.sh -> libcxx-hod-kiln.sh -> <shim>/kiln++-hod
#   control, x86-64    libcxx-debug-kiln.sh -> libcxx-x64-kiln.sh -> kiln++
#   hod, x86-64        libcxx-debug-kiln.sh -> libcxx-x64-kiln.sh
#                        -> libcxx-hod-kiln.sh -> <shim>/kiln++-hod
#
# scripts/libcxx-hod-debug.sh sets the chain up.
#
# Environment:
#   HOD_DEBUG_NEXT   the next compiler of the chain (required)
#   HOD_DEBUG_FLAGS  the flags to add, split at blanks (default when unset:
#                    -g; set and empty: none, the non -g control of WP-233)
set -eu
: "${HOD_DEBUG_NEXT:?set HOD_DEBUG_NEXT to the compiler this wrapper runs}"
# shellcheck disable=SC2086
exec "$HOD_DEBUG_NEXT" ${HOD_DEBUG_FLAGS--g} "$@"
