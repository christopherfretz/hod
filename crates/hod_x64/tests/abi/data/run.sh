#!/bin/sh
# Design 0006 section 3: every ABI experiment through llc 19 and 22 for
# x86-64 (SelectionDAG at -O0, the reference configuration), PIC.
# usage: sh run.sh   (writes <name>.llc19.s and <name>.llc22.s next to each .ll)
for f in [0-9]*.ll; do
  for v in 19 22; do
    /usr/lib/llvm-$v/bin/llc -mtriple=x86_64-unknown-linux-gnu -O0 -fast-isel=0 \
      -relocation-model=pic -asm-verbose=false "$f" -o "${f%.ll}.llc$v.s" 2> "${f%.ll}.llc$v.err" \
      || echo "$f llc$v FAILED"
    [ -s "${f%.ll}.llc$v.err" ] || rm -f "${f%.ll}.llc$v.err"
  done
done
