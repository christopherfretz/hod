//! WP-297, design 0008's I6: the i386 ABI grid (`spec/abi-x86.md` §11).
//!
//! Generated signatures over the type and attribute classes of
//! `spec/abi-x86.md` §3 to §6. For each signature `k` the hod module
//! defines `hod_k`, which forwards its arguments to `chk_k` and returns
//! what `chk_k` returns; the `llc` module defines `main`, which calls
//! every `hod_k` with known constants and checks the results, and
//! `chk_k`, which checks every argument and returns a known result. So
//! each signature crosses the boundary in both directions: `llc` caller
//! with hod callee (`main` to `hod_k`, and the result back) and hod caller
//! with `llc` callee (`hod_k` to `chk_k`, and the result back).
//!
//! `chk_k` and `mark` check that the stack pointer is 16-aligned at the
//! call (`llvm.addressofreturnaddress`): a callee pop that one side does
//! not account for shows there. An `i8`/`i16` argument with `signext` or
//! `zeroext` is received by `chk_k` as `i32`: the check sees the
//! extension hod's caller made.
//!
//! The `llc` module is built by `llc` 19 and 22; the hod module by the
//! facade and by `hod_x86` under `hod_cg::Stress::ALL`. Every pair runs
//! under `qemu-i386` and prints `ok`.
//!
//! The grid driver is this file rather than `crates/hod_x64/tests/abi`:
//! that one compares `hod_x64::abi::assign` against assembly it emits
//! with the x86-64 encoder and runs natively; a third target there would
//! be a second emitter, not a small change.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.
#![allow(
    clippy::many_single_char_names,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::trivially_copy_pass_by_ref,
    clippy::single_match_else,
    clippy::too_many_lines,
    reason = "a generator of IR text over small seeds"
)]

use std::fmt::Write as _;
use std::process::Command;

use hod::{Options, OutputKind};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch};

mod i386_support;
use i386_support::{TRIPLE, checked, llc_object, llvm_tool, run, stressed_object, tool};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    I8,
    I16,
    I32,
    Ptr,
    I64,
    F32,
    F64,
    F80,
    /// `{ i8, i32 }` as an IR value.
    Agg,
    /// `[2 x i16]` as an IR value.
    Arr,
    /// `ptr byval(%S) align 4`, `%S = { i32, i8, i16 }`.
    ByVal,
    /// `ptr byval(%B) align 16`, `%B = [5 x i32]`.
    ByVal16,
}

const TYS: [Ty; 12] = [
    Ty::I8,
    Ty::I16,
    Ty::I32,
    Ty::Ptr,
    Ty::I64,
    Ty::F32,
    Ty::F64,
    Ty::F80,
    Ty::Agg,
    Ty::Arr,
    Ty::ByVal,
    Ty::ByVal16,
];

impl Ty {
    fn ir(self) -> &'static str {
        match self {
            Ty::I8 => "i8",
            Ty::I16 => "i16",
            Ty::I32 => "i32",
            Ty::Ptr | Ty::ByVal | Ty::ByVal16 => "ptr",
            Ty::I64 => "i64",
            Ty::F32 => "float",
            Ty::F64 => "double",
            Ty::F80 => "x86_fp80",
            Ty::Agg => "{ i8, i32 }",
            Ty::Arr => "[2 x i16]",
        }
    }

    fn fp(self) -> bool {
        matches!(self, Ty::F32 | Ty::F64 | Ty::F80)
    }

    fn int(self) -> bool {
        matches!(self, Ty::I8 | Ty::I16 | Ty::I32 | Ty::Ptr | Ty::I64)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ext {
    None,
    Sign,
    Zero,
}

#[derive(Clone, Copy, Debug)]
struct Param {
    ty: Ty,
    ext: Ext,
    inreg: bool,
    nest: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ret {
    Void,
    Scalar(Ty),
    /// `{ i32, i32 }`.
    Pair,
    /// `{ i8, i16, ptr }`.
    Mixed,
    /// `{ i64, i32 }`.
    Wide,
    /// `{ float, i32 }`.
    FpInt,
    /// `ptr sret(%S) align 4` as the first parameter.
    Sret {
        inreg: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Conv {
    C,
    Fast,
    Cold,
}

impl Conv {
    fn ir(self) -> &'static str {
        match self {
            Conv::C => "",
            Conv::Fast => "fastcc ",
            Conv::Cold => "coldcc ",
        }
    }
}

#[derive(Clone, Debug)]
struct Signature {
    conv: Conv,
    ret: Ret,
    params: Vec<Param>,
}

impl Ret {
    fn ir(self) -> &'static str {
        match self {
            Ret::Void | Ret::Sret { .. } => "void",
            Ret::Scalar(ty) => ty.ir(),
            Ret::Pair => "{ i32, i32 }",
            Ret::Mixed => "{ i8, i16, ptr }",
            Ret::Wide => "{ i64, i32 }",
            Ret::FpInt => "{ float, i32 }",
        }
    }

    /// The leaves of an aggregate result.
    fn leaves(self) -> &'static [Ty] {
        match self {
            Ret::Pair => &[Ty::I32, Ty::I32],
            Ret::Mixed => &[Ty::I8, Ty::I16, Ty::Ptr],
            Ret::Wide => &[Ty::I64, Ty::I32],
            Ret::FpInt => &[Ty::F32, Ty::I32],
            _ => &[],
        }
    }
}

/// A deterministic generator (the grid must not change between runs).
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % n as u64) as usize
    }
}

