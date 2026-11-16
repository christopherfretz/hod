#!/usr/bin/env python3
"""i386 join C: writes int-shapes-float.ll and int-shapes-double.ll here.

    python3 int_shapes.py

The shapes of `spec/codegen-x86.md` §11 "Integer intrinsics": a result
computed before an integer intrinsic (`%p = a * b`, 80 bits wide on the
x87) and used after it (`%y = p + c`). Where `llc` lowers the intrinsic
with a branch (CPU `generic`, no `cmov`), its fast allocator spills `%p`
at the block end and `%y` is computed from the rounded value; elsewhere
`%y` keeps the 80 bits. Same `@<shape>(ptr %in, ptr %out)` contract and
driver as `shapes-*.ll` (`shapes.c`): the integer operands are read at
byte 180 (and 184, or 188 for more than 32 bits), the intrinsic's result
is stored at byte 200. `tests/i386_x87.rs` lists the shapes in order.
"""

import os

# name, operand width, intrinsic, operands, result width
SHAPES = [
    ("ctlz32_between", 32, "ctlz", ["%i", "i1 false"], 32),
    ("ctlz32_poison", 32, "ctlz", ["%i", "i1 true"], 32),
    ("cttz32_between", 32, "cttz", ["%i", "i1 false"], 32),
    ("cttz16_between", 16, "cttz", ["%i", "i1 false"], 16),
    ("ctlz64_poison", 64, "ctlz", ["%i", "i1 true"], 64),
    ("umax32_between", 32, "umax", ["%i", "%j"], 32),
    ("umax32_zero", 32, "umax", ["%i", "0"], 32),
    ("smin8_between", 8, "smin", ["%i", "%j"], 8),
    ("smax32_self", 32, "smax", ["%i", "%i"], 32),
    ("umin64_between", 64, "umin", ["%i", "%j"], 64),
    ("uaddsat32_between", 32, "uadd.sat", ["%i", "%j"], 32),
    ("saddsat1_between", 1, "sadd.sat", ["%i", "%j"], 1),
    ("ssubsat32_self", 32, "ssub.sat", ["%i", "%i"], 32),
    ("usubsat64_between", 64, "usub.sat", ["%i", "%j"], 64),
    ("ushlsat32_between", 32, "ushl.sat", ["%i", "%j"], 32),
    ("sshlsat32_zero", 32, "sshl.sat", ["%i", "0"], 32),
    ("fshl32_var", 32, "fshl", ["%i", "%j", "%j"], 32),
    ("fshl64_var", 64, "fshl", ["%i", "%j", "%j"], 64),
    ("fshr64_const", 64, "fshr", ["%i", "%j", "5"], 64),
    ("bswap64_between", 64, "bswap", ["%i"], 64),
    ("abs64_between", 64, "abs", ["%i", "i1 false"], 64),
    ("ctpop32_between", 32, "ctpop", ["%i"], 32),
    ("scmp32_between", 32, "scmp", ["%i", "%j"], 8),
    ("ucmp64_between", 64, "ucmp", ["%i", "%j"], 8),
]


def module(ty):
    decls, funcs = set(), []
    for name, w, op, args, rw in SHAPES:
        t, rt = f"i{w}", f"i{rw}"
        full = f"llvm.{op}.{rt}.{t}" if op in ("scmp", "ucmp") else f"llvm.{op}.{t}"
        types = ", ".join("i1" if a.startswith("i1 ") else t for a in args)
        decls.add(f"declare {rt} @{full}({types})")
        call = ", ".join(a if a.startswith("i1 ") else f"{t} {a}" for a in args)
        if rw < 32:
            store = f"  %e = zext {rt} %s to i32\n  store i32 %e, ptr %os\n"
        else:
            store = f"  store {rt} %s, ptr %os\n"
        funcs.append(f"""define void @{name}(ptr %in, ptr %out) nounwind {{
entry:
  %ga = getelementptr {ty}, ptr %in, i32 0
  %a = load {ty}, ptr %ga
  %gb = getelementptr {ty}, ptr %in, i32 1
  %b = load {ty}, ptr %gb
  %gc = getelementptr {ty}, ptr %in, i32 2
  %c = load {ty}, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load {t}, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 {188 if w > 32 else 184}
  %j = load {t}, ptr %gj
  %p = fmul {ty} %a, %b
  %gn = getelementptr {ty}, ptr %in, i32 3
  %n = load {ty}, ptr %gn
  %x = fsub {ty} %p, %n
  %ox0 = getelementptr {ty}, ptr %out, i32 0
  store {ty} %x, ptr %ox0
  %s = call {rt} @{full}({call})
  %y = fadd {ty} %p, %c
  %oy1 = getelementptr {ty}, ptr %out, i32 1
  store {ty} %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
{store}  ret void
}}
""")
    names = ", ".join(f"ptr @{s[0]}" for s in SHAPES)
    return (
        f"; Written by int_shapes.py (i386 join C): integer intrinsics between an x87 result\n"
        f"; and its use, in {ty} (spec/codegen-x86.md §11). tests/i386_x87.rs lists the shapes.\n"
        'target triple = "i686-unknown-linux-gnu"\n'
        + "\n".join(sorted(decls))
        + "\n\n"
        + "\n".join(funcs)
        + f"\n@names = constant [{len(SHAPES)} x ptr] [{names}]\n"
        + f"@count = constant i32 {len(SHAPES)}\n"
    )


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    for ty in ("float", "double"):
        with open(os.path.join(here, f"int-shapes-{ty}.ll"), "w", encoding="utf-8") as f:
            f.write(module(ty))


if __name__ == "__main__":
    main()
