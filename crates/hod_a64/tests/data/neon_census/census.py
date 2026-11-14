#!/usr/bin/env python3
"""The census of `<arm_neon.h>`, `<arm_fp16.h>` and `<arm_acle.h>` (WP-205).

For every function and function-like macro of the headers this writes a
one-call wrapper, compiles the wrappers with clang at `-O0` for AArch64
and records which `llvm.*` intrinsics and which plain vector instructions
reach the back end; with `--hod` it also asks `hod --strict` to build
each wrapper on its own and records the answer.

Mechanical: functions are read from their `__ai` (or `static __inline__`)
definitions and their `target("...")` attribute (the wrapper gets the same
attribute); macros from their `__sN = __pN` lines (a parameter without one
is a pointer for `vld*`/`vst*`, else an immediate). A wrapper clang
refuses for an immediate is retried with the next of `IMMS`; for a
feature (clang names it), with that feature added to the wrapper's
`target`; one that never compiles is counted as such.

Usage:
  python3 -B census.py <work dir> <clang> [--hod <hod>] > rows.tsv

Writes into <work dir> (temporary: delete it after) and prints one row
per wrapper: `header  function  feature  group  names  instructions
hod-answer`. `table.py` folds the rows of clang 19 and 22 into
`census.tsv`.
"""

import os
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

HEADERS = ("arm_neon.h", "arm_fp16.h", "arm_acle.h")
IMMS = ("1", "0", "2", "3", "4", "8", "15", "16", "90")
FLAGS = ("--target=aarch64-unknown-linux-gnu", "-march=armv8-a", "-O0", "-ffreestanding",
         "-w", "-ferror-limit=0", "-fno-color-diagnostics")

# The header groups, by the ACLE name (first match).
GROUPS = [
    ("crc", r"^__crc32"),
    ("aes", r"^vaes|^vmull_p64|^vmull_high_p64"),
    ("sha", r"^vsha|^veor3|^vrax1|^vxar|^vbcax"),
    ("sm", r"^vsm"),
    ("dot", r"^v(us|su)?dot|^vmmla|^vusmmla|^vsmmla|^vummla"),
    ("rdm", r"^vqrdml"),
    ("bf16", r"bf16|^vbf|^vcvt.*_bf16|^vcvtah"),
    ("fp16fml", r"^vfml"),
    ("complex", r"^vcmla|^vcadd"),
    ("fp8", r"mf8|fpm|^vamax|^vamin|^vscale|^vluti"),
    ("load-store", r"^vld|^vst"),
    ("table", r"^vtbl|^vtbx|^vqtbl|^vqtbx"),
    ("reduce", r"^v(add|max|min|maxnm|minnm)v|^vaddlv"),
    ("pairwise", r"^vp|^vpadal|^vpaddl"),
    ("saturating", r"^vq|^vuqadd|^vsqadd"),
    ("shift", r"^v(r?shr|shl|rshl|sli|sri|r?sra|shll|shrn|rshrn)"),
    ("compare", r"^vc(eq|ge|gt|le|lt|ag|al|tst)|^vtst"),
    ("min-max", r"^v(max|min)"),
    ("convert", r"^vcvt|^vrnd|^vreinterpret"),
    ("polynomial", r"_p8$|_p16$|_p64$|_p128$"),
    ("lane", r"^v(get|set|dup|copy|mov|ext|zip|uzp|trn|rev|combine|create|splat|bsl)"),
    ("system", r"^__(dmb|dsb|isb|wfi|wfe|sev|yield|nop|hint|rsr|wsr|arm_|sys|tst|chk|gcs|rnd|pl)"),
    ("scalar", r"^__"),
    ("arithmetic", r"^v"),
]

FN_NEON = re.compile(
    r"^__ai (?:__attribute__\(\(target\(\"([^\"]*)\"\)\)\) )?(.+?)\b(v\w+)\((.*)\) \{$")
FN_ACLE = re.compile(r"^static __inline__ (.+?) __attribute__\(\(([^)]*(?:\([^)]*\))?[^)]*)\)\)"
                     r"\s*(__\w+)\((.*)\) \{$")
MACRO = re.compile(r"^#define ((?:v|__)\w+)\(([\w, ]*)\)\s*(.*)$")