/// The 80-bit image of `value` (a positive normal double) as LLVM writes
/// an `x86_fp80` constant.
fn fp80(value: f64) -> String {
    let bits = value.to_bits();
    let exp = ((bits >> 52) & 0x7ff) - 1023 + 16383;
    let mant = (1u64 << 63) | ((bits & ((1 << 52) - 1)) << 11);
    format!("0xK{exp:04X}{mant:016X}")
}

fn seed(k: usize, j: usize) -> i64 {
    (k * 31 + j * 7 + 3) as i64
}

/// The constant of type `ty` for argument `j` of signature `k`.
fn value(ty: Ty, k: usize, j: usize) -> String {
    let v = seed(k, j);
    match ty {
        Ty::I8 => format!("{}", -(v % 100) - 1),
        Ty::I16 => format!("{}", -((v * 37) % 30000) - 1),
        Ty::I32 => format!("{}", (v * 1_000_003) as i32),
        Ty::Ptr => format!("getelementptr (i8, ptr @blob, i32 {})", v % 4000),
        Ty::I64 => format!("{}", v * 0x1_0000_0001 + 0x1234_5678_0000),
        Ty::F32 => format!("{v}.5"),
        Ty::F64 => format!("{v}.25"),
        Ty::F80 => fp80(v as f64 + 0.125),
        Ty::Agg => format!("{{ i8 {}, i32 {} }}", value(Ty::I8, k, j), value(Ty::I32, k, j + 1)),
        Ty::Arr => format!("[i16 {}, i16 {}]", value(Ty::I16, k, j), value(Ty::I16, k, j + 1)),
        Ty::ByVal => format!("@s{k}_{j}"),
        Ty::ByVal16 => format!("@b{k}_{j}"),
    }
}

/// The checker of a scalar type.
fn checker(ty: Ty) -> &'static str {
    match ty {
        Ty::I8 => "ck8",
        Ty::I16 => "ck16",
        Ty::I32 => "ck32",
        Ty::Ptr => "ckp",
        Ty::I64 => "ck64",
        Ty::F32 => "ckf",
        Ty::F64 => "ckd",
        Ty::F80 => "ckt",
        _ => unreachable!("not a scalar"),
    }
}

/// `call void @ckN(T got, T want, i32 k, i32 j)`.
fn check(out: &mut String, ty: Ty, got: &str, want: &str, k: usize, j: usize) {
    let t = ty.ir();
    writeln!(out, "  call void @{}({t} {got}, {t} {want}, i32 {k}, i32 {j})", checker(ty)).unwrap();
}

/// The parameter text of `p` named `name`: as hod and `main` see it, or
/// as `chk_k` does (`chk`: an extended `i8`/`i16` is an `i32`).
fn param(p: &Param, chk: bool, name: &str) -> String {
    let wide = chk && p.ext != Ext::None;
    let mut s = String::from(if wide { "i32" } else { p.ty.ir() });
    match p.ty {
        Ty::ByVal => s.push_str(" byval(%S) align 4"),
        Ty::ByVal16 => s.push_str(" byval(%B) align 16"),
        _ => {}
    }
    match (p.ext, wide) {
        (Ext::Sign, false) => s.push_str(" signext"),
        (Ext::Zero, false) => s.push_str(" zeroext"),
        _ => {}
    }
    if p.inreg {
        s.push_str(" inreg");
    }
    if p.nest {
        s.push_str(" nest");
    }
    if !name.is_empty() {
        s.push(' ');
        s.push_str(name);
    }
    s
}

