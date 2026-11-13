#!/usr/bin/env python3
"""Design 0006 section 6.2: what emulation costs per test.

For each sampled kiln test: the aarch64 corpus entry built with llc 22
(SelectionDAG) and g++, run natively; the x86-64 sample module built the
same way with the cross g++ and run under qemu-x86_64. Times the build
(llc + link) and the run of each, best of three runs, and checks that both
programs print the same thing and exit alike.

usage: python3 -I emu-cost.py <corpus dir> <x86 sample dir> <work dir> <capture.tsv> [N]
"""
import os
import subprocess
import sys
import time

corpus, sample, work, capture = sys.argv[1:5]
N = int(sys.argv[5]) if len(sys.argv) > 5 else 12
os.makedirs(work, exist_ok=True)


def timed(cmd, timeout=60):
    t = time.monotonic()
    p = subprocess.run(cmd, capture_output=True, timeout=timeout)
    return time.monotonic() - t, p


entries = [l.split("\t")[1] for l in open(capture) if l.rstrip().endswith("\tok") and l.startswith("kiln")]
print("test\tbuild-a64\trun-a64\tbuild-x64\trun-x64(qemu)\tsame-output")
sums = [0.0] * 4
n = 0
for e in entries:
    if n == N:
        break
    a_ll = os.path.join(corpus, e)
    name = e.split("/", 2)[2].replace("/", "__")
    x_ll = os.path.join(sample, "kiln", name)
    if not (os.path.isfile(a_ll) and os.path.isfile(x_ll)):
        continue
    row = []
    outs = []
    for ll, triple, cxx, runner, tag in (
        (a_ll, "aarch64-unknown-linux-gnu", "g++", [], "a"),
        (x_ll, "x86_64-unknown-linux-gnu", "x86_64-linux-gnu-g++", ["qemu-x86_64", "-L", "/usr/x86_64-linux-gnu"], "x"),
    ):
        obj, exe = os.path.join(work, name + tag + ".o"), os.path.join(work, name + tag)
        tb1, p1 = timed(["/usr/lib/llvm-22/bin/llc", f"-mtriple={triple}", "-O0", "-fast-isel=0",
                         "-relocation-model=pic", "-filetype=obj", ll, "-o", obj])
        tb2, p2 = timed([cxx, "-pie", obj, "-o", exe, "-lm"])
        if p1.returncode or p2.returncode:
            row = None
            break
        best = None
        for _ in range(3):
            tr, p = timed([*runner, exe], timeout=30)
            best = tr if best is None else min(best, tr)
        outs.append((p.returncode, p.stdout))
        row += [tb1 + tb2, best]
    if row is None:
        continue
    n += 1
    for i in range(4):
        sums[i] += row[i]
    print(f"{name}\t" + "\t".join(f"{v:.3f}" for v in row) + f"\t{outs[0] == outs[1]}")
print(f"MEAN of {n}\t" + "\t".join(f"{v / n:.3f}" for v in sums))
