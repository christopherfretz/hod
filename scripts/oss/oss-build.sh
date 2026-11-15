#!/usr/bin/env bash
# oss-build.sh <project> --mode llc|hod|strict [--flavour O0|O0g] [--target aarch64|x86_64]
#        [--no-test] [--jobs N] [--install PREFIX] [--prune | --prune-only]
#
# hod's real-project lane (docs/VALIDATION.md "Real projects (L11)",
# spec/harness.md "Real projects"). A port of kiln's scripts/oss/oss-build.sh
# (kiln:spec/landed/harness/WP-666.md): builds one row of
# scripts/oss/projects.toml through its own build system (cmake, or autotools) with kiln
# as the compiler (scripts/oss/kiln-cxx, and scripts/oss/kiln-cc for a
# `c = "kiln"` row), runs its tests, and writes
# <root>/build/<project>/<flavour>-<mode>/report.json. Changed from kiln's:
#
# - every build is at -O0: flavour O0 (`-O0 -g0`) or O0g (`-O0 -g`), the
#   flags appended to every compile by the wrapper ($HOD_OSS_CFLAGS) after
#   a Debug build type whose flags are the same, so the flavour wins
#   whatever the project sets (kiln hands a module to hod only at -O0);
# - three modes: `llc` (kiln with its LLVM back end, the control), `hod`
#   (kiln with hod, falling back to llc where hod declines; hod's census on,
#   HOD_CENSUS=1, and its coverage log, kiln's statistics log and
#   scripts/oss/hod-shim's log written into the build tree, every refused
#   module kept under declined/<reason>/), `strict` (as `hod`, with
#   KILN_HOD_STRICT=1 and the shim turning a decline into a failure: any
#   module hod does not build fails its compile);
# - configure runs with kiln's LLVM back end in every mode, so the three
#   builds of a project configure alike (a feature check hod declined must
#   not change what is built); the modes differ from the build on;
# - the tests run under the row's address-space cap too, ctest at most 4
#   at a time;
# - dependencies are built and installed with the same flavour and mode
#   (`hod` for a strict build), into <root>/install/<dep>-<flavour>-<mode>/
#   (their trees pruned);
# - `--prune` deletes the build tree but its logs, its report and the kept
#   modules afterwards; `--prune-only` does only that, building nothing.
#
# `--target` (default aarch64, the host: a native build) chooses what is
# built for. `x86_64` (WP-191) cross-builds for x86_64-unknown-linux-gnu
# on the AArch64 host: kiln-cxx and kiln-cc put `--target=<triple>` first
# in every kiln command (kiln finds the cross GCC and its system root
# itself), a C row's clang gets it through CMAKE_C_COMPILER_TARGET, CMake
# is told it cross-compiles (CMAKE_SYSTEM_NAME/PROCESSOR, the system root
# $HOD_OSS_X64_SYSROOT, default /usr/x86_64-linux-gnu) and runs target
# programs under CMAKE_CROSSCOMPILING_EMULATOR ($HOD_OSS_X64_QEMU -L
# <sysroot>, so ctest runs the tests under qemu), links get 16 GB of
# address space (KILN_OSS_LINK_VMEM_KB: mold maps every archive), the test
# timeout and ctest's per-test default (DART_TESTING_TIMEOUT, 1500 s, for
# a project that includes CTest) are times $HOD_OSS_X64_TEST_SLOWDOWN
# (default 4), the test step's address-space cap is
# $HOD_OSS_X64_TEST_VMEM_KB (default 16 GB: a guest thread is a host
# thread of qemu's), and the trees are <root>/x64-build/ and <root>/x64-install/ (the sources
# are shared).
#
# Environment (source scripts/env.sh first; make draws from the
# real-project jobserver pool, HOD_OSS_JOBSERVER_FIFO, default
# /run/hod/lanes.fifo, else the shared one): KILN_OSS_CXX (kiln++; default
# <build root>/target-kiln/release/kiln++), KILN_OSS_CC (the kiln C driver;
# default `kiln` beside kiln++), KILN_RESOURCE_DIR (default
# <workspace>/kiln/resource when it exists), HOD_OSS_HOD (hod; default
# $CARGO_TARGET_DIR/release/hod), HOD_OSS_ROOT (default <build root>/oss),
# KILN_OSS_VMEM_KB (the cap, default the row's vmem_kb; the tests' is the
# row's test_vmem_kb unless KILN_OSS_VMEM_KB is set).
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
usage() { sed -n '2,3p' "$0" | sed 's/^# //' >&2; exit 2; }
: "${CARGO_TARGET_DIR:?oss-build: source scripts/env.sh first}"
BROOT="${HOD_BUILD_ROOT:-$(dirname "$CARGO_TARGET_DIR")}"
OSS="${HOD_OSS_ROOT:-$BROOT/oss}"
# Project builds (make, cmake --install, ctest) draw from the real-project
# pool, not the shared one that cargo and the gate use (spec/harness.md
# "Job slots"): $HOD_OSS_JOBSERVER_FIFO, default /run/hod/lanes.fifo when it
# exists; otherwise MAKEFLAGS stays as scripts/env.sh left it.
LANE_FIFO="${HOD_OSS_JOBSERVER_FIFO:-/run/hod/lanes.fifo}"
[ -p "$LANE_FIFO" ] && export MAKEFLAGS="--jobserver-auth=fifo:$LANE_FIFO"

