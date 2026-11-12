#!/usr/bin/env python3
"""Census of fixed-width vector use in LLVM text modules (docs/design/0003-vectors.md).

A lexical scan, line by line (LLVM's printer puts every vector-bearing
statement on one line): for each module that mentions a vector type, where
vectors occur -- instruction opcodes, signatures, calls, intrinsics,
constants, globals -- with module counts per corpus source. Usage:

    scripts/vector-census.py [--modules out.tsv] <corpus dir>

<corpus dir> holds MANIFEST.tsv and the modules it names. With --modules,
one line per vector module: entry, then its features separated by ','.

    scripts/vector-census.py --needs <corpus dir> < modules.tsv

reads entries (first tab field per line) and prints the packages of
docs/design/0003-vectors.md section 6 (V1..V5) each module needs.
"""
import collections
import re
import sys

VEC = re.compile(r"<(vscale x )?(\d+) x (ptr(?: addrspace\(\d+\))?|i\d+|half|bfloat|float|double|fp128|x86_fp80|ppc_fp128)>")
OPC = re.compile(r"^\s*(?:%[-\w.$]+|%\d+|%\"[^\"]*\")\s*=\s*(?:tail |musttail |notail )?([a-z_]+)")
BARE = re.compile(r"^\s*(store|ret|br|switch|resume|call|invoke|fence|unreachable)\b")
INTR = re.compile(r"@(llvm\.[\w.]+)")
VLIT = re.compile(r"<\s*(?:i\d+|half|bfloat|float|double|ptr)\s+(?:-?\d|0x|true|false|poison|undef|@|null|ptrtoint|getelementptr|bitcast|inttoptr|-?\d)")


def fold(name):
    """llvm.aarch64.neon.tbl1.v16i8 -> llvm.aarch64.neon.tbl1 (type suffixes folded)."""
    parts = name.split(".")
    while len(parts) > 2 and re.fullmatch(r"(v\d+|nxv\d+)?(i\d+|f\d+|bf16|p\d+|f16|f32|f64|f128)|p0|i\d+|v\d+i\d+", parts[-1]):
        parts.pop()
    return ".".join(parts)


def shape(m):
    return ("<vscale x " if m.group(1) else "<") + m.group(2) + " x " + m.group(3) + ">"


def bits(m):
    e = m.group(3)
    w = {"half": 16, "bfloat": 16, "float": 32, "double": 64, "fp128": 128}.get(e)
    if w is None:
        w = 64 if e.startswith("ptr") else int(e[1:])
    return int(m.group(2)) * w