fn sret_param(ret: Ret, name: &str) -> Option<String> {
    match ret {
        Ret::Sret { inreg } => {
            Some(format!("ptr sret(%S) align 4{} {name}", if inreg { " inreg" } else { "" }))
        }
        _ => None,
    }
}

/// The parameter list of signature `sig`, each named by `name(j)`.
fn params(sig: &Signature, chk: bool, name: impl Fn(usize) -> String) -> String {
    let mut list: Vec<String> = sret_param(sig.ret, &name(99)).into_iter().collect();
    list.extend(sig.params.iter().enumerate().map(|(j, p)| param(p, chk, &name(j))));
    list.join(", ")
}

/// The checks of argument `j` (`p`) in `chk_k`.
fn check_arg(out: &mut String, p: &Param, k: usize, j: usize) {
    let a = format!("%a{j}");
    match p.ty {
        Ty::I8 | Ty::I16 if p.ext != Ext::None => {
            let v: i64 = value(p.ty, k, j).parse().unwrap();
            let bits = if p.ty == Ty::I8 { 8 } else { 16 };
            let want = if p.ext == Ext::Sign { v } else { v + (1 << bits) };
            check(out, Ty::I32, &a, &want.to_string(), k, j);
        }
        ty if ty.int() || ty.fp() => check(out, ty, &a, &value(ty, k, j), k, j),
        Ty::Agg => {
            writeln!(out, "  %x{j}a = extractvalue {{ i8, i32 }} {a}, 0").unwrap();
            writeln!(out, "  %x{j}b = extractvalue {{ i8, i32 }} {a}, 1").unwrap();
            check(out, Ty::I8, &format!("%x{j}a"), &value(Ty::I8, k, j), k, j);
            check(out, Ty::I32, &format!("%x{j}b"), &value(Ty::I32, k, j + 1), k, j);
        }
        Ty::Arr => {
            for (i, n) in ["a", "b"].iter().enumerate() {
                writeln!(out, "  %x{j}{n} = extractvalue [2 x i16] {a}, {i}").unwrap();
                check(out, Ty::I16, &format!("%x{j}{n}"), &value(Ty::I16, k, j + i), k, j);
            }
        }
        Ty::ByVal => check_s(out, &a, k, j),
        Ty::ByVal16 => {
            for (i, at) in [0, 16].iter().enumerate() {
                writeln!(out, "  %x{j}p{i} = getelementptr inbounds i8, ptr {a}, i32 {at}")
                    .unwrap();
                writeln!(out, "  %x{j}v{i} = load i32, ptr %x{j}p{i}").unwrap();
                check(out, Ty::I32, &format!("%x{j}v{i}"), &value(Ty::I32, k, j + i), k, j);
            }
        }
        _ => unreachable!(),
    }
}

/// The fields of an `%S` at `p` (`{ i32, i8, i16 }`), checked.
fn check_s(out: &mut String, p: &str, k: usize, j: usize) {
    let tag = &p[1..];
    for (i, (ty, at)) in [(Ty::I32, 0), (Ty::I8, 4), (Ty::I16, 6)].iter().enumerate() {
        writeln!(out, "  %{tag}p{i} = getelementptr inbounds i8, ptr {p}, i32 {at}").unwrap();
        writeln!(out, "  %{tag}v{i} = load {}, ptr %{tag}p{i}", ty.ir()).unwrap();
        check(out, *ty, &format!("%{tag}v{i}"), &value(*ty, k, j + i), k, j);
    }
}

/// The constant `chk_k` returns.
fn ret_value(ret: Ret, k: usize) -> String {
    match ret {
        Ret::Scalar(ty) => value(ty, k, 40),
        _ => {
            let leaves: Vec<String> = ret
                .leaves()
                .iter()
                .enumerate()
                .map(|(i, &ty)| format!("{} {}", ty.ir(), value(ty, k, 40 + i)))
                .collect();
            format!("{{ {} }}", leaves.join(", "))
        }
    }
}

const PRELUDE: &str = "target datalayout = \"e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128\"
target triple = \"i686-unknown-linux-gnu\"

%S = type { i32, i8, i16 }
%B = type [5 x i32]
";

