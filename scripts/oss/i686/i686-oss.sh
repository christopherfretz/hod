#!/usr/bin/env bash
# i686-oss.sh <project> <mode> [--force] [--no-test] [--prune]
#
# The i386 real-project lane (spec/landed/harness/WP-351.md,
# docs/status/oss-i686.md): builds one project for i686-linux-gnu with
# scripts/oss/i686/i686-cc (clang 22 to IR, before any LLVM pass; the back
# end by <mode>: control = llc -O0 -global-isel=0 -fast-isel=0, hybrid =
# hod with that llc as fallback, strict = hod alone) and runs its tests
# under qemu-i386, then writes the tree's summary (i686-report.py).
# Projects: zlib, zstd (the 64-bit lanes' cached sources, the tags of
# scripts/oss/projects.toml), bzip2 (1.0.8, the copy in CMake's sources,
# its library and both programs built by the lane's own Makefile).
#
# Configure always runs in control mode (the three builds configure
# alike); build and test in the mode's. Every build is the project's
# release build at -O2 (-O2 -DNDEBUG; the module hod gets is the front
# end's, see i686-cc). Resumable: a phase whose stamp exists is skipped
# (--force starts the tree afresh). Make draws from the real-project
# jobserver pool (/run/hod/lanes.fifo) when it exists, else -j4.
#
# Tree: <root>/<project>/<mode>/ with build/ and logs/ (configure.log,
# build.log, test.log, tests.tsv, time.tsv, cov.tsv, refused/).
# <root> = $HOD_I686_OSS_ROOT, default $CARGO_TARGET_DIR/oss-i686.
# Sources: $HOD_OSS_SRC (default <build root>/oss/src) for zlib and zstd,
# $HOD_I686_BZIP2_SRC (default <build root>/oss3/src/cmake/Utilities/cmbzip2).
# hod: $HOD_I686_HOD, default $CARGO_TARGET_DIR/release/hod.
# --prune deletes build/ after the summary (logs and saved modules stay).
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${CARGO_TARGET_DIR:?i686-oss: source scripts/env.sh first}"
BROOT="${HOD_BUILD_ROOT:-$(dirname "$CARGO_TARGET_DIR")}"
ROOT="${HOD_I686_OSS_ROOT:-$CARGO_TARGET_DIR/oss-i686}"
SRCS="${HOD_OSS_SRC:-$BROOT/oss/src}"
P="${1:-}" MODE="${2:-}"
shift 2 2>/dev/null || { sed -n 2p "$0" | sed 's/^# //' >&2; exit 2; }
FORCE="" TEST=1 PRUNE=""
for a in "$@"; do
  case "$a" in
    --force) FORCE=1 ;; --no-test) TEST="" ;; --prune) PRUNE=1 ;;
    *) echo "i686-oss: unknown option $a" >&2; exit 2 ;;
  esac
done
case "$MODE" in control | hybrid | strict) ;; *) echo "i686-oss: mode must be control, hybrid or strict" >&2; exit 2 ;; esac
case "$P" in
  zlib) SRC="$SRCS/zlib" KIND=cmake CONF=(-DZLIB_BUILD_TESTING=ON) ;;
  zstd) SRC="$SRCS/zstd/build/cmake" KIND=cmake
        CONF=(-DZSTD_BUILD_TESTS=ON -DZSTD_FUZZER_FLAGS=-T60s -DZSTD_ZSTREAM_FLAGS=-T60s -DZSTD_FULLBENCH_FLAGS=-i1) ;;
  bzip2) SRC="${HOD_I686_BZIP2_SRC:-$BROOT/oss3/src/cmake/Utilities/cmbzip2}" KIND=make CONF=() ;;
  *) echo "i686-oss: unknown project $P (zlib, zstd, bzip2)" >&2; exit 2 ;;
esac
[ -d "$SRC" ] || { echo "i686-oss: no sources at $SRC" >&2; exit 1; }
HOD="${HOD_I686_HOD:-$CARGO_TARGET_DIR/release/hod}"
[ "$MODE" = control ] || [ -x "$HOD" ] || { echo "i686-oss: no hod at $HOD" >&2; exit 1; }

