#!/bin/sh
# Design 0008 section 4: link i686 objects written by llc 22 with each
# linker and run them under qemu-i386, without any i686 C library (start.s
# exits through int $0x80). Run in a scratch copy of this directory.
#   main.ll       ret of an i64 global, truncated: exit status 42
#   pic.ll def.ll PIC GOT32X and PLT32, linked -pie: 42 where the linker
#                 relaxes GOT32X (no dynamic loader applies R_386_RELATIVE)
#   tdef.ll       with ../abi/10-tls-pic.ll: GD/LD relaxed to LE (disassemble)
L=/usr/lib/llvm-22/bin
WILD=${WILD:-wild}
$L/llvm-mc -triple=i686-linux-gnu -filetype=obj start.s -o start.o
for f in main pic def tdef; do
  case $f in pic|def) rm=pic ;; *) rm=static ;; esac
  $L/llc -mtriple=i686-unknown-linux-gnu -O0 -relocation-model=$rm -filetype=obj $f.ll -o $f.o
done
$L/llc -mtriple=i686-unknown-linux-gnu -O0 -fast-isel=0 -relocation-model=pic -filetype=obj ../abi/10-tls-pic.ll -o tls.o
for ld in "$L/ld.lld" mold x86_64-linux-gnu-ld.bfd "$WILD"; do
  n=$(basename "$ld")
  $ld -m elf_i386 -static start.o main.o -o "a.$n" && { qemu-i386 "./a.$n"; echo "$n static: exit $?"; }
  $ld -m elf_i386 -pie --no-dynamic-linker -z norelro start.o pic.o def.o -o "p.$n" && { qemu-i386 "./p.$n"; echo "$n pie: exit $?"; }
  $ld -m elf_i386 -static start.o tls.o tdef.o -o "t.$n" && $L/llvm-objdump -d --no-show-raw-insn "t.$n" | sed -n '/<t>:/,/retl/p' > "t.$n.dis"
done
