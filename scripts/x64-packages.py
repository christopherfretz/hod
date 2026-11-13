#!/usr/bin/env python3
"""Design 0006 section 7.2's package census, redone on a whole corpus
(WP-139): in which order the x86-64 corpus opens up.

The method is docs/design/0006-data/corpus/packages.py's, unchanged: each
module needs the packages whose constructs it contains (regular
expressions over the text IR; an over-approximation, a census and not a
classifier), and is freed when every package it needs has landed. The
package list and its order are imported from that script's table so that
the two cannot drift.

What differs is the input: the modules of a corpus's entries
(`spec/harness.md` "Entries"), each `;@ module:` part of an entry of
several modules counted as a module of its own, as the sample counted
files; the lines that start with `;` (directives, comments) are left
out. The counts are printed for the whole corpus and per group
(`<source>/llvm<N>`); `--by-entry` also counts entries, an entry being
freed when all its modules are.

usage: python3 -I scripts/x64-packages.py [--by-entry] <corpus dir>
"""
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DESIGN = os.path.join(HERE, "..", "docs", "design", "0006-data", "corpus", "packages.py")


def design_table():
    """NEEDS from the design's script, without running its census."""
    text = open(DESIGN).read()
    start = text.index("NEEDS = [")
    end = text.index("]\n", start) + 2
    scope = {}
    exec(compile(text[start:end], DESIGN, "exec"), {}, scope)  # a literal list
    return scope["NEEDS"]


NEEDS = design_table()
ORDER = [p for p, _ in NEEDS]
RX = {p: re.compile(r, re.M) for p, r in NEEDS}


def needs(text):
    """packages.py's `needs`, on text."""
    text = re.sub(r"^target datalayout = .*$", "", text, flags=re.M)
    text = re.sub(r'c"(?:[^"\\]|\\.)*"', 'c""', text)
    body = "\n".join(l for l in text.splitlines() if not l.startswith("declare "))
    return frozenset(p for p in ORDER if RX[p].search(body))


def modules(entry):
    """The modules of an entry: the text after each `;@ module:` line, or
    the whole entry."""
    parts = re.split(r"^;@ module: .*$", entry, flags=re.M)
    parts = parts[1:] if len(parts) > 1 else parts
    # The entry's directives and comments (an expected `stdout:` line can
    # read like IR) are not the module.
    return ["\n".join(l for l in p.splitlines() if not l.startswith(";")) for p in parts]


def census(mods):
    """(rows, core counts): rows are (package, needed by, frees,
    cumulative) in ORDER, as packages.py prints them."""
    rows = []
    landed = set()
    total = 0
    for i, p in enumerate(ORDER):
        landed.add(p)
        freed = sum(1 for n in mods if p in n and n <= landed)
        if i == 0:
            freed += sum(1 for n in mods if not n)
        total += freed
        rows.append((p, sum(1 for n in mods if p in n), freed, total))
    core = {"X-int", "X-mem", "X-control", "X-call"}
    extra = [
        ("core (int, mem, control, call) alone", core),
        ("core + float", core | {"X-float"}),
        ("core + aggr", core | {"X-aggr"}),
        ("core + intrinsic", core | {"X-intrinsic"}),
        ("core + float + aggr + intrinsic", core | {"X-float", "X-aggr", "X-intrinsic"}),
    ]
    return rows, [(name, sum(1 for n in mods if n <= s)) for name, s in extra]


def show(title, mods):
    rows, extra = census(mods)
    print(f"## {title}: {len(mods)}")
    print("package\tneeded by\tfrees\tcumulative")
    for row in rows:
        print("\t".join(str(x) for x in row))
    for name, n in extra:
        print(f"{name}: {n}")
    print()


def main(argv):
    by_entry = "--by-entry" in argv
    dirs = [a for a in argv if a != "--by-entry"]
    if len(dirs) != 1:
        sys.exit(__doc__)
    root = dirs[0]
    groups = {}
    for d, _, fs in os.walk(root):
        rel = os.path.relpath(d, root).split(os.sep)
        if len(rel) < 2 or not rel[1].startswith("llvm"):
            continue  # not an entry directory (the tables, `*.work`)
        group = f"{rel[0]}/{rel[1]}"
        for f in fs:
            if f.endswith(".ll"):
                text = open(os.path.join(d, f), errors="replace").read()
                groups.setdefault(group, []).append([needs(m) for m in modules(text)])
    everything = [m for entries in groups.values() for e in entries for m in e]
    show("modules, whole corpus", everything)
    for group in sorted(groups):
        show(f"modules, {group}", [m for e in groups[group] for m in e])
    if by_entry:
        merged = [frozenset().union(*e) for entries in groups.values() for e in entries]
        show("entries, whole corpus", merged)


if __name__ == "__main__":
    main(sys.argv[1:])