T="$ROOT/$P/$MODE"
B="$T/build" L="$T/logs"
if [ -n "$FORCE" ] && [ -d "$T" ]; then find "$T" -mindepth 1 -delete; fi
mkdir -p "$B" "$L"
FIFO="${HOD_OSS_JOBSERVER_FIFO:-/run/hod/lanes.fifo}"
JFLAG=()
if [ -p "$FIFO" ]; then export MAKEFLAGS="--jobserver-auth=fifo:$FIFO"; else unset MAKEFLAGS; JFLAG=(-j4); fi
export HOD_I686_HOD="$HOD" HOD_I686_KEEP=1
CC="$HERE/i686-cc"
QEMU_RUN=(qemu-i386 -cpu pentium2,-mmx -L /usr/i686-linux-gnu -R 0x40000000)
say() { echo "i686-oss: $P $MODE: $* ($(date -u +%H:%M:%S))"; }

# configure: control mode, unlogged, real executables (CMake reads them).
if [ ! -e "$T/configure.ok" ]; then
  say configure
  if [ "$KIND" = cmake ]; then
    HOD_I686_MODE=control HOD_I686_LAUNCH=0 nice -n 10 cmake --fresh -S "$SRC" -B "$B" -G "Unix Makefiles" \
      -DCMAKE_C_COMPILER="$CC" -DCMAKE_CXX_COMPILER="$HERE/i686-c++" \
      -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_FLAGS_RELEASE="-O2 -DNDEBUG" -DCMAKE_CXX_FLAGS_RELEASE="-O2 -DNDEBUG" \
      "${CONF[@]}" > "$L/configure.log" 2>&1 || { say "configure failed (logs/configure.log)"; exit 1; }
  else
    # bzip2 1.0.8's own Makefile: its CFLAGS, the library and both programs.
    {
      echo "VPATH = $SRC"
      echo "CC = $CC"
      echo "CFLAGS = -Wall -Winline -O2 -D_FILE_OFFSET_BITS=64 -I$SRC"
      echo "OBJS = blocksort.o huffman.o crctable.o randtable.o compress.o decompress.o bzlib.o"
      echo "all: bzip2 bzip2recover"
      printf 'bzip2: libbz2.a bzip2.o\n\t$(CC) $(CFLAGS) -o bzip2 bzip2.o -L. -lbz2\n'
      printf 'bzip2recover: bzip2recover.o\n\t$(CC) $(CFLAGS) -o bzip2recover bzip2recover.o\n'
      printf 'libbz2.a: $(OBJS)\n\trm -f libbz2.a\n\tar cq libbz2.a $(OBJS)\n'
      printf '%%.o: %%.c\n\t$(CC) $(CFLAGS) -c $< -o $@\n'
    } > "$B/Makefile"
  fi
  touch "$T/configure.ok"
fi

if [ ! -e "$T/build.ok" ]; then
  say build
  [ -n "$FORCE" ] || rm -f "$L/time.tsv" "$L/cov.tsv"
  if [ -d "$L/refused" ]; then find "$L/refused" -mindepth 1 -delete; fi
  if [ "$KIND" = cmake ]; then nice -n 10 make -C "$B" clean > /dev/null 2>&1; fi
  HOD_I686_MODE="$MODE" HOD_I686_LAUNCH=1 HOD_I686_LOGDIR="$L" \
    nice -n 10 make -k "${JFLAG[@]}" -C "$B" > "$L/build.log" 2>&1
  echo "$?" > "$T/build.ok"
  say "build status $(cat "$T/build.ok")"
fi

