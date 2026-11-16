#!/usr/bin/env python3
"""WP-309: writes the generated run-test programs of `i386_bits.rs`.

    python3 gen.py              # rewrites unary.ll, binary.ll, funnel.ll,
                                # compare.ll here
    python3 gen.py --probe DIR  # writes probe-*.ll to DIR: the same functions
                                # without guards, fewer constants, for
                                # reading llc's assembly (branches.py)

Every test function `@t_*` takes pointers to its operands (`i32` in a
table, truncated to the width under test, so that the bits above the width
in the register are whatever the table held) and stores its result
zero-extended to `i64` (a `scmp`/`ucmp` result of 33 to 64 bits as it is).
`@run` calls one test function over every combination of its operand
tables and prints the name and the result's halves in hex. An operand that
would make the result poison (`ctlz`/`cttz` of 0 with `is_zero_poison`,
`abs` of the minimum with `is_int_min_poison`, a saturating shift by the
width or more) is not passed: the function stores a marker instead.
"""

import os
import sys

WIDTHS = [1, 7, 8, 16, 24, 31, 32]
M32 = (1 << 32) - 1
GARBAGE = 0xA5C3_5A3C


def mask(width):
    return (1 << width) - 1


def vals(width):
    """The edge set of `width` (0, 1, all ones, the sign bit, the sign bit
    less one, patterns), each also with the bits above the width set to a
    pattern; fixed 32-bit patterns."""
    m = mask(width)
    edges = [0, 1, m, 1 << (width - 1), (1 << (width - 1)) - 1, 2 & m,
             0x5555_5555 & m, 0xAAAA_AAAA & m, 0x1234_5678 & m, 0xF0F0_0F0F & m]
    out = []
    for v in edges:
        for w in [v, v | (GARBAGE & ~m & M32)]:
            if w not in out:
                out.append(w)
    for w in [0xDEAD_BEEF, 0x8000_0001, M32, 0x7FFF_FFFF]:
        if w not in out:
            out.append(w)
    return out


def small(width):
    """A subset for the three-operand functions."""
    m = mask(width)
    out = []
    for v in [0, 1, m, 1 << (width - 1), 0x1234_5678 & m, (GARBAGE | 1) & M32, 0xDEAD_BEEF]:
        if v not in out:
            out.append(v)
    return out


def funnel_amounts(width):
    out = []
    for v in [0, 1, 2, 7, 8, 15, 16, 23, 24, 31, 32, 33, width - 1, width, width + 1,
              2 * width - 1, 2 * width + 5, 0xFFFF_FF21, 0x8000_0001, M32]:
        v &= M32
        if v not in out:
            out.append(v)
    return out


def shift_amounts(width):
    out = []
    for v in [0, 1, 2, 7, 15, 23, 30, 31, width - 2, width - 1]:
        if 0 <= v < width and v not in out:
            out.append(v)
    # The same amounts with the bits above the width set.
    for v in list(out):
        w = v | (GARBAGE & ~mask(width) & M32)
        if w not in out:
            out.append(w)
    return out


FUNNEL_CONSTS = [0, 1, 5, 7, 8, 15, 16, 23, 24, 30, 31, 32, 33, 63]


def s32(v):
    v &= M32
    return v - (1 << 32) if v >> 31 else v


def table(name, values):
    body = ", ".join(f"i32 {s32(v)}" for v in values)
    return f"@{name} = internal global [{len(values)} x i32] [{body}]\n"


UNARY = [
    # (name, intrinsic, extra args, poison guard)
    ("bswap", "bswap", "", None),
    ("ctpop", "ctpop", "", None),
    ("ctlz", "ctlz", ", i1 false", None),
    ("ctlz_zp", "ctlz", ", i1 true", "zero"),
    ("cttz", "cttz", ", i1 false", None),
    ("cttz_zp", "cttz", ", i1 true", "zero"),
    ("bitreverse", "bitreverse", "", None),
    ("abs", "abs", ", i1 false", None),
    ("abs_mp", "abs", ", i1 true", "min"),
]
BINARY = ["smax", "smin", "umax", "umin", "sadd.sat", "uadd.sat", "ssub.sat", "usub.sat"]
SHIFTS = ["sshl.sat", "ushl.sat"]
# (operand widths, result widths) of the three-way compares.
COMPARES = [(1, [2, 8]), (7, [2, 8, 32]), (8, [8, 32]), (16, [16, 64]), (24, [8, 32]),
            (31, [32]), (32, [2, 8, 32, 33, 64])]