/// The `llc` module's helpers: the checkers, `fail`, `align`, `mark`.
fn helpers(out: &mut String) {
    out.push_str(
        "@blob = global [4096 x i8] zeroinitializer
@fmt = private constant [12 x i8] c\"fail %d %d\\0A\\00\"
@okfmt = private constant [4 x i8] c\"ok\\0A\\00\"
declare i32 @printf(ptr, ...)
declare void @exit(i32) noreturn
declare ptr @llvm.addressofreturnaddress.p0()

define internal void @fail(i32 %k, i32 %j) noreturn {
  %r = call i32 (ptr, ...) @printf(ptr @fmt, i32 %k, i32 %j)
  call void @exit(i32 1)
  unreachable
}

; The caller's stack pointer was 16-aligned at the call.
define internal void @align(i32 %k) {
  %a = call ptr @llvm.addressofreturnaddress.p0()
  %i = ptrtoint ptr %a to i32
  %m = and i32 %i, 15
  %c = icmp ne i32 %m, 12
  br i1 %c, label %f, label %o
f:
  call void @fail(i32 %k, i32 98)
  unreachable
o:
  ret void
}

define void @mark(i32 %k) {
  call void @align(i32 %k)
  ret void
}
",
    );
    for (name, ty, cmp) in [
        ("ck8", "i8", "icmp ne"),
        ("ck16", "i16", "icmp ne"),
        ("ck32", "i32", "icmp ne"),
        ("ck64", "i64", "icmp ne"),
        ("ckp", "ptr", "icmp ne"),
        ("ckf", "float", "fcmp une"),
        ("ckd", "double", "fcmp une"),
        ("ckt", "x86_fp80", "fcmp une"),
    ] {
        writeln!(
            out,
            "define internal void @{name}({ty} %g, {ty} %w, i32 %k, i32 %j) {{
  %c = {cmp} {ty} %g, %w
  br i1 %c, label %f, label %o
f:
  call void @fail(i32 %k, i32 %j)
  unreachable
o:
  ret void
}}"
        )
        .unwrap();
    }
}

/// The grid's signatures: generated ones, then fixed rows.
fn signatures() -> Vec<Signature> {
    let mut rng = Lcg(0x1386_0297);
    let mut sigs = Vec::new();
    for _ in 0..96 {
        let conv = [Conv::C, Conv::C, Conv::C, Conv::Fast, Conv::Cold][rng.below(5)];
        let count = rng.below(7);
        let inreg_sig = conv == Conv::C && rng.below(3) == 0;
        let mut params = Vec::new();
        while params.len() < count {
            let ty = TYS[rng.below(TYS.len())];
            if conv == Conv::Fast && ty.fp() {
                continue;
            }
            let ext = match ty {
                Ty::I8 | Ty::I16 => [Ext::None, Ext::Sign, Ext::Zero][rng.below(3)],
                _ => Ext::None,
            };
            let inreg = inreg_sig && ty.int() && rng.below(3) != 0;
            params.push(Param { ty, ext, inreg, nest: false });
        }
        let ret = loop {
            let ret = match rng.below(14) {
                0 | 1 => Ret::Void,
                2..=9 => Ret::Scalar(
                    [Ty::I8, Ty::I16, Ty::I32, Ty::Ptr, Ty::I64, Ty::F32, Ty::F64, Ty::F80]
                        [rng.below(8)],
                ),
                10 => Ret::Pair,
                11 => Ret::Mixed,
                12 => Ret::Wide,
                _ => match rng.below(3) {
                    0 => Ret::FpInt,
                    n => Ret::Sret { inreg: n == 2 && conv == Conv::C && !inreg_sig },
                },
            };
            let fp = matches!(ret, Ret::Scalar(ty) if ty.fp()) || ret == Ret::FpInt;
            if conv == Conv::Fast && (fp || matches!(ret, Ret::Sret { .. })) {
                continue;
            }
            break ret;
        };
        sigs.push(Signature { conv, ret, params });
    }
    let p = |ty| Param { ty, ext: Ext::None, inreg: false, nest: false };
    let nest = Param { nest: true, ..p(Ty::Ptr) };
    // §3.2: a `byval` at alignment 16 after a 4-byte slot.
    sigs.push(Signature {
        conv: Conv::C,
        ret: Ret::Scalar(Ty::I32),
        params: vec![p(Ty::I32), p(Ty::ByVal16), p(Ty::I32)],
    });
    // §6.3: `nest` in `ecx`, and in `eax` under `fastcc`.
    for conv in [Conv::C, Conv::Fast] {
        sigs.push(Signature {
            conv,
            ret: Ret::Scalar(Ty::I32),
            params: vec![nest, p(Ty::I32), p(Ty::I32), p(Ty::I64)],
        });
    }
    // §3.1: an `i64 inreg` split between `ecx` and the stack.
    let inreg = |ty| Param { inreg: true, ..p(ty) };
    sigs.push(Signature {
        conv: Conv::C,
        ret: Ret::Scalar(Ty::I64),
        params: vec![inreg(Ty::I32), inreg(Ty::I32), inreg(Ty::I64), inreg(Ty::I32)],
    });
    // `fastcc`: an `i64` split between `edx` and the stack.
    sigs.push(Signature {
        conv: Conv::Fast,
        ret: Ret::Scalar(Ty::I64),
        params: vec![p(Ty::I32), p(Ty::I64), p(Ty::I8)],
    });
    // §5.2: `sret`, plain and `inreg`, with arguments in registers.
    sigs.push(Signature {
        conv: Conv::C,
        ret: Ret::Sret { inreg: false },
        params: vec![inreg(Ty::I32), p(Ty::F80)],
    });
    sigs.push(Signature {
        conv: Conv::C,
        ret: Ret::Sret { inreg: true },
        params: vec![inreg(Ty::I32), p(Ty::I64)],
    });
    sigs
}

