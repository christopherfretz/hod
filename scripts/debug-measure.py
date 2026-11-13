#!/usr/bin/env python3
"""The M3 gate's per-module measurements (docs/status/debug.md, WP-099).

For every `-g` module (an `.ll` with `!llvm.dbg.cu`) among the paths:

1. builds the object with `llc -O0 -global-isel=0 -fast-isel=0
   -filetype=obj -relocation-model=pic` of the module's LLVM (the
   reference, docs/VALIDATION.md "Oracles") and with `hod --strict` and
   the same options (as `xtask debug verify` does: spec/harness.md
   "debug", "verify"); both get the module's `;@ llc-args:`;
2. runs `llvm-dwarfdump --verify` of LLVM 22 and of 19 on both objects;
3. when both are built, links each into a position-independent executable without start
   files and with undefined symbols allowed (`gcc -pie -nostartfiles
   -Wl,--unresolved-symbols=ignore-all`),
   so that a module without `main` or with undefined symbols still gets
   one address per function) and compares the `(function, file, line)`
   pairs by the rule of `xtask/src/debug/lines.rs` (spec/harness.md
   "debug", "Lines"): every `is_stmt` row with a line other than 0 whose
   range to the next row is not empty, the function being the defined
   `T`/`t`/`W`/`w` symbol with a size that holds the address. A pair only
   hod's has at its function's first row's line is P3, one only `llc`'s
   has whose every range holds only `b` is P1 (spec/dwarf.md 5.7); the
   others are classified by the module's IR, by the rule of
   `xtask/src/debug/permitted.rs` (`IrFacts::pair_class`): when the
   function's instructions at the pair's line (any column) are loads and
   `getelementptr`s, at least one load, the pair is `load-placement`;
   else a pair only hod's has whose instructions there are
   `getelementptr`s of pointers some load of the function reads is
   `post-indexed-load`; else a pair only hod's has, in a function that
   calls `llvm.is.constant`, is `is-constant`; else unclassified. Only
   the instructions of the pair's file (the `DIFile` of the location's
   scope) and of blocks with a predecessor count.

Usage:

    scripts/debug-measure.py --hod <hod> --work <dir> [-j N] [--major N]
                             <corpus dir or .ll>... > results.tsv

The LLVM major of a module is its `;@ llvm:` directive, else a path
component `llvm<N>` or `clang<N>`, else `--major` (default 22). Output: one
line per module, tab-separated: module, major, hod outcome (`built`,
`declined:<reason>`, `failed`), llc outcome (`built`, `failed`), hod's
verify (`clean`, `-`, or the first message's class), llc's verify, then
`same`, `hod_only`, `llc_only` counts and `P1`, `P3`, `load-placement`,
`post-indexed-load`, `is-constant`, `unclassified` counts (or `-` when there was no comparison). Each module's differing
pairs are written to `<work>/<n>/pairs.tsv` (side, class, function,
file, line, the instructions of the side's rows). Nothing is written outside `--work`. `-j` is at most 4.
"""
import argparse
import bisect
import concurrent.futures
import os
import re
import resource
import subprocess
import sys

LIMIT_SECONDS = 600
LIMIT_BYTES = 8 << 30
REFERENCE = ["-O0", "-global-isel=0", "-fast-isel=0"]
COMMON = ["-filetype=obj", "-relocation-model=pic"]


def limits():
    resource.setrlimit(resource.RLIMIT_AS, (LIMIT_BYTES, LIMIT_BYTES))


def run(args, cwd=None):
    env = dict(os.environ, LC_ALL="C")
    for v in ("HOD_FALLBACK_LLC", "HOD_STRICT", "HOD_COVERAGE_LOG", "HOD_IN_FALLBACK"):
        env.pop(v, None)
    try:
        p = subprocess.run(args, cwd=cwd, env=env, capture_output=True,
                           timeout=LIMIT_SECONDS, preexec_fn=limits)
        return p.returncode, p.stdout.decode("utf-8", "replace"), p.stderr.decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        return -1, "", "timed out"


def tool(name, major):
    for c in (f"/usr/lib/llvm-{major}/bin/{name}", f"/usr/bin/{name}-{major}"):
        if os.path.exists(c):
            return c
    sys.exit(f"no {name} of LLVM {major}")


def directives(text):
    out = {}
    for line in text.splitlines():
        if line.startswith(";@ "):
            key, _, value = line[3:].partition(":")
            out.setdefault(key.strip(), value.strip())
    return out