def group(name):
    for g, pat in GROUPS:
        if re.search(pat, name):
            return g
    return "other"


def pointee(name):
    """The element type of a `vld*`/`vst*` macro's pointer, from its
    name's suffix (`void` if it has none)."""
    m = re.search(r"_(s|u|p|f|bf|mf)(\d+)(?:_x\d)?$", name)
    if not m:
        return "void"
    kind, bits = m.groups()
    return {"s": f"int{bits}_t", "u": f"uint{bits}_t", "p": f"poly{bits}_t",
            "f": f"float{bits}_t", "bf": "bfloat16_t", "mf": "mfloat8_t"}[kind]


def params(text):
    text = text.strip()
    if text in ("", "void"):
        return []
    out = []
    for p in text.split(","):
        p = p.strip()
        m = re.match(r"^(.*?)(\w+)$", p)
        out.append(m.group(1).strip())
    return out


def scan(path, header):
    """[(name, feature-or-None, wrapper-maker)] of one header, the first
    definition of each name (the little-endian one)."""
    lines = open(path, encoding="utf-8").read().split("\n")
    seen, out = set(), []
    i = 0
    while i < len(lines):
        line = lines[i]
        joined = line if line.endswith("{") or not line.startswith("static") else \
            line + " " + lines[i + 1].strip()
        m = FN_NEON.match(line)
        if m and not m.group(3).startswith("__noswap"):
            feat, ret, name, ps = m.group(1), m.group(2).strip(), m.group(3), params(m.group(4))
            if name not in seen:
                seen.add(name)
                out.append((name, feat, ("fn", ret, ps)))
        m = FN_ACLE.match(joined)
        if m:
            ret, attrs, name, ps = m.group(1).strip(), m.group(2), m.group(3), params(m.group(4))
            t = re.search(r'target\("([^"]*)"\)', attrs)
            if name not in seen:
                seen.add(name)
                out.append((name, t.group(1) if t else None, ("fn", ret, ps)))
        m = MACRO.match(line)
        if m and not m.group(1).startswith("__noswap") and m.group(1) not in ("__ai",):
            name = m.group(1)
            ps = [p.strip() for p in m.group(2).split(",") if p.strip()]
            body = []
            j = i
            while lines[j].endswith("\\"):
                j += 1
                body.append(lines[j])
            text = "\n".join([m.group(3)] + body)
            if name not in seen and ps:
                seen.add(name)
                types, ret = {}, "void"
                for b in body:
                    d = re.match(r"^\s*(\w+) (__\w+) = (__p\w+);", b)
                    if d:
                        types[d.group(3)] = d.group(1)
                    r = re.match(r"^\s*(\w+) __ret\w*;", b)
                    if r:
                        ret = r.group(1)
                if not text.startswith("__extension__") and header == "arm_acle.h":
                    ret = None  # `__typeof__` of the expression
                kinds = []
                for k, p in enumerate(ps):
                    if p in types:
                        kinds.append(types[p])
                    elif k == 0 and re.match(r"^v(ld|st)", name):
                        kinds.append(pointee(name) + " *")
                    elif header == "arm_acle.h" and re.search(r"addr|ptr", p):
                        kinds.append("void *")
                    else:
                        kinds.append(None)  # an immediate
                out.append((name, None, ("macro", ret, kinds)))
            i = j
        i += 1
    return out


# The ACLE functions clang provides as builtins rather than in a header.
# A `=<n>` operand is that constant.
BUILTINS = [(n, ("macro", "void", k)) for n, k in (
    ("__dmb", ["=11"]), ("__dsb", ["=11"]), ("__isb", ["=15"]), ("__yield", []),
    ("__wfe", []), ("__wfi", []), ("__sev", []), ("__sevl", []))]


def wrapper(name, feat, shape, imm):
    kind, ret, ps = shape
    attr = f'__attribute__((target("{feat}"))) ' if feat else ""
    args, call = [], []
    for k, t in enumerate(ps):
        if t is None:
            call.append(imm)
        elif t.startswith("="):
            call.append(t[1:])
        else:
            args.append(f"{t} a{k}" if "*" not in t else f"{t.replace('*', '').strip()} *a{k}")
            call.append(f"a{k}")
    sig = ", ".join(args) or "void"
    expr = f"{name}({', '.join(call)})"
    if ret is None:
        return f"{attr}__typeof__({expr}) w_{name}({sig}) {{ return {expr}; }}"
    if ret == "void":
        return f"{attr}void w_{name}({sig}) {{ {expr}; }}"
    return f"{attr}{ret} w_{name}({sig}) {{ return {expr}; }}"


