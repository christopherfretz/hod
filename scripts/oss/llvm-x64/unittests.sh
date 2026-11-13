#!/bin/bash
# Run gtest unit-test binaries of both trees under qemu-x86_64 and compare
# (WP-180, from WP-178):
#   unittests.sh <stage> <binary path relative to build/>...
# e.g. unittests.sh s1 unittests/Support/SupportTests unittests/ADT/ADTTests
#
# Each binary runs from its own directory with --gtest_output=json, under
# `ulimit -v 8000000` (qemu's own address space included) and a two-hour
# timeout (HOD_X64_UNIT_TIMEOUT); the result of each test is
# written as "<suite>.<name> <PASS|FAIL|SKIP>" to
# logs/unit-<stage>-<binary>.txt of its tree, and the two trees' lists
# are compared (logs/unit-<stage>-<binary>.diff in the hod tree; empty =
# the same). With HOD_OSS_DELETE=1 each binary is deleted after its run.
set -uo pipefail
source "$(dirname "$0")/env.sh"
stage="${1:?usage: unittests.sh <stage> <binary>...}"
shift
for rel in "$@"; do
  name=$(basename "$rel")
  for f in hod llc; do
    b="$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/build/$rel"
    lst="$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/logs/unit-$stage-$name.txt"
    if [ ! -x "$b" ]; then
      echo "MISSING $b" > "$lst"
      continue
    fi
    js="$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/logs/unit-$stage-$name.json"
    rm -f "$js"
    (cd "$(dirname "$b")" && ulimit -v 8000000 && timeout "${HOD_X64_UNIT_TIMEOUT:-7200}" nice -n 10 "$HOD_X64_QEMU" -L "$HOD_X64_SYSROOT" "$b" --gtest_output="json:$js" \
      > "$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/logs/unit-$stage-$name.log" 2>&1)
    rc=$?
    python3 - "$js" "$rc" > "$lst" <<'EOF'
import json, sys
js, rc = sys.argv[1], sys.argv[2]
print(f"# exit {rc}")
try:
    d = json.load(open(js))
except Exception as e:
    print(f"# no json: {e}")
    sys.exit(0)
for s in d.get("testsuites", []):
    for t in s.get("testsuite", []):
        st = "FAIL" if t.get("failures") else ("SKIP" if t.get("result") == "SKIPPED" or t.get("status") == "NOTRUN" else "PASS")
        print(f"{s['name']}.{t['name']} {st}")
EOF
    [ "${HOD_OSS_DELETE:-0}" = 1 ] && rm -f "$b"
  done
  h="$HOD_OSS_ROOT/${HOD_X64_PREFIX}hod/logs/unit-$stage-$name.txt"
  c="$HOD_OSS_ROOT/${HOD_X64_PREFIX}llc/logs/unit-$stage-$name.txt"
  diff "$c" "$h" > "$HOD_OSS_ROOT/${HOD_X64_PREFIX}hod/logs/unit-$stage-$name.diff"
  echo "$name: hod $(grep -c ' PASS$' "$h") pass $(grep -c ' FAIL$' "$h") fail $(head -1 "$h");" \
    "control $(grep -c ' PASS$' "$c") pass $(grep -c ' FAIL$' "$c") fail $(head -1 "$c");" \
    "diff lines $(wc -l < "$HOD_OSS_ROOT/${HOD_X64_PREFIX}hod/logs/unit-$stage-$name.diff")"
done
