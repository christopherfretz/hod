#!/bin/sh
# Design 0008: every ABI probe through llc 19 and 22 for i686 Linux at -O0,
# SelectionDAG (-fast-isel=0) and FastISel (default); static unless the
# file name says pic.  usage: sh run.sh [file.ll...]
# writes <name>.{dag,fast}{19,22}.s next to each .ll; FAILED on stderr.
[ $# -gt 0 ] || set -- [0-9]*.ll
for f in "$@"; do
  b=${f%.ll}
  case $b in *pic*) rm=pic ;; *) rm=static ;; esac
  for v in 19 22; do
    for isel in dag fast; do
      case $isel in dag) x=-fast-isel=0 ;; fast) x= ;; esac
      /usr/lib/llvm-$v/bin/llc -mtriple=i686-unknown-linux-gnu -O0 $x \
        -relocation-model=$rm -asm-verbose=false "$f" -o "$b.$isel$v.s" 2> "$b.$isel$v.err" \
        || echo "$f llc$v $isel FAILED: $(head -c 200 $b.$isel$v.err)"
      [ -s "$b.$isel$v.err" ] || rm -f "$b.$isel$v.err"
    done
  done
done
