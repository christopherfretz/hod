#!/bin/sh
# Design 0006 section 6.1: do x86-64's -O0 selectors (FastISel, the
# default; SelectionDAG with -fast-isel=0) and -O2 agree on hod_interp's
# random programs? The batches are what `tests/llvm_oracle.rs` writes with
# HOD_ORACLE_KEEP=<dir> (generated for aarch64; only the triple line is
# changed: the programs print what they compute, they do not depend on
# the layout beyond what both triples share, and the aarch64 reference
# output is compared as well).
#
# usage: sh batches.sh <keep dir> <work dir>   (prints one line per batch and build)
keep=$1; work=$2; mkdir -p "$work"
for b in "$keep"/batch-*.ll; do
  n=$(basename "$b" .ll)
  sed 's/^target triple = .*/target triple = "x86_64-unknown-linux-gnu"/' "$b" > "$work/$n.x.ll"
  # the aarch64 reference: SelectionDAG at -O0, run natively
  /usr/lib/llvm-22/bin/llc -O0 -global-isel=0 -fast-isel=0 -relocation-model=pic -filetype=obj "$b" -o "$work/$n.a.o" &&
    gcc "$work/$n.a.o" -o "$work/$n.a" -lm && timeout 120 "$work/$n.a" > "$work/$n.a.out" 2>&1
  for v in 19 22; do
    for cfg in fast dag o2; do
      case $cfg in fast) f="-O0";; dag) f="-O0 -fast-isel=0";; o2) f="-O2";; esac
      out="$work/$n.$v.$cfg"
      if /usr/lib/llvm-$v/bin/llc -mtriple=x86_64-unknown-linux-gnu $f -relocation-model=pic \
           -filetype=obj "$work/$n.x.ll" -o "$out.o" 2> "$out.err" &&
         x86_64-linux-gnu-gcc "$out.o" -o "$out" -lm; then
        start=$(date +%s.%N)
        timeout 120 qemu-x86_64 -L /usr/x86_64-linux-gnu "$out" > "$out.out" 2>&1
        rc=$?; end=$(date +%s.%N)
        if cmp -s "$out.out" "$work/$n.a.out"; then same=same; else same=DIFFERS; fi
        echo "$n llvm$v $cfg rc=$rc vs-aarch64=$same secs=$(awk "BEGIN{print $end - $start}")"
      else
        echo "$n llvm$v $cfg BUILD-FAILED $(head -c 200 "$out.err")"
      fi
    done
  done
done
