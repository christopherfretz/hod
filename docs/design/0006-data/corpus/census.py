#!/usr/bin/env python3
"""Design 0006 section 4: what the x86-64 sample uses, per module, by
regular expressions over the text IR (a census, not a parser: counts are
of lines or occurrences, and say which).

usage: python3 -I census.py <sample dir> [<aarch64 corpus dir> <capture.tsv>]
  prints: per feature, the number of modules that use it (and occurrences);
  then one line per module with its features (for packages.py).
With the aarch64 corpus and capture.tsv, also counts the same features on
the aarch64 entries of the same tests, for comparison.
"""
import os
import re
import sys

FEATURES = {
    # types
    "x86_fp80": r"\bx86_fp80\b",
    "fp128": r"\bfp128\b",
    "half": r"\bhalf\b",
    "bfloat": r"\bbfloat\b",
    "i128": r"\bi128\b",
    "i128-divrem": r"= (?:s|u)(?:div|rem) i128\b",
    "wide-int(>128 or odd)": r"\bi(?:(?:[2-9]\d\d+)|(?:1[3-9]\d)|(?:[3-9]\d{2,}))\b",
    "vector": r"<\d+ x (?:i\d+|float|double|half|ptr|bfloat)>",
    "float-or-double-op": r"= (?:fadd|fsub|fmul|fdiv|frem|fneg|fcmp) (?:fast |nnan |ninf |nsz |arcp |contract |afn |reassoc )*(?:float|double)\b",
    "fp-conv": r"= (?:fptosi|fptoui|sitofp|uitofp|fpext|fptrunc) ",
    "uitofp/fptoui-64": r"= (?:uitofp i64|fptoui (?:float|double) %[^ ]+ to i64)",
    # memory model
    "atomic": r"\b(?:atomicrmw|cmpxchg)\b|load atomic|store atomic|^\s*fence\b",
    "atomic-16-byte": r"(?:cmpxchg|atomicrmw \w+) ptr %[^,]+, i128\b|load atomic i128|store atomic i128",
    "thread_local": r"\bthread_local\b",
    # calls
    "invoke": r"\binvoke\b",
    "landingpad": r"\blandingpad\b",
    "va_start": r"@llvm\.va_start",
    "va_arg-inst": r"= va_arg ",
    "musttail": r"\bmusttail\b",
    "byval": r"\bbyval\(",
    "sret": r"\bsret\(",
    "inreg": r"\binreg\b",
    "inline-asm": r"\basm (?:sideeffect |inteldialect )*\"|^module asm",
    "indirectbr/blockaddress": r"\bindirectbr\b|\bblockaddress\(",
    "switch": r"^\s*switch ",
    "x86-intrinsic": r"@llvm\.x86\.",
    "memcpy/memset/memmove": r"@llvm\.mem(?:cpy|set|move)\.",
    "bit-intrinsic": r"@llvm\.(?:ctpop|ctlz|cttz|bswap|bitreverse|fshl|fshr)\.",
    "overflow-intrinsic": r"@llvm\.[su](?:add|sub|mul)\.with\.overflow",
    "fp-intrinsic": r"@llvm\.(?:fabs|copysign|floor|ceil|trunc|round|rint|nearbyint|sqrt|fma|fmuladd|minnum|maxnum|minimum|maximum|pow|exp|log|sin|cos|lrint|llrint|lround|llround|is\.fpclass|canonicalize)\.",
    "coroutine": r"@llvm\.coro\.",
    "debug-info": r"^!llvm\.dbg\.cu",
    "cx16-feature": r"\+cx16",
}
COMPILED = {k: re.compile(v, re.M) for k, v in FEATURES.items()}


def scan(path):
    text = open(path, errors="replace").read()
    # the data layout names i128 and f80 in every module
    text = re.sub(r"^target datalayout = .*$", "", text, flags=re.M)
    return {k: len(rx.findall(text)) for k, rx in COMPILED.items()}, text


def table(paths, label):
    mods = {}
    for p in paths:
        mods[p], _ = scan(p)
    n = len(mods)
    print(f"## {label}: {n} modules")
    print("feature\tmodules\toccurrences")
    for k in FEATURES:
        m = sum(1 for v in mods.values() if v[k])
        o = sum(v[k] for v in mods.values())
        print(f"{k}\t{m}\t{o}")
    return mods


def files(root):
    out = []
    for d, _, fs in os.walk(root):
        out += [os.path.join(d, f) for f in fs if f.endswith(".ll")]
    return sorted(out)


sample = sys.argv[1]
kiln = files(os.path.join(sample, "kiln"))
tort = files(os.path.join(sample, "gcc-torture"))
mk = table(kiln, "x86-64 sample, kiln")
mt = table(tort, "x86-64 sample, gcc-torture")
if len(sys.argv) > 3:
    corpus, capture = sys.argv[2], sys.argv[3]
    ok = [l.split("\t")[1] for l in open(capture) if l.rstrip().endswith("\tok")]
    a = [os.path.join(corpus, e) for e in ok]
    table([p for p in a if "/kiln/" in p], "same tests, aarch64 corpus entries, kiln")
    table([p for p in a if "/gcc-torture/" in p], "same tests, aarch64 corpus entries, gcc-torture")
# target attributes kiln and clang write for x86-64
attrs = {}
for p in kiln + tort:
    for m in re.finditer(r'"target-features"="([^"]*)"', open(p).read()):
        attrs[m.group(1)] = attrs.get(m.group(1), 0) + 1
    for m in re.finditer(r'"target-cpu"="([^"]*)"', open(p).read()):
        attrs["cpu=" + m.group(1)] = attrs.get("cpu=" + m.group(1), 0) + 1
print("## target attributes (attribute groups)")
for k, v in sorted(attrs.items(), key=lambda kv: -kv[1]):
    print(f"{v}\t{k}")
print("## per module")
for p, v in list(mk.items()) + list(mt.items()):
    print(os.path.relpath(p, sample) + "\t" + ",".join(k for k in FEATURES if v[k]))
