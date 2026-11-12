#!/bin/sh
# The `-g -O0` census of `crates/hod_a64/tests/dies_e2e.rs` `debug_census`
# (`spec/dwarf.md` §6.12 to §6.14): makes its input modules, then runs it.
#
#   scripts/debug-census.sh <out dir> [run]
#
# Inputs (each by clang 19, clang 22 and kiln++, `-g -O0 -S -emit-llvm`,
# C++20, the host's libstdc++): 44 libc++ tests (every 180th `std/`
# test the libc++ baseline passes, sorted, with libc++'s `test/support`),
# `crates/hod_ll/tests/data/stl.cpp`, the four C++ bench sources; then by
# kiln++ alone (C++23) five of kiln's libstdc++ conformance units,
# coroutines among them (kiln writes them split, as it hands them to
# `llc`). A source a compiler cannot build is left out (listed in
# `<out>/skipped.txt`). `run` then runs the census on `<out>` (one test
# thread; with `HOD_DEBUG_CENSUS_SESSIONS=1`, the debugger sessions too).
# Paths: `HOD_LIBCXX_SRC` (libc++'s source, default
# `../.toolchain/libcxx-llvmorg-22.1.8/libcxx` of the wrapper repository),
# `HOD_KILN` (kiln++), `HOD_KILN_SRC` (kiln's checkout), `HOD_LIBCXX_BASELINE`
# (the baseline's tsv).
set -eu
out="${1:?usage: debug-census.sh <out dir> [run]}"
here="$(cd "$(dirname "$0")/.." && pwd)"
ws="$(cd "$here/../../.." 2>/dev/null && pwd)"
[ -d "$ws/.toolchain" ] || ws="$(cd "$here/.." && pwd)"
libcxx="${HOD_LIBCXX_SRC:-$ws/.toolchain/libcxx-llvmorg-22.1.8/libcxx}"
kiln="${HOD_KILN:-$ws/.build/target-kiln/release/kiln++}"
kiln_src="${HOD_KILN_SRC:-$ws/kiln}"
baseline="${HOD_LIBCXX_BASELINE:-$ws/.build/libcxx-baseline/baseline.tsv}"
mkdir -p "$out"
: > "$out/skipped.txt"

one() { # <compiler tag> <compiler> <std> <source> <name> [flags...]
    tag="$1" cc="$2" std="$3" src="$4" name="$5"
    shift 5
    dst="$out/$name.$tag.ll"
    [ -s "$dst" ] && return 0
    if ! "$cc" -g -O0 -S -emit-llvm -w "-std=$std" "$@" "$src" -o "$dst" 2>/dev/null; then
        rm -f "$dst"
        echo "$tag $src" >> "$out/skipped.txt"
    fi
}

three() { # <source> <name> [flags...]
    src="$1" name="$2"
    shift 2
    one c19 clang++-19 c++20 "$src" "$name" "$@"
    one c22 clang++-22 c++20 "$src" "$name" "$@"
    one kiln "$kiln" c++20 "$src" "$name" "$@"
}

grep '^PASS' "$baseline" | cut -f2 | grep '^std/' | grep '\.pass\.cpp$' | sort |
    awk 'NR % 180 == 1' | head -44 > "$out/libcxx-tests.txt"
while read -r t; do
    name="$(echo "$t" | tr '/' '_' | sed 's/\.pass\.cpp$//')"
    three "$libcxx/test/$t" "$name" -I "$libcxx/test/support"
done < "$out/libcxx-tests.txt"
three "$here/crates/hod_ll/tests/data/stl.cpp" stl
for b in containers exceptions stl_heavy templates; do
    three "$here/bench/src/$b.cpp" "bench_$b"
done
for u in re.alg/regex-match-libstdcxx c.math/cmath-libstdcxx \
    enum.udecl/libstdcxx-local-using-enum-template \
    coro.generator/libstdcxx-generator-int-run \
    coro.generator/libstdcxx-generator-elements-of-run; do
    one kiln "$kiln" c++23 "$kiln_src/tests/conformance/$u.cpp" "$(basename "$u")"
done
echo "$(find "$out" -name '*.ll' | wc -l) modules in $out ($(wc -l < "$out/skipped.txt") skipped)"

if [ "${2:-}" = run ]; then
    cd "$here"
    . scripts/env.sh
    HOD_DEBUG_CENSUS="$out" RUST_TEST_THREADS=1 \
        cargo test -q -p hod_a64 --test dies_e2e debug_census -- --ignored --nocapture
fi
