#!/usr/bin/env python3
"""Explain the unclassified `(function, line)` pairs of scripts/debug-measure.py
by the IR they come from (docs/status/debug.md, WP-099).

For each unclassified pair of `<work>/<n>/pairs.tsv`, reads the module
(`<work>/<n>/module`), finds the function's definition and the
instructions whose `!dbg` location is on the pair's line (any column,
any scope), and prints one line, tab-separated:

    side  cause  opcodes-of-the-line  instructions-of-the-side's-rows  function  line  module

`cause` is the class the report uses (docs/status/debug.md "Line
tables"), decided by the IR alone:

- `unreachable`: hod only; the line's only instruction is `unreachable`
  (hod writes a trap, `llc` no code);
- `gep-placement`: the line's instructions are `getelementptr`s only:
  one writer has code for the address at its own line, the other folds
  it into its user (another line);
- `is-constant`: hod only, in a function that calls `llvm.is.constant`
  (`llc` folds it and deletes the blocks it guards, even at `-O0`);
- `compare-at-user`: `llc` only; the line's instructions are compares
  (`llc`'s code of the compare is at its line, hod's at its user's);
- `load-placement`: the line's instructions are loads (and
  `getelementptr`s): `llc` merges a load whose value is unused with a
  later load of the same address and keeps the first one's line (`llc`
  only), or places the load of a call's argument at the call (hod only);
- `other`: anything else (read by hand).

Usage: scripts/debug-pairs-ir.py <work dir>
"""
import os
import re
import sys

DBG = re.compile(r"!dbg (![0-9]+)")
LOC = re.compile(r"^(![0-9]+) = !DILocation\(line: ([0-9]+)")
DEF = re.compile(r"^define [^@]*@(\"[^\"]+\"|[-\w.$]+)\(")
OPC = re.compile(r"^\s*(?:(%[-\w.$\"]+) = )?(?:tail |musttail |notail )?([a-z_]+)")


def load(path):
    locs, funcs, name, body = {}, {}, None, []
    for line in open(path, encoding="utf-8", errors="replace"):
        m = LOC.match(line)
        if m:
            locs[m.group(1)] = int(m.group(2))
            continue
        m = DEF.match(line)
        if m:
            name, body = m.group(1).strip('"'), []
            continue
        if name is not None:
            if line.startswith("}"):
                funcs[name], name = body, None
            else:
                body.append(line.rstrip("\n"))
    return locs, funcs


def cause(side, insts, body):
    ops = [op for _, op, _ in insts]
    if side == "hod" and ops and all(op == "unreachable" for op in ops):
        return "unreachable"
    if ops and all(op == "getelementptr" for op in ops):
        return "gep-placement"
    if side == "hod" and any("@llvm.is.constant" in text for text in body):
        return "is-constant"
    if side == "llc" and ops and all(op in ("icmp", "fcmp") for op in ops):
        return "compare-at-user"
    if ops and all(op in ("load", "getelementptr") for op in ops) and "load" in ops:
        return "load-placement"
    return "other"


def main():
    work = sys.argv[1]
    cache = {}
    for n in sorted(os.listdir(work), key=lambda s: int(s) if s.isdigit() else -1):
        pairs = os.path.join(work, n, "pairs.tsv")
        if not os.path.exists(pairs):
            continue
        module = open(os.path.join(work, n, "module")).read().strip()
        for row in open(pairs):
            side, cls, fn, _file, line, insns = row.rstrip("\n").split("\t")
            if cls != "unclassified":
                continue
            if module not in cache:
                cache = {module: load(module)}
            locs, funcs = cache[module]
            body = funcs.get(fn, [])
            insts = []
            for text in body:
                m = DBG.search(text)
                o = OPC.match(text)
                if m and o and locs.get(m.group(1)) == int(line):
                    insts.append((o.group(1), o.group(2), text))
            print("\t".join([side, cause(side, insts, body), ",".join(op for _, op, _ in insts),
                             insns, fn, line, module]))


if __name__ == "__main__":
    main()