HEADER = """target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@fmt = private constant [13 x i8] c"%s %08x%08x\\0A\\00"
@done = private constant [5 x i8] c"done\\00"

declare i32 @printf(ptr, ...) nounwind
declare i32 @puts(ptr) nounwind

; f(&a[i], &b[j], &c[k], &out) for every i, j, k; one line per call.
define internal void @run(ptr %name, ptr %f, ptr %a, i32 %na, ptr %b, i32 %nb, ptr %c, i32 %nc) #0 {
entry:
  %out = alloca i64, align 8
  br label %li
li:
  %i = phi i32 [ 0, %entry ], [ %i1, %ni ]
  br label %lj
lj:
  %j = phi i32 [ 0, %li ], [ %j1, %nj ]
  br label %lk
lk:
  %k = phi i32 [ 0, %lj ], [ %k1, %lk ]
  %pa = getelementptr i32, ptr %a, i32 %i
  %pb = getelementptr i32, ptr %b, i32 %j
  %pc = getelementptr i32, ptr %c, i32 %k
  store i64 -2401053088876216593, ptr %out
  call void %f(ptr %pa, ptr %pb, ptr %pc, ptr %out)
  %lo = load i32, ptr %out
  %phi = getelementptr i8, ptr %out, i32 4
  %hi = load i32, ptr %phi
  %x = call i32 (ptr, ...) @printf(ptr @fmt, ptr %name, i32 %hi, i32 %lo)
  %k1 = add i32 %k, 1
  %ck = icmp slt i32 %k1, %nc
  br i1 %ck, label %lk, label %nj
nj:
  %j1 = add i32 %j, 1
  %cj = icmp slt i32 %j1, %nb
  br i1 %cj, label %lj, label %ni
ni:
  %i1 = add i32 %i, 1
  %ci = icmp slt i32 %i1, %na
  br i1 %ci, label %li, label %end
end:
  ret void
}
"""

FOOTER = """
attributes #0 = { noinline nounwind }
"""


class Program:
    def __init__(self):
        self.globals = []
        self.funcs = []
        self.calls = []
        self.decls = set()
        self.tables = {}

    def table(self, name, values):
        if name not in self.tables:
            self.tables[name] = len(values)
            self.globals.append(table(name, values))
        return name, len(values)

    def string(self, text):
        name = "s_" + text.replace(".", "_")
        self.globals.append(
            f"@{name} = private constant [{len(text) + 1} x i8] c\"{text}\\00\"\n")
        return name

    def test(self, label, body, tables):
        """A test function `@t_<label>` whose body reads %a, %b, %c (i32)."""
        fn = "t_" + label.replace(".", "_")
        self.funcs.append(
            f"define internal void @{fn}(ptr %pa, ptr %pb, ptr %pc, ptr %out) #0 {{\n"
            "entry:\n  %a = load i32, ptr %pa\n  %b = load i32, ptr %pb\n"
            f"  %c = load i32, ptr %pc\n{body}}}\n\n")
        name = self.string(label)
        args = [f"ptr @{tname}, i32 {n}" for tname, n in tables]
        while len(args) < 3:
            args.append(f"ptr @{tables[0][0]}, i32 1")
        self.calls.append(f"  call void @run(ptr @{name}, ptr @{fn}, {', '.join(args)})\n")

    def text(self, title):
        main = "define i32 @main() #0 {\nentry:\n" + "".join(self.calls) + \
            "  %d = call i32 @puts(ptr @done)\n  ret i32 0\n}\n"
        return (f"; {title}\n; Generated by gen.py (WP-309); do not edit by hand.\n"
                + HEADER + "\n" + "".join(self.globals) + "\n" + "".join(sorted(self.decls))
                + "\n" + "".join(self.funcs) + main + FOOTER)


def trunc(dst, src, width):
    if width == 32:
        return f"  %{dst} = add i32 %{src}, 0\n"
    return f"  %{dst} = trunc i32 %{src} to i{width}\n"


def store_result(var, width):
    if width == 64:
        return f"  store i64 %{var}, ptr %out\n  ret void\n"
    return f"  %rz = zext i{width} %{var} to i64\n  store i64 %rz, ptr %out\n  ret void\n"


def guard(kind, width):
    """Skips a poison operand %x: to `skip`, which returns (the marker stays)."""
    value = "0" if kind == "zero" else str(-(1 << (width - 1)))
    return (f"  %bad = icmp eq i{width} %x, {value}\n  br i1 %bad, label %skip, label %go\n"
            "skip:\n  ret void\ngo:\n")


def shift_guard(width):
    """Skips a saturating shift by the width or more."""
    # At one bit every amount but 0 is the width or more.
    return (f"  %bad = icmp uge i{width} %y, {width if width > 1 else 'true'}\n"
            "  br i1 %bad, label %skip, label %go\nskip:\n  ret void\ngo:\n")


