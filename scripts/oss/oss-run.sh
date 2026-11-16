#!/usr/bin/env bash
# oss-run.sh <project>... [--flavours "O0 O0g"] [--modes "llc hod strict"] [--target aarch64|x86_64] [--strict] [--keep]
#
# The lane's whole procedure for each project (spec/harness.md "Real
# projects"), one build at a time: for each flavour, the hybrid build
# (`--mode hod`, the census), the control (`--mode llc`), and the strict
# build (`--mode strict`) when the hybrid build fell back nowhere; then the
# refused modules are kept as findings (oss-report.py collect), the build
# trees pruned to their logs and reports (unless --keep), and the census
# page rewritten (<findings>/CENSUS.md). Source scripts/env.sh first.
# `--target x86_64` (WP-191) runs the cross lane (oss-build.sh --target):
# its trees under <root>/x64-build/, its findings under <findings>/x64/,
# its census page <findings>/X64-CENSUS.md. `--strict` runs the strict
# build even when the hybrid build fell back (its compiles of the refused
# modules fail, `make -k` builds the rest and the tests that link run).
# Each kept refusal is then reduced to its smallest module (oss-reduce.py).
# `--modes` runs only the modes named (a rerun with a new hod keeps the
# control's reports: `--modes "hod strict"`, or `--modes strict --strict`).
#
# `HOD_OSS_KEEP_MODULES=<absolute dir>` (opt-in, WP-272) keeps every
# module the hod and strict builds hand hod, with its arguments, under
# <dir>/<target>/<project>/<flavour>/ (oss-build.sh's header), for `cargo
# xtask compare-binaries --modules <dir>`.
#
# It runs from a copy of scripts/oss taken when it starts
# (<root>/lane-<pid>/, removed at exit), and bash parses this file whole
# before running it (the braces): editing the lane while a run is going
# never changes the running one.
set -uo pipefail
{
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${CARGO_TARGET_DIR:?oss-run: source scripts/env.sh first}"
BROOT="${HOD_BUILD_ROOT:-$(dirname "$CARGO_TARGET_DIR")}"
OSS="${HOD_OSS_ROOT:-$BROOT/oss}"
FINDINGS="${HOD_OSS_FINDINGS:-$BROOT/findings/oss}"
# The real-project jobserver pool for everything this run starts (as
# oss-build.sh; spec/harness.md "Job slots").
LANE_FIFO="${HOD_OSS_JOBSERVER_FIFO:-/run/hod/lanes.fifo}"
[ -p "$LANE_FIFO" ] && export MAKEFLAGS="--jobserver-auth=fifo:$LANE_FIFO"
FLAVOURS="O0 O0g" TARGET=aarch64 MODES="llc hod strict" STRICT="" KEEP="" PROJECTS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --flavours) FLAVOURS="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    --strict) STRICT=1; shift ;;
    --modes) MODES="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    -*) echo "oss-run: unknown option $1" >&2; exit 2 ;;
    *) PROJECTS+=("$1"); shift ;;
  esac
done
[ ${#PROJECTS[@]} -gt 0 ] || { sed -n '2p' "$0" | sed 's/^# //' >&2; exit 2; }
case "$TARGET" in
  aarch64) TP="" CENSUS="$FINDINGS/CENSUS.md" REGS="$FINDINGS/TEST-REGRESSIONS.txt" ;;
  x86_64) TP="x64-" CENSUS="$FINDINGS/X64-CENSUS.md" REGS="$FINDINGS/x64/TEST-REGRESSIONS.txt" ;;
  *) echo "oss-run: --target must be aarch64 or x86_64" >&2; exit 2 ;;
esac
mkdir -p "$FINDINGS" "$(dirname "$REGS")" "$OSS"
SNAP="$OSS/lane-$$"
mkdir "$SNAP" && cp -p "$HERE"/oss-build.sh "$HERE"/oss-report.py "$HERE"/projects.py "$HERE"/projects.toml \
  "$HERE"/kiln-cxx "$HERE"/kiln-cc "$HERE"/hod-shim "$HERE"/oss-reduce.py "$SNAP"/ || exit 2