PROJECT="" MODE="" FLAVOUR=O0 TARGET=aarch64 RUN_TEST=1 JOBS=4 INSTALL="" PRUNE="" PRUNE_ONLY=""
while [ $# -gt 0 ]; do
  case "$1" in
    --mode) MODE="${2:-}"; shift 2 ;;
    --mode=*) MODE="${1#*=}"; shift ;;
    --flavour) FLAVOUR="${2:-}"; shift 2 ;;
    --flavour=*) FLAVOUR="${1#*=}"; shift ;;
    --target) TARGET="${2:-}"; shift 2 ;;
    --target=*) TARGET="${1#*=}"; shift ;;
    --no-test) RUN_TEST=""; shift ;;
    --jobs) JOBS="${2:-}"; shift 2 ;;
    --jobs=*) JOBS="${1#*=}"; shift ;;
    --install) INSTALL="${2:-}"; RUN_TEST=""; shift 2 ;;
    --prune) PRUNE=1; shift ;;
    --prune-only) PRUNE_ONLY=1; shift ;;
    -h|--help) usage ;;
    -*) echo "oss-build: unknown option $1" >&2; usage ;;
    *) [ -z "$PROJECT" ] || usage; PROJECT="$1"; shift ;;
  esac
done
[ -n "$PROJECT" ] || usage
case "$MODE" in llc|hod|strict) ;; *) echo "oss-build: --mode must be llc, hod or strict" >&2; exit 2 ;; esac
case "$FLAVOUR" in
  O0) FLAGS="-O0 -g0" ;;
  O0g) FLAGS="-O0 -g" ;;
  *) echo "oss-build: --flavour must be O0 or O0g" >&2; exit 2 ;;
esac
# The target: where its trees live (TP, a prefix of build/ and install/)
# and what CMake is told (XCONF).
XCONF=()
case "$TARGET" in
  aarch64) TP="" ;;
  x86_64)
    TP="x64-"
    X64_TRIPLE=x86_64-unknown-linux-gnu
    X64_SYSROOT="${HOD_OSS_X64_SYSROOT:-/usr/x86_64-linux-gnu}"
    X64_QEMU="${HOD_OSS_X64_QEMU:-$(command -v qemu-x86_64 || echo qemu-x86_64)}"
    XCONF=(-DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=x86_64
      -DCMAKE_C_COMPILER_TARGET=x86_64-linux-gnu
      "-DCMAKE_CROSSCOMPILING_EMULATOR=$X64_QEMU;-L;$X64_SYSROOT"
      "-DCMAKE_FIND_ROOT_PATH=$X64_SYSROOT" -DCMAKE_FIND_ROOT_PATH_MODE_PROGRAM=NEVER
      -DDART_TESTING_TIMEOUT=$((1500 * ${HOD_OSS_X64_TEST_SLOWDOWN:-4})))
    export HOD_OSS_TARGET_TRIPLE="$X64_TRIPLE" KILN_OSS_LINK_VMEM_KB="${KILN_OSS_LINK_VMEM_KB:-16000000}" ;;
  *) echo "oss-build: --target must be aarch64 or x86_64" >&2; exit 2 ;;
