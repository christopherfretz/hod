#!/bin/bash
# Run lit slices in both trees and compare (WP-180, from WP-178):
#   lit.sh <flavour: hod|llc|both> <slice>...
# A slice is a path under build/ of a tree's lit suite, e.g. test/CodeGen/X86,
# test/MC, test (all of check-llvm's lit tests), or <name>=@<list>: the
# tests named in <list> (paths relative to llvm/test of the source tree,
# one per line; build/bin/llvm-lit maps them to the tree's suite).
# Under qemu-x86_64 the tree's tools must be wrapped first (qemu-tools.sh). Each runs as
# `llvm-lit -sv -j 4 -o <json>` under `ulimit -v 16000000` (qemu included) and a 3 h
# timeout; the result of each test is written as "<code> <name>" to
# logs/lit-<slice>.txt of its tree (slice with / as _). With both trees'
# lists present, logs/lit-<slice>.diff in the hod tree is the comparison
# and a summary line is printed: the tests that pass in the control and
# fail with hod are the findings.
set -uo pipefail
source "$(dirname "$0")/env.sh"
which="${1:?usage: lit.sh hod|llc|both <slice>...}"
shift
case "$which" in both) trees="hod llc" ;; hod | llc) trees=$which ;; *) exit 2 ;; esac
for slice in "$@"; do
  case "$slice" in
    *=@*) key="${slice%%=@*}"; mapfile -t paths < <(sed "s#^#$HOD_OSS_SRC/llvm/test/#" "${slice#*=@}") ;;
    *) key=$(echo "$slice" | tr / _); paths=() ;;
  esac
  for f in $trees; do
    b="$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/build"
    js="$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/logs/lit-$key.json"
    rm -f "$js"
    start=$(date +%s)
    (ulimit -v 16000000 && timeout 10800 nice -n 10 python3 "$b/bin/llvm-lit" -sv -j 4 -o "$js" "${paths[@]:-$b/$slice}" \
      > "$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/logs/lit-$key.log" 2>&1)
    rc=$?
    echo "lit-$key $start $(date +%s) $rc" >> "$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/logs/stages.txt"
    python3 - "$js" > "$HOD_OSS_ROOT/$HOD_X64_PREFIX$f/logs/lit-$key.txt" <<'EOF'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception as e:
    print(f"# no json: {e}")
    sys.exit(0)
for t in sorted(d.get("tests", []), key=lambda t: t["name"]):
    print(f"{t['code']} {t['name']}")
EOF
  done
  h="$HOD_OSS_ROOT/${HOD_X64_PREFIX}hod/logs/lit-$key.txt" c="$HOD_OSS_ROOT/${HOD_X64_PREFIX}llc/logs/lit-$key.txt"
  if [ -f "$h" ] && [ -f "$c" ]; then
    python3 - "$c" "$h" > "$HOD_OSS_ROOT/${HOD_X64_PREFIX}hod/logs/lit-$key.diff" <<'EOF'
import collections, sys
def load(p):
    r = {}
    for line in open(p):
        if line.startswith("#"):
            continue
        code, name = line.rstrip("\n").split(" ", 1)
        r[name] = code
    return r
c, h = load(sys.argv[1]), load(sys.argv[2])
good = {"PASS", "XFAIL", "UNSUPPORTED"}
for n in sorted(set(c) | set(h)):
    cc, hc = c.get(n, "MISSING"), h.get(n, "MISSING")
    if cc != hc:
        tag = "REGRESSION" if cc in good and hc not in good else "differ"
        print(f"{tag} control={cc} hod={hc} {n}")
EOF
    summ() { awk '!/^#/{n[$1]++} END{for(k in n) printf "%s %d ", k, n[k]}' "$1"; }
    echo "$slice: control: $(summ "$c")| hod: $(summ "$h")| regressions $(grep -c '^REGRESSION' "$HOD_OSS_ROOT/${HOD_X64_PREFIX}hod/logs/lit-$key.diff"), other differences $(grep -c '^differ' "$HOD_OSS_ROOT/${HOD_X64_PREFIX}hod/logs/lit-$key.diff")"
  fi
done
