#!/bin/sh
# Design 0008 section 1.2: each line of mc.s encoded by llvm-mc 22 for i686
# and, with its address registers written as 64-bit ones (pushl/popl/calll/
# jmpl/retl as q), for x86_64.  usage: sh compare.sh > compare.out
L=/usr/lib/llvm-22/bin
while IFS= read -r line; do
  e32=$(echo "$line" | $L/llvm-mc -triple=i686-linux-gnu -show-encoding 2>&1 | grep -o 'encoding: .*\|error: .*' | head -1)
  l64=$(echo "$line" | sed -E 's/\(%e(..)/(%r\1/; s/,%e(..)\)/,%r\1)/; s/,%e(..),/,%r\1,/; s/^(push|pop|call|jmp|ret)l/\1q/; s/%e(..)$/%r\1/; /^(push|pop|call|jmp)/!s/%r(..)$/%e\1/')
  e64=$(echo "$l64" | $L/llvm-mc -triple=x86_64-linux-gnu -show-encoding 2>&1 | grep -o 'encoding: .*\|error: .*' | head -1)
  printf '%-28s | %-46s | %-28s | %s\n' "$line" "${e32#encoding: }" "$l64" "${e64#encoding: }"
done < mc.s