/// The two modules of the grid: hod's and `llc`'s.
fn modules(sigs: &[Signature]) -> (String, String) {
    let mut hod = String::from(PRELUDE);
    hod.push_str("declare void @mark(i32) nounwind\n");
    let mut llc = String::from(PRELUDE);
    helpers(&mut llc);
    let mut main = String::from("define i32 @main() {\n");
    for (k, sig) in sigs.iter().enumerate() {
        let cc = sig.conv.ir();
        let ret = sig.ret.ir();
        let proto = params(sig, false, |_| String::new());
        // hod: the forwarder.
        writeln!(hod, "declare {cc}{ret} @chk{k}({proto}) nounwind").unwrap();
        let named = params(sig, false, |j| format!("%a{j}"));
        writeln!(hod, "define {cc}{ret} @hod{k}({named}) nounwind {{").unwrap();
        let result = if ret == "void" { String::new() } else { "%r = ".into() };
        writeln!(hod, "  {result}call {cc}{ret} @chk{k}({named})").unwrap();
        writeln!(hod, "  call void @mark(i32 {k})").unwrap();
        if ret == "void" {
            hod.push_str("  ret void\n}\n");
        } else {
            writeln!(hod, "  ret {ret} %r\n}}").unwrap();
        }
        // llc: the checker.
        writeln!(llc, "define {cc}{ret} @chk{k}({}) {{", params(sig, true, |j| format!("%a{j}")))
            .unwrap();
        writeln!(llc, "  call void @align(i32 {k})").unwrap();
        for (j, p) in sig.params.iter().enumerate() {
            check_arg(&mut llc, p, k, j);
        }
        if let Ret::Sret { .. } = sig.ret {
            for (i, (ty, at)) in [(Ty::I32, 0), (Ty::I8, 4), (Ty::I16, 6)].iter().enumerate() {
                writeln!(llc, "  %sp{i} = getelementptr inbounds i8, ptr %a99, i32 {at}").unwrap();
                writeln!(llc, "  store {} {}, ptr %sp{i}", ty.ir(), value(*ty, k, 50 + i)).unwrap();
            }
        }
        if ret == "void" {
            llc.push_str("  ret void\n}\n");
        } else {
            writeln!(llc, "  ret {ret} {}\n}}", ret_value(sig.ret, k)).unwrap();
        }
        // llc: the call in `main` and its result's checks.
        for (j, p) in sig.params.iter().enumerate() {
            match p.ty {
                Ty::ByVal => writeln!(
                    llc,
                    "@s{k}_{j} = global %S {{ i32 {}, i8 {}, i16 {} }}",
                    value(Ty::I32, k, j),
                    value(Ty::I8, k, j + 1),
                    value(Ty::I16, k, j + 2)
                )
                .unwrap(),
                Ty::ByVal16 => writeln!(
                    llc,
                    "@b{k}_{j} = global %B [i32 {}, i32 0, i32 0, i32 0, i32 {}], align 16",
                    value(Ty::I32, k, j),
                    value(Ty::I32, k, j + 1)
                )
                .unwrap(),
                _ => {}
            }
        }
        writeln!(llc, "declare {cc}{ret} @hod{k}({proto})").unwrap();
        let mut args: Vec<String> = Vec::new();
        if let Some(s) = sret_param(sig.ret, &format!("%out{k}")) {
            writeln!(main, "  %out{k} = alloca %S").unwrap();
            args.push(s);
        }
        for (j, p) in sig.params.iter().enumerate() {
            args.push(param(p, false, &value(p.ty, k, j)));
        }
        let got = if ret == "void" { String::new() } else { format!("%r{k} = ") };
        writeln!(main, "  {got}call {cc}{ret} @hod{k}({})", args.join(", ")).unwrap();
        match sig.ret {
            Ret::Void => {}
            Ret::Sret { .. } => {
                for (i, (ty, at)) in [(Ty::I32, 0), (Ty::I8, 4), (Ty::I16, 6)].iter().enumerate() {
                    writeln!(
                        main,
                        "  %o{k}p{i} = getelementptr inbounds i8, ptr %out{k}, i32 {at}"
                    )
                    .unwrap();
                    writeln!(main, "  %o{k}v{i} = load {}, ptr %o{k}p{i}", ty.ir()).unwrap();
                    check(&mut main, *ty, &format!("%o{k}v{i}"), &value(*ty, k, 50 + i), k, 50 + i);
                }
            }
            Ret::Scalar(ty) => check(&mut main, ty, &format!("%r{k}"), &value(ty, k, 40), k, 40),
            _ => {
                for (i, &ty) in sig.ret.leaves().iter().enumerate() {
                    writeln!(main, "  %r{k}x{i} = extractvalue {ret} %r{k}, {i}").unwrap();
                    check(&mut main, ty, &format!("%r{k}x{i}"), &value(ty, k, 40 + i), k, 40 + i);
                }
            }
        }
    }
    main.push_str("  %p = call i32 (ptr, ...) @printf(ptr @okfmt)\n  ret i32 0\n}\n");
    llc.push_str(&main);
    (hod, llc)
}