# The list of expected results (spec/harness.md "Expected results"), read
# by oss-report.py from beside itself.
cp -p "$HERE/../../docs/status/oss-expected.tsv" "$SNAP/oss-expected.tsv" || exit 2
[ ! -d "$HERE/shims" ] || cp -Rp "$HERE/shims" "$SNAP/shims" || exit 2
trap 'find "$SNAP" -mindepth 1 -delete; rmdir "$SNAP"' EXIT
HERE="$SNAP"
# Where a crash's dump would go and the core limit the lane inherited
# (WP-268; every phase sets its own to 1 byte): read only.
echo "oss-run: core_pattern $(cat /proc/sys/kernel/core_pattern 2>/dev/null || echo '?'); core limit soft/hard (bytes) $(python3 -c 'import resource; s, h = resource.getrlimit(resource.RLIMIT_CORE); f = lambda v: "unlimited" if v == resource.RLIM_INFINITY else str(v); print(f(s) + "/" + f(h))' 2>/dev/null || echo '?')"

fell_back() {  # the hybrid report's fallbacks: declined + failed (not offered is kiln's choice)
  python3 -c "import json,sys; c=json.load(open(sys.argv[1]))['hod'] or {}; b=c.get('backends',{}); print(b.get('hod-declined',0)+b.get('hod-failed',0))" "$1" 2>/dev/null || echo 1
}

# Build one mode; a hod or strict tree whose tests do worse than the
# control's, by a test the list of expected results does not name, keeps
# its objects (the finding), any other is pruned. The regressions file
# (and TEST-EXPECTED.txt, TEST-STALE.txt beside it) are rewritten from
# every report by `oss-report.py check` after each project and flavour.
one() {
  local p="$1" fl="$2" m="$3"
  # The machine's state as each build starts (WP-268): a runner that slows
  # from one build to the next shows it here.
  local mem disk
  mem="$(awk '/^MemAvailable:/ {printf "%d", $2 / 1024}' /proc/meminfo 2>/dev/null)"
  disk="$(df -Pk "$OSS" 2>/dev/null | awk 'NR == 2 {printf "%d", $4 / 1048576}')"
  echo "oss-run: $p $fl $m ($(date -u +%H:%M:%S); load $(cut -d' ' -f1-3 /proc/loadavg 2>/dev/null), ${mem:-?} MB available, ${disk:-?} GB free)"
  if [ "$m" = llc ]; then
    local prune=(--prune); [ -z "$KEEP" ] || prune=()
    "$HERE/oss-build.sh" "$p" --mode llc --flavour "$fl" --target "$TARGET" "${prune[@]}"
    return
  fi
  "$HERE/oss-build.sh" "$p" --mode "$m" --flavour "$fl" --target "$TARGET"
  local w; w="$(python3 "$HERE/oss-report.py" --target "$TARGET" unexpected "$p" "$fl" "$m")"
  if [ -n "$w" ]; then
    echo "oss-run: $p $fl $m: tests passing under llc and not here (not expected): $w (tree kept)"
  elif [ -z "$KEEP" ]; then
    "$HERE/oss-build.sh" "$p" --mode "$m" --flavour "$fl" --target "$TARGET" --prune-only
  fi
}

for p in "${PROJECTS[@]}"; do
  for fl in $FLAVOURS; do
    case " $MODES " in *" llc "*) one "$p" "$fl" llc ;; esac
    case " $MODES " in *" hod "*) one "$p" "$fl" hod ;; esac
    case " $MODES " in
      *" strict "*)
        n="$(fell_back "$OSS/${TP}build/$p/$fl-hod/report.json")"
        if [ "$n" = 0 ] || [ -n "$STRICT" ]; then
          one "$p" "$fl" strict
        else
          echo "oss-run: $p $fl: $n objects hod declined or failed in the hybrid build: no strict build"
        fi ;;
    esac
    python3 "$HERE/oss-report.py" --target "$TARGET" check > /dev/null  # rewrites $REGS; its status is the summary's
    python3 "$HERE/oss-report.py" --target "$TARGET" collect "$p"
    python3 "$HERE/oss-reduce.py" "$(dirname "$REGS")" --project "$p" --hod "${HOD_OSS_HOD:-$CARGO_TARGET_DIR/release/hod}"
    python3 "$HERE/oss-report.py" --target "$TARGET" census --out "$CENSUS" > /dev/null
  done
done
exit 0
}
