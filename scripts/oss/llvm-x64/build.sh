#!/bin/bash
# Build targets in one tree (WP-180, from WP-178): build.sh hod|llc <stage> <target>...
#
# ninja -j 6 (the cap for this machine), links one at a time (the
# configure's LLVM_PARALLEL_LINK_JOBS=1), -k 0 so one failing TU does not
# hide the rest. Appends "<stage> <start> <end> <rc>" to logs/stages.txt
# and the ninja output to logs/build-<stage>.log.
set -uo pipefail
source "$(dirname "$0")/env.sh"
flavour="${1:?usage: build.sh hod|llc <stage> <target>...}"
stage="${2:?stage name}"
shift 2
tree="$HOD_OSS_ROOT/$HOD_X64_PREFIX$flavour"
export HOD_OSS_PHASE=build
start=$(date +%s)
nice -n 10 ninja -C "$tree/build" -j "${HOD_OSS_JOBS:-6}" -k 0 "$@" > "$tree/logs/build-$stage.log" 2>&1
rc=$?
end=$(date +%s)
echo "$stage $start $end $rc $*" >> "$tree/logs/stages.txt"
echo "$flavour $stage rc=$rc $((end - start)) s"
exit $rc