def major_of(path, d, default):
    if "llvm" in d:
        return int(d["llvm"].split()[0])
    for part in reversed(path.split(os.sep)):
        m = re.fullmatch(r"(?:llvm|clang)(\d+)", part)
        if m:
            return int(m.group(1))
    return default


def verify_class(dwarfdump, obj):
    code, out, err = run([dwarfdump, "--verify", obj])
    if code == 0:
        return "clean"
    for line in (out + err).splitlines():
        line = line.strip()
        if line.startswith("error:"):
            msg = re.sub(r"0x[0-9a-fA-F]+", "0x?", line[6:].strip())
            msg = re.sub(r"\b\d+\b", "N", msg)
            msg = re.sub(r"\"[^\"]*\"", "\"?\"", msg)
            return msg[:160]
    return "status %d" % code


# The pairs, as xtask/src/debug/lines.rs reads them.
def line_rows(text):
    rows, dirs, files, current = [], {}, {}, None
    idx = re.compile(r"\[\s*(\d+)\]")
    for raw in text.splitlines():
        t = raw.strip()
        if t.startswith("debug_line["):
            dirs, files, current = {}, {}, None
        elif t.startswith("include_directories["):
            m = idx.search(t)
            if m:
                dirs[int(m.group(1))] = t.split('"')[1] if '"' in t else ""
        elif t.startswith("file_names["):
            m = idx.search(t)
            current = int(m.group(1)) if m else None
        elif current is not None and t.startswith("name:"):
            files.setdefault(current, ["", 0])[0] = t.split('"')[1] if '"' in t else ""
        elif current is not None and t.startswith("dir_index:"):
            files.setdefault(current, ["", 0])[1] = int(t.split(":")[1])
        elif t.startswith("0x"):
            w = t.split()
            try:
                address, line, fileno = int(w[0], 16), int(w[1]), int(w[3])
            except (ValueError, IndexError):
                continue
            name, d = files.get(fileno, ["", 0])
            path = name if name.startswith("/") or d not in dirs else dirs[d] + "/" + name
            rows.append((address, path, line, "is_stmt" in w, "end_sequence" in w))
    return rows


def symbols(text):
    out = []
    for line in text.splitlines():
        w = line.split()
        if len(w) < 4:
            continue
        try:
            start, size = int(w[0], 16), int(w[1], 16)
        except ValueError:
            continue
        if size > 0 and w[2] in ("T", "t", "W", "w"):
            out.append((start, start + size, w[3]))
    out.sort()
    return out


def pairs_of(rows, syms):
    starts = [s[0] for s in syms]
    ranges, first = {}, {}
    for row, nxt in zip(rows, rows[1:]):
        if row[4] or nxt[0] <= row[0]:
            continue
        i = bisect.bisect_right(starts, row[0]) - 1
        if i < 0 or not (syms[i][0] <= row[0] < syms[i][1]):
            continue
        fn = syms[i][2]
        if row[2] != 0:
            first.setdefault(fn, (row[1], row[2]))
            if row[3]:
                ranges.setdefault((fn, row[1], row[2]), []).append((row[0], nxt[0]))
    return ranges, first


def instructions(text):
    code = {}
    for line in text.splitlines():
        a, sep, rest = line.partition(":")
        a = a.strip()
        if not sep or not a or not all(c in "0123456789abcdefABCDEF" for c in a):
            continue
        w = rest.split()
        if w:
            code[int(a, 16)] = w[0]
    return code


def read_pairs(lib):
    _, lines, _ = run([tool("llvm-dwarfdump", 22), "--debug-line", lib])
    _, nm, _ = run([tool("llvm-nm", 22), "-S", "--defined-only", lib])
    return pairs_of(line_rows(lines), symbols(nm))


def disassembly(lib):
    _, dis, _ = run([tool("llvm-objdump", 22), "-d", "--no-show-raw-insn", lib])
    code = instructions(dis)
    return code, sorted(code)


def mnemonics(code, ranges):
    """The instructions in `ranges`, in order: `ldr,str,bl`."""
    code, addrs = code
    out = []
    for lo, hi in ranges:
        out += [code[a] for a in addrs[bisect.bisect_left(addrs, lo):bisect.bisect_left(addrs, hi)]]
    return ",".join(out)


DBG = re.compile(r"!dbg (![0-9]+)")
LOC = re.compile(r"^(![0-9]+) = !DILocation\(line: ([0-9]+)")
DEF = re.compile(r"^define [^@]*@(\"[^\"]+\"|[-\w.$]+)\(")
OPC = re.compile(r"^\s*(?:%[-\w.$\"]+ = )?(?:tail |musttail |notail )?([a-z_]+)")
NODE = re.compile(r"^(![0-9]+) = (?:distinct )?!(DI\w+)\((.*)")
REF = lambda name: re.compile(r"\b" + name + r": (![0-9]+)")
STR = lambda name: re.compile(r"\b" + name + r": \"([^\"]*)\"")
PTR = re.compile(r"ptr (%\"[^\"]*\"|%[-\w.$]+)")
LABEL_REF = re.compile(r"label (%\"[^\"]*\"|%[-\w.$]+)")