/// Every signature of the grid, both directions, against `llc` 19 and
/// 22, hod plain and under `Stress::ALL`.
#[test]
fn the_abi_grid_runs_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let sigs = signatures();
    let (hod_ll, llc_ll) = modules(&sigs);
    let scratch = Scratch::new("i386-abi");
    let llc_path = scratch.write("grid-llc.ll", &llc_ll);
    scratch.write("grid-hod.ll", &hod_ll);
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    let plain = hod::compile(hod_ll.as_bytes(), &options).unwrap_or_else(|e| panic!("hod: {e}"));
    let plain = scratch.write("grid-hod.o", plain);
    let stress = scratch.write("grid-stress.o", stressed_object(hod_ll.as_bytes()));
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let Some(llc) = llvm_tool(major, "llc") else { continue };
        let peer = llc_object(&scratch, &llc, &llc_path, &[], &format!("grid-llc{major}"));
        for (who, object) in [("hod", &plain), ("stress", &stress)] {
            let exe = scratch.join(format!("grid-{who}-{major}"));
            let mut command = Command::new(&gcc);
            command.arg("-no-pie").arg(&peer).arg(object).arg("-o").arg(&exe);
            checked(&format!("link of the grid ({who}, llc {major})"), &mut command);
            let (status, out) = run(&qemu, &exe);
            let text = String::from_utf8_lossy(&out);
            if let Some(k) = text.strip_prefix("fail ").and_then(|t| t.split(' ').next()) {
                eprintln!("signature {k}: {:?}", sigs.get(k.parse::<usize>().unwrap_or(0)));
                for line in hod_ll.lines().filter(|l| l.contains(&format!("@chk{k}("))) {
                    eprintln!("{line}");
                }
            }
            assert_eq!(
                (status, String::from_utf8_lossy(&out).into_owned()),
                (Some(0), "ok\n".to_string()),
                "the grid ({who}, llc {major}): `fail k j` names signature k, argument j \
                 (40.. the result, 50.. the sret fields, 98 the stack alignment)"
            );
            runs += 1;
        }
    }
    eprintln!("i386 ABI grid: {} signatures, {runs} runs, both directions each", sigs.len());
}