def features(text):
    f = collections.Counter()
    for line in text.splitlines():
        if line.startswith(";") or line.startswith("!") or line.startswith("source_filename"):
            continue
        ms = list(VEC.finditer(line))
        if not ms:
            continue
        for m in ms:
            f["type " + shape(m)] += 1
            b = bits(m)
            cls = "<=64" if b <= 64 else "128" if b == 128 else "65..127" if b < 128 else ">128"
            el = m.group(3)
            kind = "i1" if el == "i1" else "ptr" if el.startswith("ptr") else "int" if el.startswith("i") else "fp"
            if m.group(1):
                f["size scalable"] += 1
            else:
                f["size %s %s" % (kind, cls)] += 1
        s = line.lstrip()
        if s.startswith("define"):
            head = s.split("(", 1)[0]
            f["sig define: vector return" if VEC.search(head) else "sig define: vector param"] += 1
            if re.search(r"\[\d+ x <", s) or re.search(r"\{[^}]*<\d+ x", s):
                f["sig define: aggregate of vectors"] += 1
            continue
        if s.startswith("declare"):
            i = INTR.search(s)
            f[("declare " + fold(i.group(1))) if i else "sig declare (non-intrinsic)"] += 1
            continue
        if re.match(r"^%[-\w.\"]+ = type", s):
            f["named struct with vector field"] += 1
            continue
        if re.match(r"^@[-\w.$\"]+ = ", s):
            f["global"] += 1
            if VLIT.search(s.split("=", 1)[1]):
                f["global: vector literal initializer"] += 1
            continue
        m = OPC.match(line) or BARE.match(line)
        if not m:
            f["other line"] += 1
            continue
        op = m.group(1)
        if op in ("call", "invoke"):
            i = INTR.search(s)
            if i:
                f["intrinsic " + fold(i.group(1))] += 1
            else:
                head = s.split("@", 1)[0] if "@" in s else s.split("(", 1)[0]
                f["call: vector return" if VEC.search(head) else "call: vector arg"] += 1
        else:
            f["inst " + op] += 1
            if op in ("icmp", "fcmp"):
                f["inst %s (vector, result <N x i1>)" % op] += 1
            if op == "select":
                cond = s.split("select", 1)[1].strip()
                f["inst select: vector condition" if cond.startswith("<") else "inst select: scalar condition"] += 1
            if op in ("extractelement", "insertelement"):
                idx = s.rsplit(",", 1)[1].strip()
                f["inst %s: %s index" % (op, "constant" if re.fullmatch(r"i\d+ -?\d+", idx) else "variable")] += 1
            if op == "bitcast":
                src, _, dst = s.split("bitcast", 1)[1].partition(" to ")
                f["inst bitcast %s->%s" % ("vec" if VEC.search(src) else "scalar", "vec" if VEC.search(dst) else "scalar")] += 1
            if op in ("load", "store"):
                a = re.search(r"align (\d+)", s)
                vm = VEC.search(s)
                nat = min(bits(vm) // 8, 16) if vm else 0
                if a and int(a.group(1)) < nat:
                    f["inst %s: under-aligned" % op] += 1
                if vm and vm.group(3) == "i1":
                    f["inst %s of <N x i1>" % op] += 1
            if op == "shufflevector":
                f["inst shufflevector"] += 0
            if op == "getelementptr" and re.search(r"<\d+ x ptr>", s):
                f["inst getelementptr: vector of pointers"] += 1
        if VLIT.search(s) or "splat (" in s:
            f["const: vector literal / splat"] += 1
        if re.search(r"<\d+ x [^>]+> (zeroinitializer|poison|undef)", s):
            f["const: zeroinitializer/poison/undef vector"] += 1
    return f


def vclass(m):
    """The value class 0003-vectors.md gives a vector type: mask, fpr, promoted or mem."""
    if m.group(3) == "i1":
        return "mask"
    b, n = bits(m), int(m.group(2))
    if b in (64, 128) and n & (n - 1) == 0:
        return "fpr"
    if n & (n - 1) == 0 and b < 64 and not m.group(3).startswith(("half", "bfloat", "float", "double")):
        return "promoted"
    return "mem"


NEON_INT = {"add", "sub", "and", "or", "xor", "shl", "lshr", "ashr"}
NEON_FP = {"fadd", "fsub", "fmul", "fdiv", "fneg"}
SCALAR_INTR = ("llvm.fma", "llvm.fmuladd", "llvm.fabs", "llvm.sqrt", "llvm.ceil", "llvm.floor", "llvm.trunc",
               "llvm.round", "llvm.roundeven", "llvm.rint", "llvm.nearbyint", "llvm.copysign", "llvm.minnum",
               "llvm.maxnum", "llvm.ctpop", "llvm.ctlz", "llvm.cttz", "llvm.bswap", "llvm.bitreverse",
               "llvm.smax", "llvm.smin", "llvm.umax", "llvm.umin", "llvm.abs", "llvm.sadd.sat", "llvm.uadd.sat",
               "llvm.ssub.sat", "llvm.usub.sat", "llvm.fshl", "llvm.fshr")


def needs(text):
    """Which proposed package (0003-vectors.md section 6) each vector-bearing line needs."""
    need = set()
    for line in text.splitlines():
        if line.startswith((";", "!", "source_filename")):
            continue
        ms = list(VEC.finditer(line))
        if not ms:
            continue
        classes = {vclass(m) for m in ms}
        s = line.lstrip()
        if s.startswith(("define", "declare")) or OPC.match(line) and OPC.match(line).group(1) in ("call", "invoke") \
                or BARE.match(line) and BARE.match(line).group(1) in ("call", "invoke", "ret"):
            i = INTR.search(s)
            if i and not s.startswith("define"):
                name = fold(i.group(1))
                if name.startswith(("llvm.aarch64.", "llvm.arm.")):
                    need.add("V5 neon-intrinsics")
                elif name.startswith("llvm.vector.reduce.") and "mask" in classes:
                    need.add("V2 mask")
                elif name.startswith("llvm.vector.reduce.") or name in SCALAR_INTR:
                    need.add("V3 lane-loop")
                elif name.startswith(("llvm.memcpy", "llvm.memset", "llvm.memmove", "llvm.lifetime", "llvm.dbg")):
                    pass
                else:
                    need.add("V3 lane-loop (other intrinsic %s)" % name)
                continue
            if "mask" in classes or "promoted" in classes:
                need.add("V4 abi")
            if re.search(r"\[\d+ x <", s):
                need.add("V1 base")
            need.add("V1 base")
            continue
        m = OPC.match(line) or BARE.match(line)
        if not m:
            need.add("V1 base")
            continue
        op = m.group(1)
        if op in ("extractelement", "insertelement", "shufflevector"):
            need.add("V1 element")
            if "mask" in classes:
                need.add("V2 mask")
        elif op in ("icmp", "fcmp"):
            need.add("V2 mask")
        elif op == "select":
            need.add("V2 mask" if s.split("select", 1)[1].strip().startswith("<") else "V1 base")
        elif op in ("sext", "zext", "trunc", "bitcast"):
            if "mask" in classes:
                need.add("V2 mask")
            elif op != "bitcast" and ("mem" in classes or "promoted" in classes):
                need.add("V3 lane-loop")
            else:
                need.add("V1 base")
        elif op in NEON_INT or op in NEON_FP or op == "mul":
            vm = ms[0]
            el = vm.group(3)
            if classes <= {"fpr"} and not (op == "mul" and el == "i64") and el not in ("half", "bfloat"):
                need.add("V1 base")
            else:
                need.add("V3 lane-loop")
        elif op in ("sdiv", "udiv", "srem", "urem", "frem") or op in (
                "fptosi", "fptoui", "sitofp", "uitofp", "fptrunc", "fpext", "ptrtoint", "inttoptr"):
            need.add("V3 lane-loop" if op not in ("fptrunc", "fpext") or "mem" in classes else "V1 base")
        else:
            need.add("V1 base")
    return need


def main(argv):
    out = None
    if len(argv) > 2 and argv[1] == "--needs":
        root = argv[2]
        for line in sys.stdin:
            if not line.strip():
                continue
            entry = line.split("\t")[0].strip()
            n = needs(open(root + "/" + entry, encoding="utf-8", errors="replace").read())
            print("%s\t%s" % (entry, ",".join(sorted(n))))
        return
    if len(argv) > 2 and argv[1] == "--modules":
        out, argv = argv[2], [argv[0]] + argv[3:]
    root = argv[1]
    entries = []
    with open(root + "/MANIFEST.tsv") as fh:
        next(fh)
        for row in fh:
            c = row.rstrip("\n").split("\t")
            entry, src, major, args = c[0], c[1], c[2], c[6]
            olevel = (re.findall(r"(?:^| )-O([0-3sz])", args) or ["0"])[-1]
            lang = "C" if entry.endswith(".c.ll") else "C++"
            group = "%s %s%s" % (src, lang, "" if olevel == "0" else " -O" + olevel)
            entries.append((entry, group, major))
    total = collections.Counter()
    rows = collections.defaultdict(collections.Counter)   # feature -> (group,major) -> modules
    uses = collections.defaultdict(collections.Counter)   # feature -> (group,major) -> occurrences
    lines = []
    for entry, group, major in entries:
        total[(group, major)] += 1
        try:
            text = open(root + "/" + entry, encoding="utf-8", errors="replace").read()
        except OSError:
            continue
        f = features(text)
        if not f:
            continue
        f["any vector"] = 1
        for k, n in f.items():
            rows[k][(group, major)] += 1
            uses[k][(group, major)] += n
        lines.append("%s\t%s" % (entry, ",".join(sorted(k for k in f if not k.startswith("type ")))))
    cols = sorted(total)
    print("| feature | " + " | ".join("%s (%s)" % c for c in cols) + " |")
    print("|---|" + "---:|" * len(cols))
    print("| modules | " + " | ".join(str(total[c]) for c in cols) + " |")
    for k in sorted(rows, key=lambda k: (-sum(rows[k].values()), k)):
        print("| `%s` | " % k + " | ".join(("%d (%d)" % (uses[k][c], rows[k][c])) if rows[k][c] else "" for c in cols) + " |")
    if out:
        with open(out, "w") as fh:
            fh.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main(sys.argv)