def ir_files(text):
    """`DILocation` id: the joined path of its scope's `DIFile` (as the
    line table joins it: the name when absolute, else directory/name)."""
    names, scopes, locs = {}, {}, {}
    for line in text.splitlines():
        m = NODE.match(line)
        if not m:
            continue
        node, kind, rest = m.groups()
        if kind == "DIFile":
            n, d = STR("filename").search(rest), STR("directory").search(rest)
            n, d = n.group(1) if n else "", d.group(1) if d else ""
            names[node] = n if n.startswith("/") or not d else d + "/" + n
        elif kind == "DILocation":
            r = REF("scope").search(rest)
            if r:
                locs[node] = r.group(1)
        else:
            r = REF("file").search(rest)
            if r:
                scopes[node] = r.group(1)
    return {loc: names.get(scopes.get(scope)) for loc, scope in locs.items()}


def ir_facts(text):
    """Per defined function: the (line, opcode, file, reachable, pointer)
    of each instruction with a `!dbg`, and whether it calls
    `llvm.is.constant` (xtask/src/debug/permitted.rs: the file of the
    location's scope, whether its block is the entry block or named by a
    `label %N`, and a load's or `getelementptr`'s pointer operand)."""
    locs, funcs, name = {}, {}, None
    files = ir_files(text)
    for line in text.splitlines():
        m = LOC.match(line)
        if m:
            locs[m.group(1)] = int(m.group(2))
    block, blocks, named = None, [], set()
    for line in text.splitlines():
        m = DEF.match(line)
        if m:
            name = m.group(1).strip('"')
            funcs[name] = ([], False)
            block, blocks, named = None, [], set()
            continue
        if name is None:
            continue
        if line.startswith("}"):
            insts, is_constant = funcs[name]
            funcs[name] = ([(l, o, f, b is None or b in named, p) for (l, o, f, p), b in zip(insts, blocks)], is_constant)
            name = None
            continue
        if line and line[0] not in " \t;":
            if ":" in line:
                block = "%" + line.split(":")[0]
            continue
        named.update(r if not r.startswith('%"') else '%' + r[2:-1] for r in LABEL_REF.findall(line))
        insts, is_constant = funcs[name]
        d, o = DBG.search(line), OPC.match(line)
        if d and o and d.group(1) in locs:
            p = PTR.search(line) if o.group(1) in ("load", "getelementptr") else None
            insts.append((locs[d.group(1)], o.group(1), files.get(d.group(1)), p.group(1) if p else None))
            blocks.append(block.replace('%"', "%").rstrip('"') if block else None)
        funcs[name] = (insts, is_constant or "@llvm.is.constant." in line)
    return funcs


def ir_class(facts, fn, file, line, hod_side):
    """`load-placement`, `post-indexed-load`, `is-constant` (spec/dwarf.md
    5.7), or unclassified: by the function's instructions at the pair's
    line of the pair's file, in blocks with a predecessor."""
    insts, is_constant = facts.get(fn, ([], False))
    at = [i for i in insts if i[0] == line and i[3] and (i[2] is None or i[2] == file)]
    ops = {i[1] for i in at}
    if "load" in ops and ops <= {"load", "getelementptr"}:
        return "load-placement"
    loaded = {i[4] for i in insts if i[1] == "load" and i[4]}
    if hod_side and at and all(i[1] == "getelementptr" and i[4] in loaded for i in at):
        return "post-indexed-load"
    if hod_side and is_constant:
        return "is-constant"
    return "unclassified"


def compare(hod_lib, llc_lib, facts):
    """`same` and the differing pairs: (side, class, pair, instructions of its rows)."""
    (ours, first), (theirs, _) = read_pairs(hod_lib), read_pairs(llc_lib)
    hod_code, llc_code = disassembly(hod_lib), disassembly(llc_lib)
    same, diffs = 0, []
    for key in sorted(set(ours) | set(theirs)):
        if key in ours and key in theirs:
            same += 1
        elif key in ours:
            p3 = first.get(key[0]) == (key[1], key[2])
            diffs.append(("hod", "P3" if p3 else ir_class(facts, key[0], key[1], key[2], True), key, mnemonics(hod_code, ours[key])))
        else:
            # P1: every range holds only unconditional branches.
            p1 = all(m and set(m.split(",")) <= {"b", "jmp"}
                     for m in (mnemonics(llc_code, [r]) for r in theirs[key]))
            diffs.append(("llc", "P1" if p1 else ir_class(facts, key[0], key[1], key[2], False), key, mnemonics(llc_code, theirs[key])))
    return same, diffs


