#!/usr/bin/env bash
# i686-time.sh <tree> [runs]: the back-end time of llc (the control's
# flags) against hod over the same modules, one at a time: every <obj>.ll
# the lane kept under <tree>/build (a control tree's are the same modules
# as hybrid's and strict's), each back end run [runs] times (default 3)
# in turn, the fastest kept. Prints modules, llc ms, hod ms, hod / llc,
# and the modules hod refuses (left out of both sums). The wrapper's
# time.tsv measures the same steps inside a parallel build on a shared
# machine; this is the quieter number. Writes nothing but /dev/null.
set -uo pipefail
t="${1:?usage: i686-time.sh <tree> [runs]}"; runs="${2:-3}"
: "${CARGO_TARGET_DIR:?i686-time: source scripts/env.sh first}"
hod="${HOD_I686_HOD:-$CARGO_TARGET_DIR/release/hod}"
llc="${HOD_I686_BIN:-/usr/lib/llvm-22/bin}/llc"
best() { # command...: the fastest of $runs runs, microseconds; status of the last
  local b="" i t0 t1 st=0
  for ((i = 0; i < runs; i++)); do
    t0="${EPOCHREALTIME/./}"; "$@" > /dev/null 2>&1; st=$?; t1="${EPOCHREALTIME/./}"
    [ -z "$b" ] || [ $((t1 - t0)) -lt "$b" ] && b=$((t1 - t0))
  done
  echo "$b"; return "$st"
}
n=0 tl=0 th=0 refused=0
while IFS= read -r ll; do
  be=(-O0 -filetype=obj -mtriple=i686-unknown-linux-gnu)
  grep -q '"PIC Level"' "$ll" && be+=(-relocation-model=pic)
  h="$(best "$hod" "${be[@]}" --strict "$ll" -o /dev/null)" || { refused=$((refused + 1)); continue; }
  l="$(best "$llc" "${be[@]}" -global-isel=0 -fast-isel=0 "$ll" -o /dev/null)" || continue
  n=$((n + 1)) tl=$((tl + l)) th=$((th + h))
done < <(find "$t/build" -name '*.o.ll' | sort)
awk -v n="$n" -v l="$tl" -v h="$th" -v r="$refused" 'BEGIN {
  printf "modules %d; llc %.0f ms; hod %.0f ms; hod / llc %.2f; refused by hod (left out) %d\n", n, l / 1000, h / 1000, (l ? h / l : 0), r }'
