#!/usr/bin/env bash
# The `-g` set of real sources for the M3 measurement (docs/status/debug.md,
# WP-099), in the spirit of WP-087's and WP-094's census
# (spec/landed/dwarf/WP-087.md, WP-094.md), whose exact libc++ list was not
# recorded. This one is fixed by rule, so it can be regenerated:
#
#   - every 120th `*.pass.cpp` of libc++'s `test/std` (sorted by path),
#     with `-I test/support`, against the system's libstdc++;
#   - crates/hod_ll/tests/data/stl.cpp, crates/hod_a64/tests/data/dies_e2e/e2e.cpp;
#   - every source of bench/src;
#   - kiln's four libstdc++ units of WP-094 (kiln++ only).
#
# each by clang 19, clang 22 and kiln++ at `-g -O0 -S -emit-llvm`, into
#   <out>/llvm19/clang/<name>.ll, <out>/llvm22/clang/<name>.ll, <out>/llvm22/kiln/<name>.ll
# A source a front end does not compile is left out (listed in <out>/failed.txt).
#
# Usage: scripts/debug-gset.sh <out dir> [<libc++ checkout>] [<kiln repo>] [<kiln++>]
set -u
here="$(cd "$(dirname "$0")/.." && pwd)"
out="$1"
libcxx="${2:-$here/../../../.toolchain/libcxx-llvmorg-22.1.8/libcxx}"
kilnrepo="${3:-$here/../../../kiln}"
kiln="${4:-$here/../../../.build/target-kiln/release/kiln++}"
mkdir -p "$out/llvm19/clang" "$out/llvm22/clang" "$out/llvm22/kiln"
: > "$out/failed.txt"

compile() { # <producer dir> <compiler...> -- <source> <name> <std> [flags]
  local dir="$1"; shift
  local cc=() ; while [ "$1" != "--" ]; do cc+=("$1"); shift; done; shift
  local src="$1" name="$2" std="$3"; shift 3
  if ! (cd "$(dirname "$src")" && timeout 300 "${cc[@]}" -std="$std" -g -O0 -S -emit-llvm -w \
        -fdebug-compilation-dir=. "$@" "$(basename "$src")" -o "$out/$dir/$name.ll") >/dev/null 2>&1; then
    rm -f "$out/$dir/$name.ll"
    echo "$dir $name" >> "$out/failed.txt"
  fi
}

each() { # <source> <name> <std> [flags]
  local src="$1" name="$2" std="$3"; shift 3
  if [[ "$src" == *.c ]]; then
    compile llvm19/clang clang-19 -- "$src" "$name" "$std" "$@"
    compile llvm22/clang clang-22 -- "$src" "$name" "$std" "$@"
  else
    compile llvm19/clang clang++-19 -- "$src" "$name" "$std" "$@"
    compile llvm22/clang clang++-22 -- "$src" "$name" "$std" "$@"
    compile llvm22/kiln "$kiln" --llvm-dir=/usr/lib/llvm-22 -fno-crash-diagnostics -- "$src" "$name" c++26 "$@"
  fi
}

n=0
while read -r t; do
  n=$((n + 1))
  [ $((n % 120)) -eq 1 ] || continue
  name="libcxx-$(echo "${t#./}" | tr '/' '_' )"
  each "$libcxx/test/std/${t#./}" "${name%.cpp}" c++23 -I "$libcxx/test/support"
done < <(cd "$libcxx/test/std" && find . -name '*.pass.cpp' | LC_ALL=C sort)

each "$here/crates/hod_ll/tests/data/stl.cpp" stl c++20
each "$here/crates/hod_a64/tests/data/dies_e2e/e2e.cpp" e2e c++20
for s in "$here"/bench/src/*; do
  b="$(basename "$s")"
  case "$b" in *.c) each "$s" "bench-${b%.c}-c" c17 ;; *.cpp) each "$s" "bench-${b%.cpp}" c++23 ;; esac
done
for t in re.alg/regex-match-libstdcxx.cpp coro.generator/libstdcxx-generator-int-run.cpp \
         c.math/cmath-libstdcxx.cpp enum.udecl/libstdcxx-local-using-enum-template.cpp; do
  b="$(basename "$t")"
  compile llvm22/kiln "$kiln" --llvm-dir=/usr/lib/llvm-22 -fno-crash-diagnostics -- \
    "$kilnrepo/tests/conformance/$t" "kiln-${b%.cpp}" c++26
done
find "$out" -name '*.ll' | wc -l