esac
case "$JOBS" in ''|*[!0-9]*) echo "oss-build: --jobs takes a number" >&2; exit 2 ;; esac
[ "$JOBS" -le 4 ] || JOBS=4

# Keep the logs, the report and the kept modules of a tree; drop the rest.
prune() {
  find "$1" -mindepth 1 -depth -not -path "$1/declined*" \
    \( -type f -not -name '*.log' -not -name '*.jsonl' -not -name 'report.json' -o -type l \) -delete
  find "$1" -mindepth 1 -depth -type d -empty -delete
  echo "oss-build: pruned $1"
}
if [ -n "$PRUNE_ONLY" ]; then
  t="$OSS/${TP}build/$PROJECT/$FLAVOUR-$MODE"
  [ -d "$t" ] && prune "$t"
  exit 0
fi

CFG="$(python3 "$HERE/projects.py" shell "$PROJECT")" || exit 2
eval "$CFG"
# `@KILN_OSS@` in a configure flag or the test command is kiln's
# scripts/oss (its project shims, read where they are: the Boost rows'
# boost-jamfile-tests.cmake), $KILN_OSS_DIR, default <workspace>/kiln/scripts/oss.
KILN_OSS_DIR="${KILN_OSS_DIR:-$(dirname "$BROOT")/kiln/scripts/oss}"
P_CONFIGURE=("${P_CONFIGURE[@]//@KILN_OSS@/$KILN_OSS_DIR}")
P_INSTALL_CONFIGURE=("${P_INSTALL_CONFIGURE[@]//@KILN_OSS@/$KILN_OSS_DIR}")
P_TEST="${P_TEST//@KILN_OSS@/$KILN_OSS_DIR}"
# `@HOD_OSS@` is this lane's scripts/oss (its own shims/, copied with the
# lane by oss-run.sh), for a row's project include.
P_CONFIGURE=("${P_CONFIGURE[@]//@HOD_OSS@/$HERE}")
P_INSTALL_CONFIGURE=("${P_INSTALL_CONFIGURE[@]//@HOD_OSS@/$HERE}")

CXX="${KILN_OSS_CXX:-$BROOT/target-kiln/release/kiln++}"
[ -x "$CXX" ] || { echo "oss-build: no kiln++ at $CXX (set KILN_OSS_CXX)" >&2; exit 2; }
# The kiln commit the row is built with (its report's `kiln`): $HOD_OSS_KILN,
# else the workspace's kiln checkout, which target-kiln is built from.
KILN_ID="${HOD_OSS_KILN:-$(git -C "$(dirname "$BROOT")/kiln" rev-parse --short=9 HEAD 2>/dev/null)}"
CXX="$(cd "$(dirname "$CXX")" && pwd)/$(basename "$CXX")"
HOD="${HOD_OSS_HOD:-$CARGO_TARGET_DIR/release/hod}"
if [ "$MODE" != llc ]; then
  [ -x "$HOD" ] || { echo "oss-build: no hod at $HOD (cargo build --release -p hod_cli, or set HOD_OSS_HOD)" >&2; exit 2; }
  HOD="$(cd "$(dirname "$HOD")" && pwd)/$(basename "$HOD")"
fi
# A kiln++ built in a kiln checkout needs the checkout's resource
# directory; an installed one (kiln's dist) finds its own.
if [ -z "${KILN_RESOURCE_DIR:-}" ] && [ -d "$(dirname "$BROOT")/kiln/resource" ]; then
  export KILN_RESOURCE_DIR="$(dirname "$BROOT")/kiln/resource"
fi
# C TUs: kiln-cc over the kiln C driver for a `c = "kiln"` row, else the
# row's c_compiler directly (unlogged, the same in every mode).
CC_REAL="$(command -v "$P_C_COMPILER" || echo "$P_C_COMPILER")"
if [ "$P_C" = kiln ]; then
  CC_REAL="${KILN_OSS_CC:-$(dirname "$CXX")/kiln}"
  CC_REAL="$(command -v "$CC_REAL")" || { echo "oss-build: $PROJECT has c = \"kiln\" but there is no kiln C driver beside $CXX: set KILN_OSS_CC" >&2; exit 2; }
  P_C_COMPILER="$HERE/kiln-cc"
  export KILN_OSS_CC="$CC_REAL"
