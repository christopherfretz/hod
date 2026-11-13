#!/usr/bin/env python3
"""Design 0006 section 5: the branch forms llc -O0 writes for x86-64
(after MC relaxation), and the bytes a "forward branches are rel32" policy
would add. Reads llvm-objdump -d of the objects fold.py left.

usage: python3 -I branches.py <object>... ; prints totals
"""
import re
import subprocess
import sys

LINE = re.compile(r"^\s+([0-9a-f]+):\s+((?:[0-9a-f]{2} )+)\s*(j\w+)\s+0x([0-9a-f]+) <")
c = dict(fwd8=0, fwd32=0, back8=0, back32=0, extra=0, text=0)
for o in sys.argv[1:]:
    dis = subprocess.run(["/usr/lib/llvm-22/bin/llvm-objdump", "-d", o], capture_output=True, text=True).stdout
    for l in dis.splitlines():
        m = LINE.match(l)
        if not m:
            continue
        at, n, op, tgt = int(m.group(1), 16), len(m.group(2).split()), m.group(3), int(m.group(4), 16)
        fwd = tgt > at
        short = n == 2
        c[("fwd" if fwd else "back") + ("8" if short else "32")] += 1
        if fwd and short:
            c["extra"] += 3 if op == "jmp" else 4
    size = subprocess.run(["/usr/lib/llvm-22/bin/llvm-size", "-A", o], capture_output=True, text=True).stdout
    c["text"] += sum(int(l.split()[1]) for l in size.splitlines() if l.startswith(".text"))
print(c, f"extra/text = {100 * c['extra'] / c['text']:.1f}%")
