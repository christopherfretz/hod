"""Alloca classes of pre-split coroutines (WP-100, design 0005 §3.4).

usage: python3 -I coro-slots.py [-v] <module.ll>...

For every alloca of every presplitcoroutine body:
  escaped   some use, of it or of a pointer derived from it by GEP or cast,
            other than a load from, a store to, a memcpy/memmove/memset on,
            a lifetime marker, a #dbg_* record, or the promise operand of
            coro.id (a store OF the address, a call argument, a phi, a
            select, a compare or ptrtoint escape). Derived pointers that
            cross a suspend are not detected here (none at -O0 in kiln);
  markers   it has llvm.lifetime.start;
  outside   it carries !coro.outside.frame;
  pre-begin some use lies on a path from entry that has not passed coro.begin;
  post-free some use is reachable from the operator delete of coro.free's
            result (the frame is gone there);
  suspended some use is on the default (-1) edge of a suspend's switch or
            after it, up to the return: the suspending activation, while
            the coroutine may already run or be destroyed elsewhere.
Prints totals per class and, with -v, each alloca of the interesting classes
with its name stripped of numeric suffixes.
"""
import re
import sys
from collections import Counter

ATTR = re.compile(r"^attributes #(\d+) = \{(.*)\}")
DEFINE = re.compile(r"^define [^@]*@(\"[^\"]+\"|[\w.$]+)\((.*)\)(.*)\{")
LABEL = re.compile(r"^([\w.$-]+|\"[^\"]+\"):")
ALLOCA = re.compile(r"^\s*(%[\w.$-]+) = alloca ")


def tok(name):
    return re.compile(re.escape(name) + r"(?![\w.$-])")


def blocks_of(body):
    blocks, order, cur = {}, [], "%entry0"
    blocks[cur] = []
    order.append(cur)
    for line in body:
        m = LABEL.match(line)
        if m:
            cur = "%" + m.group(1).strip('"')
            blocks[cur] = []
            order.append(cur)
            continue
        if line.strip():
            blocks[cur].append(line)
    succ = {b: set(re.findall(r"label (%[\w.$-]+)", "\n".join(ls))) for b, ls in blocks.items()}
    return blocks, order, succ


def reach(starts, succ, stop=frozenset()):
    seen, work = set(), list(starts)
    while work:
        b = work.pop()
        if b in seen or b in stop:
            continue
        seen.add(b)
        work.extend(succ.get(b, ()))
    return seen


MEM = re.compile(r"@llvm\.(memcpy|memmove|memset)[\w.]*\(")
DERIVE = re.compile(r"^\s*(%[\w.$-]+) = (getelementptr|bitcast|addrspacecast)\b")


def use_kind(line, name):
    """How `line` uses pointer `name` (the alloca or a pointer derived from it)."""
    if "#dbg_" in line:
        return None
    if "@llvm.lifetime." in line:
        return "lifetime"
    if re.search(r"= load [^,]+, ptr " + re.escape(name) + r"(?![\w.$-])", line):
        return "load"
    m = re.match(r"\s*store (.*), ptr " + re.escape(name) + r"(?![\w.$-])", line)
    if m and not tok(name).search(m.group(1)):
        return "store"
    if MEM.search(line):
        return "mem"
    if "@llvm.coro.id(" in line:
        return "promise"
    m = DERIVE.match(line)
    if m:
        return "derive:" + m.group(1)
    if re.search(r"\b(call|invoke)\b", line):
        return "call"
    if line.lstrip().startswith("store "):
        return "store-of"
    return "other"


