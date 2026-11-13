#!/usr/bin/env python3
"""Design 0006 section 7.2: in which order the x86-64 sample opens up.

Each module needs the packages whose constructs it contains (detected by
regular expressions over the text IR; a census, not a classifier: an
over-approximation where a construct is mentioned but would not need the
package, e.g. a declaration nobody calls). A module is freed when every
package it needs has landed. Prints, for the proposed order, how many
modules each package frees, then the modules still waiting.

usage: python3 -I packages.py <dir with .ll files, searched recursively>...
"""
import os
import re
import sys

# package -> what makes a module need it (beyond the skeleton S0)
NEEDS = [
    ("X-int", r"= (?:add|sub|mul|[su]div|[su]rem|shl|[la]shr|and|or|xor|icmp|zext|sext|trunc|select|ptrtoint|inttoptr|freeze) "),
    ("X-mem", r"= (?:load|alloca|getelementptr)\b|^\s*store "),
    ("X-control", r"^\s*br i1 |^\s*switch |= phi |^\s*indirectbr |blockaddress\("),
    ("X-call", r"(?:= |^\s*)(?:tail |musttail |notail )?call [^@]*@(?!llvm\.)|^\s*invoke |\bbyval\(|\bsret\(|@llvm\.va_(?:start|end|copy)|landingpad|^\s*resume "),
    ("X-float", r"\b(?:float|double|half|bfloat|fp128)\b"),
    ("X-aggr", r"\bi(?:12[89]|1[3-9]\d|[2-9]\d\d+)\b|= (?:extractvalue|insertvalue) |\.with\.overflow|landingpad"),
    ("X-intrinsic", r"@llvm\.(?!lifetime|dbg|va_|threadlocal|eh\.typeid|coro|donothing|experimental\.noalias)"),
    ("X-x87", r"\bx86_fp80\b"),
    ("X-atomic", r"\b(?:atomicrmw|cmpxchg)\b|load atomic|store atomic|^\s*fence\b"),
    ("X-tls", r"\bthread_local\b|@llvm\.threadlocal"),
    ("X-vector", r"<\d+ x (?:i\d+|float|double|half|ptr|bfloat)>"),
    ("X-asm", r"\basm (?:sideeffect |inteldialect )*\"|^module asm"),
    ("C7-coro", r"@llvm\.coro\."),
]
ORDER = [p for p, _ in NEEDS]
RX = {p: re.compile(r, re.M) for p, r in NEEDS}


def needs(path):
    text = open(path, errors="replace").read()
    text = re.sub(r"^target datalayout = .*$", "", text, flags=re.M)
    text = re.sub(r'c"(?:[^"\\]|\\.)*"', 'c""', text)  # string constants
    # declarations alone do not need a package
    body = "\n".join(l for l in text.splitlines() if not l.startswith("declare "))
    return {p for p in ORDER if RX[p].search(body)}


paths = []
for root in sys.argv[1:]:
    for d, _, fs in os.walk(root):
        paths += [os.path.join(d, f) for f in fs if f.endswith(".ll")]
paths.sort()
mods = {p: needs(p) for p in paths}
print(f"{len(mods)} modules")
landed = set()
freed_total = 0
print("package\tneeded by\tfrees\tcumulative")
for p in ORDER:
    landed.add(p)
    freed = sum(1 for n in mods.values() if p in n and n <= landed)
    freed0 = sum(1 for n in mods.values() if not n) if p == ORDER[0] else 0
    freed_total += freed + freed0
    need = sum(1 for n in mods.values() if p in n)
    print(f"{p}\t{need}\t{freed + freed0}\t{freed_total}")
# what the four core groups alone give
core = {"X-int", "X-mem", "X-control", "X-call"}
print(f"core (int, mem, control, call) alone: {sum(1 for n in mods.values() if n <= core)}")
print(f"core + float: {sum(1 for n in mods.values() if n <= core | {'X-float'})}")
print(f"core + aggr: {sum(1 for n in mods.values() if n <= core | {'X-aggr'})}")
print(f"core + intrinsic: {sum(1 for n in mods.values() if n <= core | {'X-intrinsic'})}")
print(f"core + float + aggr + intrinsic: {sum(1 for n in mods.values() if n <= core | {'X-float', 'X-aggr', 'X-intrinsic'})}")