# Tests: one line per test in logs/tests.tsv (name, PASS or FAIL).
ctest_tsv() { # ctest's per-test lines to name<TAB>PASS|FAIL
  sed -nE 's/^ *[0-9]+\/[0-9]+ Test +#[0-9]+: ([^ ]+) \.*[ ]+(\*\*\*)?([A-Za-z]+).*/\1\t\3/p' "$1" |
    awk -F'\t' '{ print $1 "\t" ($2 == "Passed" ? "PASS" : "FAIL") }'
}
check() { # name, command...: one named test of the lane's own
  local n="$1"; shift
  if ( "$@" ) >> "$L/test.log" 2>&1; then printf '%s\tPASS\n' "$n"; else printf '%s\tFAIL\n' "$n"; fi
}
if [ -n "$TEST" ] && [ ! -e "$T/test.ok" ]; then
  say test
  : > "$L/test.log"
  tmp="$T/tmp"; mkdir -p "$tmp"
  export TMPDIR="$tmp"
  case "$P" in
    zlib)
      (cd "$B" && timeout 1800 ctest -j1 --timeout 600 --output-on-failure -E 'install|find_package|add_subdirectory') >> "$L/test.log" 2>&1
      { ctest_tsv "$L/test.log"
        for lvl in -1 -6 -9 -h -r -f; do
          check "minigzip$lvl" sh -c "'$B/test/minigzip' $lvl -c < '$B/libz.a' > '$tmp/z.gz' && '$B/test/minigzip' -d -c < '$tmp/z.gz' | cmp - '$B/libz.a'"
        done; } > "$L/tests.tsv" ;;
    zstd)
      (cd "$B" && timeout 3600 ctest -j1 --timeout 1200 --output-on-failure -E playTests) >> "$L/test.log" 2>&1
      ctest_tsv "$L/test.log" > "$L/tests.tsv" ;;
    bzip2)
      # Inputs: the library, a program, text, an empty file, one byte
      # repeated (bzip2's run-length paths), random bytes.
      cp "$B/libbz2.a" "$tmp/lib.in"; cp "$B/bzip2.i686" "$tmp/prog.in"
      cat "$SRC"/*.c > "$tmp/text.in"; : > "$tmp/empty.in"
      head -c 300000 /dev/zero | tr '\0' 'a' > "$tmp/same.in"
      head -c 200000 /dev/urandom > "$tmp/rand.in"
      {
        for f in lib prog text empty same rand; do
          for lvl in -1 -9 -s; do
            # round trip, and the stream equals the host bzip2's (1.0.8)
            check "rt-$f$lvl" sh -c "'$B/bzip2' $lvl -c < '$tmp/$f.in' > '$tmp/o.bz2' && '$B/bzip2' -d -c < '$tmp/o.bz2' | cmp - '$tmp/$f.in' && bzip2 $lvl -c < '$tmp/$f.in' | cmp - '$tmp/o.bz2'"
          done
          check "host-to-lane-$f" sh -c "bzip2 -9 -c < '$tmp/$f.in' | '$B/bzip2' -d -c | cmp - '$tmp/$f.in'"
          check "test-$f" sh -c "bzip2 -c < '$tmp/$f.in' > '$tmp/t.bz2' && '$B/bzip2' -t '$tmp/t.bz2'"
        done
        check "corrupt-detected" sh -c "bzip2 -c < '$tmp/text.in' > '$tmp/c.bz2' && printf 'X' | dd of='$tmp/c.bz2' bs=1 seek=5000 conv=notrunc 2>/dev/null && ! '$B/bzip2' -t '$tmp/c.bz2'"
        mkdir -p "$tmp/rec"
        check "recover" sh -c "bzip2 -1 -c < '$tmp/text.in' > '$tmp/rec/r.bz2' && cd '$tmp/rec' && '$B/bzip2recover' r.bz2 && cat rec*r.bz2 | bzip2 -d -c | cmp - ../text.in"
      } > "$L/tests.tsv" ;;
  esac
  find "$tmp" -mindepth 1 -delete; rmdir "$tmp"
  touch "$T/test.ok"
fi

python3 "$HERE/i686-report.py" tree "$T" > "$T/summary.txt" && cat "$T/summary.txt"
if [ -n "$PRUNE" ]; then find "$B" -mindepth 1 -delete; fi
exit 0