def unary(probe=False):
    p = Program()
    for width in WIDTHS:
        t = p.table(f"v{width}", vals(width))
        for name, intr, extra, poison in UNARY:
            if intr == "bswap" and width % 16:
                continue
            p.decls.add(
                f"declare i{width} @llvm.{intr}.i{width}(i{width}{', i1' if extra else ''})\n")
            body = trunc("x", "a", width)
            if poison and not probe:
                body += guard(poison, width)
            body += f"  %r = call i{width} @llvm.{intr}.i{width}(i{width} %x{extra})\n"
            body += store_result("r", width)
            p.test(f"{name}.{width}", body, [t])
    return p


def binary(probe=False):
    p = Program()
    for width in WIDTHS:
        t = p.table(f"v{width}", vals(width))
        k = p.table(f"k{width}", shift_amounts(width))
        for name in BINARY + SHIFTS:
            p.decls.add(f"declare i{width} @llvm.{name}.i{width}(i{width}, i{width})\n")
            body = trunc("x", "a", width) + trunc("y", "b", width)
            if name in SHIFTS and not probe:
                # An amount of the width or more is poison.
                body += shift_guard(width)
            body += f"  %r = call i{width} @llvm.{name}.i{width}(i{width} %x, i{width} %y)\n"
            body += store_result("r", width)
            p.test(f"{name}.{width}", body, [t, k] if name in SHIFTS else [t, t])
            # The same operand twice.
            body = trunc("x", "a", width)
            if name in SHIFTS and not probe:
                body += shift_guard(width).replace("%y", "%x")
            body += f"  %r = call i{width} @llvm.{name}.i{width}(i{width} %x, i{width} %x)\n"
            body += store_result("r", width)
            p.test(f"{name}.{width}.same", body, [t])
    return p


def funnel(probe=False):
    p = Program()
    for width in WIDTHS:
        t = p.table(f"s{width}", small(width))
        k = p.table(f"f{width}", funnel_amounts(width))
        for name in ["fshl", "fshr"]:
            p.decls.add(
                f"declare i{width} @llvm.{name}.i{width}(i{width}, i{width}, i{width})\n")
            body = trunc("x", "a", width) + trunc("y", "b", width) + trunc("z", "c", width)
            body += (f"  %r = call i{width} @llvm.{name}.i{width}"
                     f"(i{width} %x, i{width} %y, i{width} %z)\n")
            body += store_result("r", width)
            p.test(f"{name}.{width}", body, [t, t, k])
            # A rotate (the same value twice) by a variable amount.
            body = trunc("x", "a", width) + trunc("z", "c", width)
            body += (f"  %r = call i{width} @llvm.{name}.i{width}"
                     f"(i{width} %x, i{width} %x, i{width} %z)\n")
            body += store_result("r", width)
            p.test(f"{name}.{width}.rot", body, [t, k])
            consts = [0, 1, 7, 33] if probe else FUNNEL_CONSTS
            for amount in consts:
                c = amount & mask(width)
                body = trunc("x", "a", width) + trunc("y", "b", width)
                body += (f"  %r = call i{width} @llvm.{name}.i{width}"
                         f"(i{width} %x, i{width} %y, i{width} {c})\n")
                body += store_result("r", width)
                p.test(f"{name}.{width}.k{amount}", body, [t, t])
    return p


def compare(probe=False):
    p = Program()
    for width, results in COMPARES:
        t = p.table(f"v{width}", vals(width))
        for name in ["scmp", "ucmp"]:
            for rw in results:
                p.decls.add(
                    f"declare i{rw} @llvm.{name}.i{rw}.i{width}(i{width}, i{width})\n")
                body = trunc("x", "a", width) + trunc("y", "b", width)
                body += (f"  %r = call i{rw} @llvm.{name}.i{rw}.i{width}"
                         f"(i{width} %x, i{width} %y)\n")
                body += store_result("r", rw)
                p.test(f"{name}.{rw}.{width}", body, [t, t])
    return p


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    programs = [
        ("unary.ll", unary, "WP-309: bswap, ctpop, ctlz, cttz, bitreverse, abs of i1..i32"),
        ("binary.ll", binary, "WP-309: min/max, saturating add/sub/shift of i1..i32"),
        ("funnel.ll", funnel, "WP-309: fshl/fshr of i1..i32, variable and constant amounts"),
        ("compare.ll", compare, "WP-309: scmp/ucmp of i1..i32 operands"),
    ]
    probe = sys.argv[1:2] == ["--probe"]
    where = sys.argv[2] if probe else here
    for file, make, title in programs:
        name = "probe-" + file if probe else file
        with open(os.path.join(where, name), "w", encoding="utf-8") as out:
            out.write(make(probe=probe).text(title))


if __name__ == "__main__":
    main()
