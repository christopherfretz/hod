#!/usr/bin/env python3
"""Design 0006 section 2.4: how often llc -O0 (x86-64, FastISel) folds a
load into the instruction that uses it, and what folding is worth in bytes.

For each module: llc 22 -O0 for x86-64, then llvm-objdump; counts
instructions, ALU instructions with a memory operand (folded loads), frame
loads and stores (mov to/from [rbp|rsp+d]), and the IR's `load`s.

usage: python3 -I fold.py <work dir> <module.ll>...
"""
import os
import re
import subprocess
import sys

LLC = "/usr/lib/llvm-22/bin/llc"
DUMP = "/usr/lib/llvm-22/bin/llvm-objdump"
INST = re.compile(r"^\s+[0-9a-f]+:\s+\t?(\S+)\s*(.*)$")
ALU = re.compile(r"^(add|sub|and|or|xor|cmp|imul|test|adc|sbb|adds[sd]|subs[sd]|muls[sd]|divs[sd]|ucomis[sd]|cvtsi2s[sd]\w*)[bwlq]?$")
FRAME = re.compile(r"-?(0x[0-9a-f]+)?\(%r[bs]p\)")

work = sys.argv[1]
os.makedirs(work, exist_ok=True)
tot = {"insts": 0, "alu-mem": 0, "alu": 0, "frame-loads": 0, "frame-stores": 0, "ir-loads": 0, "text": 0}
print("module\tinsts\talu-mem\talu\tframe-loads\tframe-stores\tir-loads\ttext-bytes")
for ll in sys.argv[2:]:
    b = os.path.join(work, os.path.basename(ll))
    if subprocess.run([LLC, "-mtriple=x86_64-unknown-linux-gnu", "-O0", "-relocation-model=pic",
                       "-filetype=obj", ll, "-o", b + ".o"], capture_output=True).returncode:
        continue
    dis = subprocess.run([DUMP, "-d", "--no-show-raw-insn", b + ".o"], capture_output=True, text=True).stdout
    row = dict.fromkeys(tot, 0)
    for line in dis.splitlines():
        m = INST.match(line)
        if not m:
            continue
        op, args = m.group(1), m.group(2)
        row["insts"] += 1
        if ALU.match(op):
            row["alu"] += 1
            if "(" in args:
                row["alu-mem"] += 1
        if op.startswith("mov") and "," in args:
            src, dst = args.rsplit(",", 1)
            if FRAME.search(src):
                row["frame-loads"] += 1
            if FRAME.search(dst):
                row["frame-stores"] += 1
    row["ir-loads"] = len(re.findall(r"= load ", open(ll).read()))
    size = subprocess.run(["/usr/lib/llvm-22/bin/llvm-size", "-A", b + ".o"], capture_output=True, text=True).stdout
    row["text"] = sum(int(l.split()[1]) for l in size.splitlines() if l.startswith(".text"))
    for k in tot:
        tot[k] += row[k]
    print(os.path.basename(ll) + "\t" + "\t".join(str(row[k]) for k in tot))
print("TOTAL\t" + "\t".join(str(tot[k]) for k in tot))