fi
export KILN_OSS_CXX="$CXX" HOD_OSS_CFLAGS="$FLAGS"
export HOD_OSS_DROP_FLAGS="$(printf '%s\n' "${P_DROP_FLAGS[@]}")"
echo "oss-build: $PROJECT $P_TAG, target $TARGET, mode $MODE, flavour $FLAVOUR ($FLAGS), kiln $CXX${HOD:+, hod $HOD}; C TUs (c = $P_C) with $CC_REAL"

# Dependencies: built with this flavour and mode, installed once; a
# strict build takes the hybrid build's (a dependency's refusals are its
# own row's census, and strict is about this project's modules).
PREFIXES=()
DEP_MODE="$MODE"; [ "$MODE" != strict ] || DEP_MODE=hod
for dep in "${P_DEPS[@]}"; do
  pre="$OSS/${TP}install/$dep-$FLAVOUR-$DEP_MODE"
  if [ ! -f "$pre/.oss-installed" ]; then
    echo "oss-build: dependency $dep -> $pre"
    "$0" "$dep" --mode "$DEP_MODE" --flavour "$FLAVOUR" --target "$TARGET" --jobs "$JOBS" --install "$pre" --prune || { echo "oss-build: dependency $dep failed" >&2; exit 1; }
  fi
  PREFIXES+=("$pre")
done
if [ -n "$INSTALL" ]; then
  P_CONFIGURE=("${P_INSTALL_CONFIGURE[@]}" "-DCMAKE_INSTALL_PREFIX=$INSTALL")
  P_BUILD_TARGETS=()
fi
[ ${#PREFIXES[@]} -eq 0 ] || P_CONFIGURE+=("-DCMAKE_PREFIX_PATH=$(IFS=';'; echo "${PREFIXES[*]}")")

again() {
  echo "oss-build: fetch failed, retrying once in 30 s" >&2
  sleep 30
  "$@"
}

SRC="$OSS/src/$P_CHECKOUT"
BUILD="$OSS/${TP}build/$PROJECT/$FLAVOUR-$MODE${INSTALL:+-dep}"
mkdir -p "$OSS/src" "$BUILD"
# Every build starts from an empty tree: a tree left by an earlier build
# (kept, or cut off half-built) may hold objects of another compiler,
# which make would not rebuild.
find "$BUILD" -mindepth 1 -delete
P_TEST="${P_TEST//@SRC@/$SRC}"
export KILN_OSS_LOG="$BUILD/compile.jsonl"
export KILN_OSS_VMEM_KB="${KILN_OSS_VMEM_KB:-$P_VMEM_KB}"
# A link's cap: the row's link_vmem_kb when it sets one (else kiln-cxx
# uses the compile cap), unless the environment (or the x86-64 lane) set it.
[ "$P_LINK_VMEM_KB" = "$P_VMEM_KB" ] || export KILN_OSS_LINK_VMEM_KB="${KILN_OSS_LINK_VMEM_KB:-$P_LINK_VMEM_KB}"
# The tests: the row's test_vmem_kb (default its vmem_kb), or
# KILN_OSS_VMEM_KB when that is set to override the row.
TEST_VMEM_KB="$P_TEST_VMEM_KB"
[ "$KILN_OSS_VMEM_KB" = "$P_VMEM_KB" ] || TEST_VMEM_KB="$KILN_OSS_VMEM_KB"
if [ "$TARGET" != aarch64 ]; then
  P_TEST_TIMEOUT=$((P_TEST_TIMEOUT * ${HOD_OSS_X64_TEST_SLOWDOWN:-4}))
  # Under qemu every guest thread is a host thread whose stack counts
  # against the cap: abseil's thread tests abort under 3 GB in the
  # control too (WP-191).
  TEST_VMEM_KB="${HOD_OSS_X64_TEST_VMEM_KB:-16000000}"
fi
: > "$KILN_OSS_LOG"
rm -f "$BUILD/report.json" "$BUILD/configure.log" "$BUILD/build.log" "$BUILD/test.log" \
  "$BUILD/kiln-stats.jsonl" "$BUILD/coverage.log" "$BUILD/hod-shim.jsonl"
[ ! -d "$BUILD/declined" ] || find "$BUILD/declined" -mindepth 1 -delete

if [ -d "$SRC/.git" ]; then
  # Compare commits: a commit can carry several tags (protobuf's v33.1 is
  # also v4.33.1-objectivec), and describe names any one of them.
  have="$(git -C "$SRC" describe --tags --exact-match 2>/dev/null || echo '?')"
  [ "$(git -C "$SRC" rev-parse -q --verify HEAD 2>/dev/null)" = "$(git -C "$SRC" rev-parse -q --verify "$P_TAG^{commit}" 2>/dev/null)" ] || echo "oss-build: warning: $SRC is at '$have', the toml pins '$P_TAG' (delete the checkout to refetch)" >&2
else
  sub=()
  [ -n "$P_SUBMODULES" ] && sub=(--recurse-submodules --shallow-submodules)
  echo "oss-build: cloning $P_URL@$P_TAG"
  clone() { git -c advice.detachedHead=false clone -q --depth 1 --branch "$P_TAG" "${sub[@]}" "$P_URL" "$SRC"; }
  clone || { [ ! -e "$SRC" ] && again clone; } || { echo "oss-build: clone failed" >&2; exit 1; }
fi
SRCDIR="$SRC${P_SOURCE_SUBDIR:+/$P_SOURCE_SUBDIR}"

# make: inherit the jobserver from MAKEFLAGS (above); -j only without one.
JFLAG=()
case "${MAKEFLAGS:-}" in
  *--jobserver-auth=fifo:*)
    fifo="${MAKEFLAGS##*--jobserver-auth=fifo:}"; fifo="${fifo%% *}"
    [ -p "$fifo" ] || JFLAG=(-j "$JOBS") ;;
  *) JFLAG=(-j "$JOBS") ;;