def analyse(path, verbose, total, shapes):
    text = open(path, encoding="utf-8", errors="replace").read()
    presplit = {m.group(1) for line in text.splitlines()
                if (m := ATTR.match(line)) and "presplitcoroutine" in m.group(2)}
    funcs, cur = [], None
    for line in text.splitlines():
        m = DEFINE.match(line)
        if m:
            cur = (m.group(1), bool(set(re.findall(r"#(\d+)", m.group(3))) & presplit), [])
            funcs.append(cur)
            continue
        if cur is not None:
            if line.startswith("}"):
                cur = None
            else:
                cur[2].append(line)
    for fname, is_coro, body in funcs:
        if not is_coro:
            continue
        blocks, order, succ = blocks_of(body)
        # positions: (block, index in block)
        begin = None
        free_pos = []
        for b in order:
            for i, line in enumerate(blocks[b]):
                if "@llvm.coro.begin(" in line:
                    begin = (b, i)
                m = re.match(r"\s*(%[\w.$-]+) = call ptr @llvm\.coro\.free\(", line)
                if m:
                    freed = m.group(1)
                    for b2 in order:
                        for j, l2 in enumerate(blocks[b2]):
                            if re.search(r"@_ZdlPv\w*\(ptr[^,)]* " + re.escape(freed) + r"(?![\w.$-])", l2):
                                free_pos.append((b2, j))
        pre = reach([order[0]], succ, stop={begin[0]}) if begin else set()
        # the suspending activation's path: the default edge of every switch
        # on a coro.suspend result, up to the return
        susp = set()
        for b in order:
            text_b = "\n".join(blocks[b])
            for m in re.finditer(r"(%[\w.$-]+) = call i8 @llvm\.coro\.suspend\(", text_b):
                for b2 in order:
                    for l2 in blocks[b2]:
                        mm = re.match(r"\s*switch i8 " + re.escape(m.group(1)) + r", label (%[\w.$-]+)", l2)
                        if mm:
                            susp |= reach([mm.group(1)], succ)
        post = set()
        for b, j in free_pos:
            post |= reach(succ[b], succ)

        def where(b, i):
            w = set()
            if b in pre or (begin and b == begin[0] and i < begin[1]):
                w.add("pre-begin")
            if b in post or any(b == fb and i > fj for fb, fj in free_pos):
                w.add("post-free")
            if b in susp:
                w.add("suspended")
            return w

        for b in order:
            for line in blocks[b]:
                m = ALLOCA.match(line)
                if not m:
                    continue
                name = m.group(1)
                outside = "!coro.outside.frame" in line
                kinds, pos = Counter(), set()
                names, seen_names = [name], set()
                while names:
                    nm = names.pop()
                    if nm in seen_names:
                        continue
                    seen_names.add(nm)
                    t = tok(nm)
                    for b2 in order:
                        for i, l2 in enumerate(blocks[b2]):
                            if l2 is line or not t.search(l2) or (
                                    nm != name and l2.lstrip().startswith(nm + " =")):
                                continue
                            k = use_kind(l2, nm)
                            if k is None:
                                continue
                            if k.startswith("derive:"):
                                names.append(k[7:])
                                k = "derive"
                            kinds[k] += 1
                            pos |= {p + ":" + k for p in where(b2, i)}
                escaped = any(k in kinds for k in ("call", "store-of", "other"))
                markers = "lifetime" in kinds
                cls = ("outside" if outside else "promise" if "promise" in kinds else
                       ("escaped" if escaped else "unescaped") + ("+markers" if markers else "-markers"))
                total[cls] += 1
                short = re.sub(r"\d+$", "", name)
                for p in sorted(pos):
                    total[cls + " " + p] += 1
                    if cls.startswith("escaped") or cls == "outside":
                        shapes[(cls, p, short)] += 1
                if verbose and cls.startswith("escaped-markers") and pos:
                    print(path.split("/")[-1], fname[:60], name, dict(kinds), sorted(pos))


verbose = "-v" in sys.argv
total, shapes = Counter(), Counter()
for p in [a for a in sys.argv[1:] if a != "-v"]:
    analyse(p, verbose, total, shapes)
for k in sorted(total):
    print(f"{total[k]:6d}  {k}")
print("-- escaped / outside slots touched before coro.begin or after the free, by name:")
for (cls, p, short), n in sorted(shapes.items()):
    print(f"{n:6d}  {cls:18s} {p:22s} {short}")