def compile_round(work, clang, todo, tag, syntax=False):
    """Compiles the wrappers `todo` ({name: text}); returns the names that
    failed with their errors, and the IR text (`None` on a failure)."""
    src = os.path.join(work, f"{tag}.c")
    names = list(todo)
    with open(src, "w") as f:
        f.write("#include <arm_neon.h>\n#include <arm_fp16.h>\n#include <arm_acle.h>\n")
        for n in names:
            f.write(todo[n] + "\n")
    out = os.path.join(work, f"{tag}.ll")
    mode = ["-fsyntax-only"] if syntax else ["-S", "-emit-llvm", "-o", out]
    r = subprocess.run([clang, *FLAGS, *mode, src], capture_output=True, text=True)
    bad = {}
    for m in re.finditer(rf"{re.escape(src)}:(\d+):\d+: error: (.*)", r.stderr):
        k = int(m.group(1)) - 4
        if 0 <= k < len(names):
            bad.setdefault(names[k], []).append(m.group(2))
    if r.returncode != 0 and not bad:
        sys.exit(f"clang failed without a wrapper to blame:\n{r.stderr[:2000]}")
    return bad, (open(out).read() if r.returncode == 0 and not syntax else None)


NEED = re.compile(r"needs target feature ([\w.,+]+)|requires target feature '([\w.,+]+)'")


def feature_of(work, clang, pch, name, feat, text_of):
    """The wrapper's feature: clang's code generation stops at its first
    error, so each wrapper is compiled alone (against the headers'
    precompiled form) and each feature clang names is added."""
    src = os.path.join(work, f"f_{name}.c")
    for _ in range(6):
        with open(src, "w") as f:
            f.write(text_of(feat) + "\n")
        r = subprocess.run([clang, *FLAGS, "-include-pch", pch, "-S", "-emit-llvm", "-o",
                            os.devnull, src], capture_output=True, text=True)
        if r.returncode == 0:
            os.unlink(src)
            return feat or ""
        m = NEED.search(r.stderr)
        if not m:
            break
        need = set((m.group(1) or m.group(2)).split(","))
        cur = set(filter(None, (feat or "").split(",")))
        if need <= cur:
            break
        feat = ",".join(sorted(cur | need))
    os.unlink(src)
    return None


def base_name(callee):
    parts = callee.split(".")
    while len(parts) > 2 and re.match(r"^(v\d+(i|f|bf)\d+|i\d+|f\d+|bf16|p0|nxv\w+|v\d+i1)$",
                                      parts[-1]):
        parts.pop()
    return ".".join(parts)


def split_module(ir):
    """{function: (body text, [declared names])} and the module's
    non-function text (header, declarations, attributes, metadata)."""
    funcs, head, decls, cur = {}, [], {}, None
    for line in ir.split("\n"):
        if cur is not None:
            cur[1].append(line)
            if line == "}":
                funcs[cur[0]] = "\n".join(cur[1])
                cur = None
            continue
        m = re.match(r"^define .*@(w_\w+)\(", line)
        if m:
            cur = (m.group(1), [line])
        elif line.startswith("declare "):
            d = re.search(r"@([\w.$]+)\(", line)
            decls[d.group(1)] = line
        elif not line.startswith("define "):
            head.append(line)
    return funcs, head, decls


VECTOR_OP = re.compile(r"=\s+(\w+)\b[^,]*?<\d+ x ")


def analyse(body):
    names = sorted({base_name(c) for c in re.findall(r"@(llvm\.[\w.]+)\(", body)})
    insts = set()
    for line in body.split("\n"):
        m = re.match(r"^\s+%\S+ = (\w+)\b(.*)$", line)
        if not m or m.group(1) in ("alloca", "load", "call", "bitcast"):
            continue
        if "<" in m.group(2) and " x " in m.group(2):
            insts.add(m.group(1))
    return names, sorted(insts)