esac

# The back end of a phase: configure always with LLVM; build and test in
# the mode's.
backend_env() {
  unset KILN_HOD KILN_HOD_STRICT HOD_OSS_STRICT HOD_CENSUS HOD_COVERAGE_LOG HOD_OSS_SHIM_LOG HOD_OSS_DECLINED KILN_STATS_LOG
  if [ "$1" = configure ] || [ "$MODE" = llc ]; then
    export KILN_BACKEND=llvm
  else
    export KILN_BACKEND=hod KILN_HOD="$HERE/hod-shim" HOD_OSS_REAL_HOD="$HOD" HOD_CENSUS=1 \
      HOD_COVERAGE_LOG="$BUILD/coverage.log" HOD_OSS_SHIM_LOG="$BUILD/hod-shim.jsonl" HOD_OSS_DECLINED="$BUILD/declined"
    [ "$MODE" != strict ] || export KILN_HOD_STRICT=1 HOD_OSS_STRICT=1
  fi
  [ "$1" = configure ] || export KILN_STATS_LOG="$BUILD/kiln-stats.jsonl"
}

T0=$(date +%s.%N)
BUILD_RC="" TEST_RC=""
export KILN_OSS_PHASE=configure
backend_env configure
if [ "$P_BUILD" = autotools ]; then
  # Out of source, the project's own configure (kiln's autotools rows,
  # WP-920 there), told the compilers and the flavour's flags; native only,
  # and no deps (nothing to point it at).
  if [ "$TARGET" != aarch64 ] || [ ${#PREFIXES[@]} -gt 0 ]; then
    echo "oss-build: $PROJECT: an autotools row builds natively and without deps" > "$BUILD/configure.log"; CONF_RC=2
  else
    (cd "$BUILD" && nice -n 10 "$SRCDIR/configure" CC="$P_C_COMPILER" CXX="$HERE/kiln-cxx" \
      CFLAGS="$FLAGS" CXXFLAGS="$FLAGS -std=c++$P_CXX_STD" ${INSTALL:+"--prefix=$INSTALL"} \
      "${P_CONFIGURE[@]}") > "$BUILD/configure.log" 2>&1
    CONF_RC=$?
  fi
else
nice -n 10 cmake --fresh -S "$SRCDIR" -B "$BUILD" -G "Unix Makefiles" \
  -DCMAKE_CXX_COMPILER="$HERE/kiln-cxx" -DCMAKE_C_COMPILER="$P_C_COMPILER" \
  -DCMAKE_CXX_STANDARD="$P_CXX_STD" -DCMAKE_BUILD_TYPE=Debug \
  "-DCMAKE_CXX_FLAGS_DEBUG=$FLAGS" "-DCMAKE_C_FLAGS_DEBUG=$FLAGS" \
  "${XCONF[@]}" "${P_CONFIGURE[@]}" > "$BUILD/configure.log" 2>&1
CONF_RC=$?
fi
echo "oss-build: configure rc=$CONF_RC ($BUILD/configure.log)"

if [ "$CONF_RC" = 0 ]; then
  export KILN_OSS_PHASE=build
  backend_env build
  [ "$P_BUILD" = autotools ] || nice -n 10 make -C "$BUILD" clean > /dev/null 2>&1
  nice -n 10 make -k "${JFLAG[@]}" -C "$BUILD" "${P_BUILD_TARGETS[@]}" > "$BUILD/build.log" 2>&1
  BUILD_RC=$?
  echo "oss-build: build rc=$BUILD_RC ($BUILD/build.log)"
  if [ -n "$INSTALL" ]; then
    inst=(cmake --install "$BUILD"); [ "$P_BUILD" != autotools ] || inst=(make -C "$BUILD" install)
    [ "$BUILD_RC" = 0 ] && nice -n 10 "${inst[@]}" >> "$BUILD/build.log" 2>&1 && date -u > "$INSTALL/.oss-installed" || BUILD_RC=1
    echo "oss-build: install into $INSTALL rc=$BUILD_RC"
  fi
  if [ -n "$RUN_TEST" ]; then
    export KILN_OSS_PHASE=test
    # Per-tree test scratch, so the builds of one project never share it.
    TTMP="$BUILD/test-tmp"; mkdir -p "$TTMP" && find "$TTMP" -mindepth 1 -delete
    [ "$TARGET" = aarch64 ] || python3 "$HERE/projects.py" emulate-tests "$BUILD" "$X64_QEMU" -L "$X64_SYSROOT" > "$BUILD/emulate.log" 2>&1
    # The cap is on address space, and glibc reserves 64 MB of it for each
    # malloc arena (up to 8 per CPU): a test running many threads at once
    # exhausts it on a many-CPU host without using the memory, sooner the
    # slower its code. MALLOC_ARENA_MAX bounds the reservation, the same
    # for every mode.
    (cd "$BUILD" && ulimit -v "$TEST_VMEM_KB" && export CTEST_PARALLEL_LEVEL="${CTEST_PARALLEL_LEVEL:-4}" \
      TMPDIR="$TTMP" TEST_TMPDIR="$TTMP" MALLOC_ARENA_MAX="${MALLOC_ARENA_MAX:-4}" && timeout "$P_TEST_TIMEOUT" nice -n 10 bash -c "$P_TEST") > "$BUILD/test.log" 2>&1
    TEST_RC=$?
    # A test's forked child can outlive the suite (DuckDB's unittest leaves
    # one blocked in accept() on a listening socket): kill whatever still
    # runs an executable of this tree, so no later run meets it.
    for e in /proc/[0-9]*/exe; do
      case "$(readlink "$e" 2>/dev/null)" in
        "$BUILD"/*) e="${e#/proc/}"; kill -9 "${e%/exe}" 2> /dev/null && echo "oss-build: killed leftover test process ${e%/exe}" ;;
      esac
    done
    echo "oss-build: test rc=$TEST_RC ($BUILD/test.log)"
  fi
fi
WALL=$(python3 -c "print($(date +%s.%N) - $T0)")

python3 "$HERE/projects.py" report-json --project "$PROJECT" --mode "$MODE" --flavour "$FLAVOUR" --target "$TARGET" \
  --compiler-path "$CXX" --hod-path "${HOD:-}" --kiln "$KILN_ID" --build-dir "$BUILD" --configure-rc "$CONF_RC" \
  ${BUILD_RC:+--build-rc "$BUILD_RC"} ${TEST_RC:+--test-rc "$TEST_RC"} --wall "$WALL"

[ -z "$PRUNE" ] || prune "$BUILD"
[ -z "$INSTALL" ] || [ "$BUILD_RC" = 0 ]