def measure(job):
    n, path, args = job
    work = os.path.join(args.work, str(n))
    os.makedirs(work, exist_ok=True)
    with open(os.path.join(work, "module"), "w") as f:
        f.write(path + "\n")
    text = open(path, encoding="utf-8", errors="replace").read()
    d = directives(text)
    major = major_of(path, d, args.major)
    extra = d.get("llc-args", "").split()
    extra = [a for a in extra if a not in ("-O0",) and not a.startswith("-filetype") and not a.startswith("-relocation-model")]
    llc_o, hod_o, log = (os.path.join(work, f) for f in ("llc.o", "hod.o", "coverage.tsv"))
    code, _, err = run([tool("llc", major)] + REFERENCE + COMMON + extra + ["-o", llc_o, path])
    llc = "built" if code == 0 and os.path.exists(llc_o) else "failed"
    code, _, err = run([args.hod, "--coverage-log=" + log, "--strict", "-O0"] + COMMON + extra + ["-o", hod_o, path])
    cov = open(log).read().split("\n")[0].split("\t") if os.path.exists(log) else [""]
    if code == 0 and os.path.exists(hod_o) and cov[0] == "hod":
        hod = "built"
    elif code == 2 and cov[0] == "error":
        hod = "declined:" + (cov[1] if len(cov) > 1 else "?")
    else:
        hod = "failed:" + (err.strip().splitlines() or ["status %d" % code])[-1][:120]
    hv = lv = "-"
    if hod == "built":
        hv = ";".join(sorted({verify_class(tool("llvm-dwarfdump", m), hod_o) for m in (22, 19)}))
    if llc == "built":
        lv = ";".join(sorted({verify_class(tool("llvm-dwarfdump", m), llc_o) for m in (22, 19)}))
    cmp = ["-"] * 9
    if hod == "built" and llc == "built":
        libs = []
        for o in (hod_o, llc_o):
            lib = o[:-2] + ".so"
            run(["gcc", "-pie", "-nostartfiles", "-Wl,--unresolved-symbols=ignore-all",
                 "-Wl,--defsym=__dso_handle=0", "-o", lib, o])
            libs.append(lib)
        if all(os.path.exists(x) for x in libs):
            same, diffs = compare(*libs, ir_facts(text))
            with open(os.path.join(work, "pairs.tsv"), "w") as f:
                for side, cls, (fn, file, line), insns in diffs:
                    f.write(f"{side}\t{cls}\t{fn}\t{file}\t{line}\t{insns}\n")
            count = lambda s=None, c=None: sum(1 for x in diffs if (s is None or x[0] == s) and (c is None or x[1] == c))
            cmp = [same, count("hod"), count("llc"), count(c="P1"), count(c="P3"),
                   count(c="load-placement"), count(c="post-indexed-load"),
                   count(c="is-constant"), count(c="unclassified")]
        else:
            cmp = ["nolink"] * 9
    for f in (hod_o, llc_o):
        if os.path.exists(f) and not args.keep:
            os.remove(f)
    return "\t".join(str(x) for x in [path, major, hod, llc, hv, lv] + cmp)


def modules(paths):
    out = []
    for p in paths:
        if os.path.isdir(p):
            for root, _, names in os.walk(p):
                out += [os.path.join(root, n) for n in names if n.endswith(".ll")]
        else:
            out.append(p)
    return sorted(m for m in set(out) if "!llvm.dbg.cu" in open(m, errors="replace").read())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--hod", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("-j", type=int, default=4)
    ap.add_argument("--major", type=int, default=22)
    ap.add_argument("--keep", action="store_true", help="keep the objects")
    ap.add_argument("paths", nargs="+")
    args = ap.parse_args()
    args.j = max(1, min(args.j, 4))
    mods = modules(args.paths)
    print("module\tmajor\thod\tllc\thod_verify\tllc_verify\tsame\thod_only\tllc_only\tP1\tP3\tload-placement\tpost-indexed-load\tis-constant\tunclassified")
    with concurrent.futures.ThreadPoolExecutor(args.j) as pool:
        for line in pool.map(measure, [(n, m, args) for n, m in enumerate(mods)]):
            print(line, flush=True)


if __name__ == "__main__":
    main()
