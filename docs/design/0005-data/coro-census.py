"""Shapes of pre-split coroutines in LLVM text modules (WP-100 census).

usage: python3 -I coro-census.py <module.ll>...
Prints one line per module and a total line.
"""
import re
import sys
from collections import Counter

ATTR = re.compile(r"^attributes #(\d+) = \{(.*)\}")
DEFINE = re.compile(r"^define [^@]*@(\"[^\"]+\"|[\w.$]+)\((.*)\)(.*)\{")
NAMES = ["coro.suspend", "coro.suspend.final", "await.suspend.void",
         "await.suspend.bool", "await.suspend.handle", "coro.end.unwind",
         "invoke-in-coro", "landingpad", "coro.align", "alloc-fail-check",
         "dyn-alloca", "outside.frame", "promise-align>16", "coro.noop",
         "coro.resume", "coro.destroy", "coro.done", "coro.promise",
         "coro.frame", "is_in_ramp", "musttail", "byval-or-inalloca",
         "vararg", "callbr-or-indirectbr", "setjmp", "dbg"]


def census(path):
    text = open(path, encoding="utf-8", errors="replace").read()
    presplit = {m.group(1) for line in text.splitlines()
                if (m := ATTR.match(line)) and "presplitcoroutine" in m.group(2)}
    c = Counter()
    funcs = []
    cur = None
    for line in text.splitlines():
        m = DEFINE.match(line)
        if m:
            groups = set(re.findall(r"#(\d+)", m.group(3)))
            cur = [m.group(1), bool(groups & presplit), m.group(2), []]
            funcs.append(cur)
            continue
        if cur is not None:
            if line.startswith("}"):
                cur = None
            else:
                cur[3].append(line)
    for name, is_coro, params, body in funcs:
        b = "\n".join(body)
        if not is_coro:
            for k in ["coro.resume", "coro.destroy", "coro.done", "coro.promise", "coro.noop"]:
                c[k] += len(re.findall(r"@llvm\." + re.escape(k) + r"\(", b))
            continue
        c["coroutines"] += 1
        n_susp = len(re.findall(r"@llvm\.coro\.suspend\(", b))
        c["coro.suspend"] += n_susp
        c["max-suspends"] = max(c["max-suspends"], n_susp)
        c["coro.suspend.final"] += len(re.findall(r"@llvm\.coro\.suspend\(token %?[\w.]+, i1 true\)", b))
        for k in ["void", "bool", "handle"]:
            c["await.suspend." + k] += len(re.findall(r"@llvm\.coro\.await\.suspend\." + k + r"\(", b))
        c["coro.end.unwind"] += len(re.findall(r"@llvm\.coro\.end\(ptr [^,]+, i1 true", b))
        c["invoke-in-coro"] += len(re.findall(r"\binvoke\b", b))
        c["landingpad"] += len(re.findall(r"\blandingpad\b", b))
        c["coro.align"] += len(re.findall(r"@llvm\.coro\.align\.", b))
        c["coro.frame"] += len(re.findall(r"@llvm\.coro\.frame\(", b))
        c["is_in_ramp"] += len(re.findall(r"@llvm\.coro\.is_in_ramp\(", b))
        c["musttail"] += len(re.findall(r"\bmusttail\b", b))
        c["dyn-alloca"] += len(re.findall(r"= alloca [^,]+, i\d+ %", b))
        c["outside.frame"] += len(re.findall(r"!coro\.outside\.frame", b))
        c["dbg"] += 1 if "#dbg_declare" in b else 0
        c["vararg"] += 1 if "..." in params else 0
        c["byval-or-inalloca"] += 1 if ("byval" in params or "inalloca" in params) else 0
        c["callbr-or-indirectbr"] += len(re.findall(r"\b(callbr|indirectbr)\b", b))
        c["setjmp"] += len(re.findall(r"@_?setjmp\(", b))
        m = re.search(r"@llvm\.coro\.id\(i32 (\d+), ptr (%[\w.]+)", b)
        if m:
            slot = m.group(2)
            am = re.search(re.escape(slot) + r" = alloca [^\n]*align (\d+)", b)
            if am and int(am.group(1)) > 16:
                c["promise-align>16"] += 1
        # the allocation-failure check: the coro.alloc'd pointer compared with null
        if re.search(r"get_return_object_on_allocation_failure", b):
            c["alloc-fail-check"] += 1
    return c


total = Counter()
modules = 0
for p in sys.argv[1:]:
    c = census(p)
    if not +c:
        continue
    modules += 1
    total.update({k: v for k, v in c.items() if k != "max-suspends"})
    total["max-suspends"] = max(total["max-suspends"], c["max-suspends"])
    print(p, " ".join(f"{k}={c[k]}" for k in ["coroutines"] + NAMES if c[k]))
print("TOTAL modules", modules, " ".join(f"{k}={total[k]}" for k in ["coroutines", "max-suspends"] + NAMES))