def hod_answer(hod, work, fname, body, head, decls):
    callees = set(re.findall(r"@([\w.$]+)\(", body))
    text = "\n".join(head) + "\n" + "\n".join(
        decls[c] for c in sorted(callees) if c in decls) + "\n" + body + "\n"
    path = os.path.join(work, f"m_{fname}.ll")
    with open(path, "w") as f:
        f.write(text)
    r = subprocess.run([hod, "--strict", "-O0", "-filetype=obj", path, "-o", "/dev/null"],
                       capture_output=True, text=True)
    os.unlink(path)
    if r.returncode == 0:
        return "built"
    m = re.search(r"unsupported construct '([^']*)'", r.stderr)
    return m.group(1) if m else "error:" + r.stderr.strip().split("\n")[0][:80]


def main():
    work, clang = sys.argv[1], sys.argv[2]
    hod = sys.argv[4] if len(sys.argv) > 4 and sys.argv[3] == "--hod" else None
    os.makedirs(work, exist_ok=True)
    inc = subprocess.run([clang, "-print-resource-dir"], capture_output=True,
                         text=True).stdout.strip() + "/include"
    entries = []
    for h in HEADERS:
        for name, feat, shape in scan(os.path.join(inc, h), h):
            entries.append((h, name, feat, shape))
    entries += [("builtin", name, None, shape) for name, shape in BUILTINS]
    feats = {name: feat for _, name, feat, _ in entries}
    imms = dict.fromkeys(feats, 0)
    shapes = {name: shape for _, name, _, shape in entries}
    # 1. The immediates: Sema reports every error of a translation unit.
    live, done = set(feats), {}
    for rnd in range(len(IMMS) + 1):
        todo = {n: wrapper(n, feats[n], shapes[n], IMMS[imms[n]]) for n in sorted(live)}
        bad, _ = compile_round(work, clang, todo, f"s{rnd}", syntax=True)
        for n in bad:
            if None in shapes[n][2] and imms[n] + 1 < len(IMMS):
                imms[n] += 1
            else:
                live.discard(n)
                done[n] = None
        if not bad:
            break
    # 2. Each wrapper alone (some errors are only found by code generation,
    #    which stops at the first): the features of those without a
    #    `target`, and the wrappers clang 19 cannot compile.
    pch = os.path.join(work, "h.pch")
    with open(os.path.join(work, "h.h"), "w") as f:
        f.write("#include <arm_neon.h>\n#include <arm_fp16.h>\n#include <arm_acle.h>\n")
    subprocess.run([clang, *FLAGS, "-x", "c-header", os.path.join(work, "h.h"), "-o", pch],
                   check=True)

    def settle(n):
        return n, feature_of(work, clang, pch, n, feats[n],
                             lambda f: wrapper(n, f, shapes[n], IMMS[imms[n]]))

    with ThreadPoolExecutor(max_workers=4) as pool:
        for n, f in pool.map(settle, sorted(live)):
            if f is None:
                live.discard(n)
                done[n] = None
            else:
                feats[n] = f
    # 3. The IR of every wrapper that compiles.
    todo = {n: wrapper(n, feats[n], shapes[n], IMMS[imms[n]]) for n in sorted(live)}
    bad, ir = compile_round(work, clang, todo, "all")
    if ir is None:
        sys.exit(f"the wrappers do not compile together: {sorted(bad)[:5]}")
    for n in live:
        done[n] = feats[n] or ""
    ir_all = [ir]
    funcs, head, decls = {}, [], {}
    for ir in ir_all:
        f, h, d = split_module(ir)
        funcs.update(f)
        head, decls = h, {**decls, **d}
    head = [line for line in head if not line.startswith("source_filename")]

    def row(entry):
        h, name, _, _ = entry
        feat = done.get(name)
        if feat is None:
            return f"{h}\t{name}\t-\t{group(name)}\t(does not compile)\t\t"
        body = funcs.get(f"w_{name}", "")
        names, insts = analyse(body)
        ans = hod_answer(hod, work, name, body, head, decls) if hod else ""
        return (f"{h}\t{name}\t{feat or 'neon'}\t{group(name)}\t{' '.join(names)}\t"
                f"{' '.join(insts)}\t{ans}")

    with ThreadPoolExecutor(max_workers=4) as pool:
        for line in pool.map(row, entries):
            print(line)


if __name__ == "__main__":
    main()
