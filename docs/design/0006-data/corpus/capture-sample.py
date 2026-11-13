#!/usr/bin/env python3
"""Design 0006 section 4: an x86-64 sample of the corpus, for counts.

Takes every Nth entry of the corpus manifest (LLVM 22; kiln and
gcc-torture separately), recompiles its source for x86_64-unknown-linux-gnu
with the entry's own producer and arguments (`kiln++ --target=...` /
`clang-22 --target=...`), and writes the text IR to <out>/<source>/<name>.ll.
Not the corpus: no program, no expected output, no manifest. kiln's
`-S -emit-llvm` is the IR kiln writes, before any `opt` kiln's driver
would run for `alwaysinline` or coroutines.

usage: python3 -I capture-sample.py <manifest> <kiln repo> <kiln++> <torture dir> <out> [n_kiln n_torture | <entry list>]
"""
import os
import subprocess
import sys

manifest, kiln_repo, kiln, torture, out = sys.argv[1:6]
# a sixth argument that is a file lists the entries to take instead
listed = None
if len(sys.argv) > 6 and os.path.isfile(sys.argv[6]):
    listed = [l.strip() for l in open(sys.argv[6]) if l.strip()]
    n_kiln = n_tort = 10**6
else:
    n_kiln = int(sys.argv[6]) if len(sys.argv) > 6 else 50
    n_tort = int(sys.argv[7]) if len(sys.argv) > 7 else 25
TRIPLE = "x86_64-unknown-linux-gnu"

rows = [line.rstrip("\n").split("\t") for line in open(manifest)][1:]


def pick(source, n):
    sel = [r for r in rows if r[1] == source and r[2] == "22"]
    # single-source kiln tests whose arguments need no substitution
    sel = [r for r in sel if "%{" not in r[6] and r[0].endswith(".ll")]
    if listed is not None:
        return [r for r in sel if r[0] in listed]
    step = max(1, len(sel) // n)
    return sel[::step][:n]


def run(cmd, cwd=None, env=None):
    p = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, timeout=120)
    return p.returncode, p.stderr.decode(errors="replace")


env = dict(os.environ, LC_ALL="C", XDG_CACHE_HOME=os.path.join(out, "cache"),
           TMPDIR=os.path.join(out, "tmp"))
env.pop("KILN_LLVM_DIR", None)
os.makedirs(env["TMPDIR"], exist_ok=True)
for source, n in (("kiln", n_kiln), ("gcc-torture", n_tort)):
    for r in pick(source, n):
        entry, test, args = r[0], r[3], r[6].split()
        name = entry.split("/", 2)[2].replace("/", "__")
        dst = os.path.join(out, source, name)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        if source == "kiln":
            args = [a for a in args if not a.startswith("-fdebug-prefix-map")]
            src = args[-1]
            cmd = [kiln, f"--target={TRIPLE}", "--llvm-dir=/usr/lib/llvm-22",
                   *args[:-1], "-S", "-emit-llvm", src, "-o", dst]
            rc, err = run(cmd, cwd=kiln_repo, env=env)
        else:
            src = None
            for root, _, files in os.walk(torture):
                if test in files and "execute" in root:
                    src = os.path.join(root, test)
                    break
            flags = [a for a in args if a not in ("-S", "-emit-llvm") and a != test]
            cmd = ["/usr/lib/llvm-22/bin/clang", f"--target={TRIPLE}", *flags,
                   "-S", "-emit-llvm", src or test, "-o", dst]
            rc, err = run(cmd, cwd=os.path.dirname(src) if src else None, env=env)
        print(f"{source}\t{entry}\t{'ok' if rc == 0 else 'FAILED ' + err.strip()[:150]}")
        sys.stdout.flush()
