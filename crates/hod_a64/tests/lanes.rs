//! Element-wise operations on vectors (WP-080, `spec/codegen.md` §5.5.1
//! "Vectors, the lane loop (WP-080)"): every operation × element type ×
//! lane count, checked natively.
//!
//! The generator builds one function per operation, element type and
//! lane count (1, 2, 3, 4, 8, 16 and 256: both classes),
//! `void f(ptr out, ptr a, ptr b, ptr c)`, which loads its operands from
//! `a`, `b` and `c` and stores its result at `out`. Each is built by hod
//! (plain and under every stress option), by
//! `llc -O0 -global-isel=0 -fast-isel=0` of LLVM 22 and of LLVM 19 (the
//! intrinsics LLVM 19 lacks left out of that leg), and, where C can say
//! it, as C vectors by clang (`ext_vector_type`) and gcc (`vector_size`,
//! power-of-two lengths). One C driver reads the cases from its standard
//! input and prints each result with 16 sentinel bytes on either side;
//! it is linked under bfd, lld and mold. Every leg is compared lane by
//! lane with the Rust model below (`exp10`, which Rust has no function
//! for, with `llc` 22's result):
//!
//! - a lane whose result is poison or undefined behaviour is not
//!   compared: a shift by the width or more, `fptosi`/`fptoui` and
//!   `lround`..`llrint` out of range; divisors are chosen non-zero, and
//!   `MIN / -1` is avoided;
//! - a floating-point result that is a NaN matches any NaN (a computed
//!   NaN's payload is open, `LangRef`), except for `fneg`, `fabs` and
//!   `copysign`, which are bit operations;
//! - the inputs mix random lanes with each element type's edge values
//!   (zeros, ones, the extremes, subnormals, infinities, quiet NaNs,
//!   integer bounds); no input is a signalling NaN.
//!
//! `hod_interp` does not model vectors (`spec/interp.md`), so it is not
//! a leg here.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::float_cmp,
    clippy::many_single_char_names,
    clippy::format_collect,
    clippy::match_same_arms,
    clippy::needless_range_loop,
    clippy::type_complexity,
    clippy::if_not_else,
    reason = "a test generator over bit patterns: `.ll` and C text built by hand"
)]

mod common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::linking::Toolchain;
use common::{assert_refused, chunk, ll_header, parse, write_object};
use hod_a64::{compile_module, intrinsic, vector};
use hod_cg::{Options, Stress};
use hod_testutil::{Scratch, run_with, tool};

/// The lane counts every operation is built for.
const LANES: [u32; 7] = [1, 2, 3, 4, 8, 16, 256];
/// The sentinel bytes on either side of a result.
const MARGIN: usize = 16;
/// What the driver fills the output buffer with.
const SENTINEL: u8 = 0xA5;
/// The largest image: 256 lanes of 8 bytes.
const MAX_IMAGE: usize = 2048;

// ---- element types ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum E {
    I8,
    I16,
    I32,
    I64,
    Ptr,
    Half,
    Float,
    Double,
}

use E::{Double, Float, Half, I8, I16, I32, I64, Ptr};

const INTS: [E; 4] = [I8, I16, I32, I64];
const FLOATS: [E; 3] = [Half, Float, Double];

impl E {
    fn bits(self) -> u32 {
        match self {
            I8 => 8,
            I16 | Half => 16,
            I32 | Float => 32,
            I64 | Ptr | Double => 64,
        }
    }

    fn bytes(self) -> usize {
        self.bits() as usize / 8
    }

    fn mask(self) -> u64 {
        u64::MAX >> (64 - self.bits())
    }

    fn is_float(self) -> bool {
        matches!(self, Half | Float | Double)
    }

    fn ll(self) -> &'static str {
        match self {
            I8 => "i8",
            I16 => "i16",
            I32 => "i32",
            I64 => "i64",
            Ptr => "ptr",
            Half => "half",
            Float => "float",
            Double => "double",
        }
    }

    fn mangle(self) -> &'static str {
        match self {
            Ptr => "p0",
            Half => "f16",
            Float => "f32",
            Double => "f64",
            _ => self.ll(),
        }
    }

    /// The C element type: signed or unsigned integers, `float`,
    /// `double`; none for `half` and pointers.
    fn c(self, signed: bool) -> Option<&'static str> {
        Some(match (self, signed) {
            (I8, true) => "signed char",
            (I8, false) => "unsigned char",
            (I16, true) => "short",
            (I16, false) => "unsigned short",
            (I32, true) => "int",
            (I32, false) => "unsigned",
            (I64, true) => "long long",
            (I64, false) => "unsigned long long",
            (Float, _) => "float",
            (Double, _) => "double",
            _ => return None,
        })
    }
}

// ---- functions ----

/// An operand: a vector of the lane count, or a scalar `i32`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum A {
    V(E),
    S32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum What {
    Bin(&'static str),
    Fneg,
    Cast(&'static str),
    Intr(&'static str),
}

#[derive(Clone, Debug)]
struct Func {
    what: What,
    n: u32,
    ret: E,
    args: Vec<A>,
}

impl Func {
    fn new(what: What, n: u32, ret: E, args: &[A]) -> Self {
        Self { what, n, ret, args: args.to_vec() }
    }

    /// Whether llc `major` compiles the construct correctly. LLVM 19 has no
    /// ptrtoaddr, minimumnum, maximumnum or atan2, refuses vector lround,
    /// llround, lrint and llrint, cannot select vector canonicalize, and
    /// cannot scalarize fptosi.sat and fptoui.sat of `<1 x half>`;
    /// LLVM 22 cannot select canonicalize of `<1 x double>`, and both crash
    /// on ldexp of `<1 x half>`. Both store a `<3 x i8>` truncated from 64-bit
    /// lanes into the wrong bytes (lane 2 lands on byte 1), and clang with
    /// them; those functions are compared on hod and the model alone.
    fn in_llc(&self, major: u32) -> bool {
        let first = match self.args[0] {
            A::V(e) => e,
            A::S32 => I32,
        };
        let three_bytes = self.n == 3
            && self.ret == I8
            && matches!(first, I64 | Ptr)
            && matches!(self.what, What::Cast("trunc" | "ptrtoint"));
        let single = self.n == 1
            && matches!(
                (self.what, self.ret),
                (What::Intr("canonicalize"), Double) | (What::Intr("ldexp"), Half)
            );
        if three_bytes || single {
            return false;
        }
        if major < 20 {
            let half_sat = self.n == 1
                && first == Half
                && matches!(self.what, What::Intr("fptosi.sat" | "fptoui.sat"));
            return !half_sat
                && !matches!(
                    self.what,
                    What::Cast("ptrtoaddr")
                        | What::Intr(
                            "minimumnum"
                                | "maximumnum"
                                | "atan2"
                                | "lround"
                                | "llround"
                                | "lrint"
                                | "llrint"
                                | "canonicalize"
                        )
                );
        }
        true
    }

    /// The bytes of the largest operand image.
    fn in_bytes(&self) -> usize {
        self.args
            .iter()
            .map(|a| match a {
                A::V(e) => self.n as usize * e.bytes(),
                A::S32 => 4,
            })
            .max()
            .unwrap_or(0)
    }

    fn out_bytes(&self) -> usize {
        self.n as usize * self.ret.bytes()
    }

    /// Whether a NaN result must keep its bits.
    fn exact_nan(&self) -> bool {
        matches!(self.what, What::Fneg | What::Intr("fabs" | "copysign"))
    }
}

fn vty(e: E, n: u32) -> String {
    format!("<{n} x {}>", e.ll())
}

fn aty(a: A, n: u32) -> String {
    match a {
        A::V(e) => vty(e, n),
        A::S32 => "i32".into(),
    }
}

fn amangle(a: A, n: u32) -> String {
    match a {
        A::V(e) => format!("v{n}{}", e.mangle()),
        A::S32 => "i32".into(),
    }
}

/// The intrinsics whose name carries an operand's type besides the
/// result's, and which operand.
fn second_overload(name: &str) -> Option<usize> {
    match name {
        "fptosi.sat" | "fptoui.sat" | "lround" | "llround" | "lrint" | "llrint" | "scmp"
        | "ucmp" => Some(0),
        "powi" | "ldexp" | "ptrmask" => Some(1),
        _ => None,
    }
}

/// The intrinsics with a trailing `i1 false` (the poison flag clear).
fn has_flag(name: &str) -> bool {
    matches!(name, "ctlz" | "cttz" | "abs")
}

/// The `.ll` function `name`; its `declare`, if any, is added to `decls`.
fn ll(f: &Func, name: &str, decls: &mut Vec<String>) -> String {
    let n = f.n;
    let mut body = format!("define void @{name}(ptr %out, ptr %a, ptr %b, ptr %c) {{\n");
    let ptrs = ["%a", "%b", "%c"];
    for (k, &a) in f.args.iter().enumerate() {
        writeln!(body, "  %x{k} = load {}, ptr {}, align 1", aty(a, n), ptrs[k]).unwrap();
    }
    let ret = vty(f.ret, n);
    let x = |k: usize| format!("{} %x{k}", aty(f.args[k], n));
    let line = match f.what {
        What::Bin(op) => format!("{op} {}, %x1", x(0)),
        What::Fneg => format!("fneg {}", x(0)),
        What::Cast(op) => format!("{op} {} to {ret}", x(0)),
        What::Intr(iname) => {
            let mut full = format!("llvm.{iname}.{}", amangle(A::V(f.ret), n));
            if let Some(k) = second_overload(iname) {
                write!(full, ".{}", amangle(f.args[k], n)).unwrap();
            }
            let mut params: Vec<String> = f.args.iter().map(|&a| aty(a, n)).collect();
            let mut list: Vec<String> = (0..f.args.len()).map(x).collect();
            if has_flag(iname) {
                params.push("i1".into());
                list.push("i1 false".into());
            }
            let decl = format!("declare {ret} @{full}({})\n", params.join(", "));
            if !decls.contains(&decl) {
                decls.push(decl);
            }
            format!("call {ret} @{full}({})", list.join(", "))
        }
    };
    writeln!(body, "  %r = {line}\n  store {ret} %r, ptr %out, align 1\n  ret void\n}}").unwrap();
    body
}

fn module_text(funcs: &[Func], major: u32, llc: bool) -> String {
    let mut decls = Vec::new();
    let mut text = String::new();
    for (k, f) in funcs.iter().enumerate() {
        if llc && !f.in_llc(major) {
            writeln!(
                text,
                "define void @f{k}(ptr %out, ptr %a, ptr %b, ptr %c) {{\n  ret void\n}}"
            )
            .unwrap();
        } else {
            text.push_str(&ll(f, &format!("f{k}"), &mut decls));
        }
    }
    format!("{}{}{text}", ll_header(major), decls.concat())
}

// ---- the suites ----

const INT_BINARY: [&str; 13] = [
    "add", "sub", "mul", "and", "or", "xor", "shl", "lshr", "ashr", "udiv", "sdiv", "urem", "srem",
];
const FP_BINARY: [&str; 5] = ["fadd", "fsub", "fmul", "fdiv", "frem"];
const INT_UNARY: [&str; 6] = ["ctlz", "cttz", "ctpop", "bswap", "bitreverse", "abs"];
const INT_TWO: [&str; 8] =
    ["smax", "smin", "umax", "umin", "sadd.sat", "uadd.sat", "ssub.sat", "usub.sat"];
const FP_UNARY: [&str; 26] = [
    "sqrt",
    "floor",
    "ceil",
    "trunc",
    "round",
    "roundeven",
    "nearbyint",
    "rint",
    "canonicalize",
    "arithmetic.fence",
    "fabs",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "sinh",
    "cosh",
    "tanh",
    "exp",
    "exp2",
    "exp10",
    "log",
    "log2",
    "log10",
];
const FP_TWO: [&str; 9] = [
    "copysign",
    "minnum",
    "maxnum",
    "minimum",
    "maximum",
    "minimumnum",
    "maximumnum",
    "pow",
    "atan2",
];

fn integer_ops() -> Vec<Func> {
    let mut out = Vec::new();
    for n in LANES {
        for e in INTS {
            for op in INT_BINARY {
                out.push(Func::new(What::Bin(op), n, e, &[A::V(e), A::V(e)]));
            }
        }
    }
    out
}

fn float_ops() -> Vec<Func> {
    let mut out = Vec::new();
    for n in LANES {
        for e in FLOATS {
            for op in FP_BINARY {
                out.push(Func::new(What::Bin(op), n, e, &[A::V(e), A::V(e)]));
            }
            out.push(Func::new(What::Fneg, n, e, &[A::V(e)]));
        }
    }
    out
}

fn casts() -> Vec<Func> {
    let mut out = Vec::new();
    for n in LANES {
        let mut add =
            |op, from: E, to: E| out.push(Func::new(What::Cast(op), n, to, &[A::V(from)]));
        for from in INTS {
            for to in INTS {
                if from.bits() > to.bits() {
                    add("trunc", from, to);
                } else if from.bits() < to.bits() {
                    add("zext", from, to);
                    add("sext", from, to);
                }
            }
            add("inttoptr", from, Ptr);
            add("ptrtoint", Ptr, from);
            for to in FLOATS {
                add("sitofp", from, to);
                add("uitofp", from, to);
                add("fptosi", to, from);
                add("fptoui", to, from);
            }
        }
        add("ptrtoaddr", Ptr, I64);
        for from in FLOATS {
            for to in FLOATS {
                if from.bits() < to.bits() {
                    add("fpext", from, to);
                } else if from.bits() > to.bits() {
                    add("fptrunc", from, to);
                }
            }
        }
    }
    out
}

fn integer_intrinsics() -> Vec<Func> {
    let mut out = Vec::new();
    for n in LANES {
        for e in INTS {
            for name in INT_UNARY {
                if name != "bswap" || e != I8 {
                    out.push(Func::new(What::Intr(name), n, e, &[A::V(e)]));
                }
            }
            for name in INT_TWO {
                out.push(Func::new(What::Intr(name), n, e, &[A::V(e), A::V(e)]));
            }
            for name in ["scmp", "ucmp"] {
                out.push(Func::new(What::Intr(name), n, e, &[A::V(e), A::V(e)]));
            }
            for name in ["fshl", "fshr"] {
                out.push(Func::new(What::Intr(name), n, e, &[A::V(e), A::V(e), A::V(e)]));
            }
        }
        // A three-way result narrower and wider than its operands.
        out.push(Func::new(What::Intr("scmp"), n, I8, &[A::V(I32), A::V(I32)]));
        out.push(Func::new(What::Intr("ucmp"), n, I32, &[A::V(I8), A::V(I8)]));
        out.push(Func::new(What::Intr("ptrmask"), n, Ptr, &[A::V(Ptr), A::V(I64)]));
    }
    out
}

fn float_intrinsics() -> Vec<Func> {
    let mut out = Vec::new();
    for n in LANES {
        for e in FLOATS {
            let v = A::V(e);
            for name in FP_UNARY {
                out.push(Func::new(What::Intr(name), n, e, &[v]));
            }
            for name in FP_TWO {
                out.push(Func::new(What::Intr(name), n, e, &[v, v]));
            }
            for name in ["fma", "fmuladd"] {
                out.push(Func::new(What::Intr(name), n, e, &[v, v, v]));
            }
            out.push(Func::new(What::Intr("powi"), n, e, &[v, A::S32]));
            out.push(Func::new(What::Intr("ldexp"), n, e, &[v, A::V(I32)]));
            for (name, to) in [("lround", I32), ("llround", I64), ("lrint", I32), ("llrint", I64)] {
                out.push(Func::new(What::Intr(name), n, to, &[v]));
            }
            for to in INTS {
                out.push(Func::new(What::Intr("fptosi.sat"), n, to, &[v]));
                out.push(Func::new(What::Intr("fptoui.sat"), n, to, &[v]));
            }
        }
    }
    out
}

// ---- half precision ----

/// The value of a `half` pattern, exactly.
fn h2d(b: u64) -> f64 {
    let sign = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (b >> 10) & 0x1f;
    let frac = (b & 0x3ff) as f64;
    sign * match exp {
        0 => frac * 2f64.powi(-24),
        31 if frac == 0.0 => f64::INFINITY,
        31 => f64::NAN,
        _ => (1.0 + frac / 1024.0) * 2f64.powi(exp as i32 - 15),
    }
}

fn h2f(b: u64) -> f32 {
    h2d(b) as f32
}

/// `x` rounded to `half`, to nearest even (one rounding).
fn d2h(x: f64) -> u64 {
    let sign = (x.to_bits() >> 63) << 15;
    if x.is_nan() {
        return sign | 0x7e00;
    }
    let a = x.abs();
    if a >= 65520.0 {
        return sign | 0x7c00;
    }
    if a < 2f64.powi(-14) {
        return sign | (a * 2f64.powi(24)).round_ties_even() as u64;
    }
    let e = ((a.to_bits() >> 52) & 0x7ff) as i32 - 1023;
    let q = ((a / 2f64.powi(e) - 1.0) * 1024.0).round_ties_even() as u64;
    sign | ((((e + 15) as u64) << 10) + q)
}

fn f2h(x: f32) -> u64 {
    d2h(f64::from(x))
}

/// The exact value of a floating-point lane.
fn value(e: E, b: u64) -> f64 {
    match e {
        Half => h2d(b),
        Float => f64::from(f32::from_bits(b as u32)),
        Double => f64::from_bits(b),
        _ => unreachable!(),
    }
}

/// `x` rounded to `e` (`half` from `double` in one rounding).
fn round_to(e: E, x: f64) -> u64 {
    match e {
        Half => d2h(x),
        Float => u64::from((x as f32).to_bits()),
        Double => x.to_bits(),
        _ => unreachable!(),
    }
}

fn is_nan(e: E, b: u64) -> bool {
    e.is_float() && value(e, b).is_nan()
}

/// One operation in single (for `half` and `float`: `half` widened and
/// the result narrowed) or double precision.
fn fp1(e: E, x: u64, s: impl Fn(f32) -> f32, d: impl Fn(f64) -> f64) -> u64 {
    match e {
        Half => f2h(s(h2f(x))),
        Float => u64::from(s(f32::from_bits(x as u32)).to_bits()),
        Double => d(f64::from_bits(x)).to_bits(),
        _ => unreachable!(),
    }
}

fn fp2(e: E, x: u64, y: u64, s: impl Fn(f32, f32) -> f32, d: impl Fn(f64, f64) -> f64) -> u64 {
    match e {
        Half => f2h(s(h2f(x), h2f(y))),
        Float => u64::from(s(f32::from_bits(x as u32), f32::from_bits(y as u32)).to_bits()),
        Double => d(f64::from_bits(x), f64::from_bits(y)).to_bits(),
        _ => unreachable!(),
    }
}

/// `scalbn` as musl writes it: at most one rounding.
fn scalbn(x: f64, n: i32) -> f64 {
    let p = |k: i32| f64::from_bits(((0x3ff + k) as u64) << 52);
    let (mut y, mut n) = (x, n);
    if n > 1023 {
        y *= p(1023);
        n -= 1023;
        if n > 1023 {
            y *= p(1023);
            n = (n - 1023).min(1023);
        }
    } else if n < -1022 {
        y *= p(-1022) * p(53);
        n += 1022 - 53;
        if n < -1022 {
            y *= p(-1022) * p(53);
            n = (n + 1022 - 53).max(-1022);
        }
    }
    y * p(n)
}

fn scalbnf(x: f32, n: i32) -> f32 {
    let p = |k: i32| f32::from_bits(((0x7f + k) as u32) << 23);
    let (mut y, mut n) = (x, n);
    if n > 127 {
        y *= p(127);
        n -= 127;
        if n > 127 {
            y *= p(127);
            n = (n - 127).min(127);
        }
    } else if n < -126 {
        y *= p(-126) * p(24);
        n += 126 - 24;
        if n < -126 {
            y *= p(-126) * p(24);
            n = (n + 126 - 24).max(-126);
        }
    }
    y * p(n)
}

/// `fminnm`/`fmaxnm` (`num`) or `fmin`/`fmax` of two lanes, as bits:
/// the chosen operand, `-0 < +0`.
fn min_max(e: E, x: u64, y: u64, max: bool, num: bool) -> u64 {
    let (a, b) = (value(e, x), value(e, y));
    match (a.is_nan(), b.is_nan()) {
        (true, true) => x,
        (true, false) => {
            if num {
                y
            } else {
                x
            }
        }
        (false, true) => {
            if num {
                x
            } else {
                y
            }
        }
        _ if a == b => {
            let a_neg = a.is_sign_negative();
            if a_neg == max { y } else { x }
        }
        _ => {
            if (a < b) != max {
                x
            } else {
                y
            }
        }
    }
}

// ---- the model ----

fn sext(x: u64, bits: u32) -> i64 {
    ((x << (64 - bits)) as i64) >> (64 - bits)
}

/// An integer of `bits` as a lane: wrapped.
fn wrap(x: i128, bits: u32) -> u64 {
    (x as u64) & (u64::MAX >> (64 - bits))
}

/// `t` (an integer value) as an integer lane of `to`, if it fits.
fn fits(t: f64, to: E, signed: bool) -> Option<u64> {
    let w = to.bits() as i32;
    let ok = if signed {
        t >= -(2f64.powi(w - 1)) && t < 2f64.powi(w - 1)
    } else {
        t >= 0.0 && t < 2f64.powi(w)
    };
    ok.then(|| wrap(t as i128, to.bits()))
}

/// One lane of `f` from the lanes (or the scalar) `x`; `None` if it is
/// poison.
fn lane(f: &Func, x: [u64; 3]) -> Option<u64> {
    let e = match f.args[0] {
        A::V(e) => e,
        A::S32 => unreachable!(),
    };
    let w = e.bits();
    let m = e.mask();
    let (a, b, c) = (x[0] & m, x[1], x[2]);
    let ret = f.ret;
    let b_int = match f.args.get(1) {
        Some(A::V(be)) => b & be.mask(),
        _ => b & 0xffff_ffff,
    };
    let (sa, sb) = (sext(a, w), sext(b_int, w.min(64)));
    let int = |r: i128| Some(wrap(r, ret.bits()));
    match f.what {
        What::Bin(op) if !e.is_float() => {
            let b = b_int;
            match op {
                "add" => int(i128::from(a) + i128::from(b)),
                "sub" => int(i128::from(a) - i128::from(b)),
                "mul" => int(i128::from(a).wrapping_mul(i128::from(b))),
                "and" => Some(a & b),
                "or" => Some(a | b),
                "xor" => Some(a ^ b),
                "shl" => (b < u64::from(w)).then(|| (a << b) & m),
                "lshr" => (b < u64::from(w)).then(|| a >> b),
                "ashr" => (b < u64::from(w)).then(|| (sa >> b) as u64 & m),
                "udiv" => Some(a / b),
                "urem" => Some(a % b),
                "sdiv" => int(i128::from(sa) / i128::from(sb)),
                "srem" => int(i128::from(sa) % i128::from(sb)),
                _ => unreachable!("{op}"),
            }
        }
        What::Bin(op) => Some(match op {
            "fadd" => fp2(e, a, b, |p, q| p + q, |p, q| p + q),
            "fsub" => fp2(e, a, b, |p, q| p - q, |p, q| p - q),
            "fmul" => fp2(e, a, b, |p, q| p * q, |p, q| p * q),
            "fdiv" => fp2(e, a, b, |p, q| p / q, |p, q| p / q),
            "frem" => fp2(e, a, b, |p, q| p % q, |p, q| p % q),
            _ => unreachable!("{op}"),
        }),
        What::Fneg => Some(a ^ (1 << (w - 1))),
        What::Cast(op) => match op {
            "trunc" => Some(a & ret.mask()),
            "zext" | "ptrtoint" | "ptrtoaddr" | "inttoptr" => Some(a & ret.mask()),
            "sext" => Some(sa as u64 & ret.mask()),
            "fpext" | "fptrunc" => Some(round_to(ret, value(e, a))),
            "sitofp" | "uitofp" => {
                let v = if op == "sitofp" { sa as i128 } else { i128::from(a) };
                Some(match ret {
                    // Through single, as hod and `llc` convert.
                    Half => f2h(v as f32),
                    Float => u64::from((v as f32).to_bits()),
                    _ => (v as f64).to_bits(),
                })
            }
            "fptosi" | "fptoui" => fits(value(e, a).trunc(), ret, op == "fptosi"),
            _ => unreachable!("{op}"),
        },
        What::Intr(name) => intrinsic_lane(f, name, e, a, b, c),
    }
}

fn intrinsic_lane(f: &Func, name: &str, e: E, a: u64, b: u64, c: u64) -> Option<u64> {
    let w = e.bits();
    let m = e.mask();
    let ret = f.ret;
    // `powi`'s and `ldexp`'s exponent: an `i32`.
    let exponent = b as u32 as i32;
    let (b, c) = (b & m, c & m);
    let (sa, sb) = (sext(a, w), sext(b, w));
    let clamp = |r: i128, signed: bool| -> u64 {
        let (lo, hi) = if signed {
            (-(1i128 << (w - 1)), (1i128 << (w - 1)) - 1)
        } else {
            (0, (1i128 << w) - 1)
        };
        wrap(r.clamp(lo, hi), w)
    };
    let to_int = |t: f64| fits(t, ret, true);
    Some(match name {
        "ctlz" => u64::from((a << (64 - w)).leading_zeros().min(w)),
        "cttz" => u64::from(a.trailing_zeros().min(w)),
        "ctpop" => u64::from(a.count_ones()),
        "bswap" => a.swap_bytes() >> (64 - w),
        "bitreverse" => a.reverse_bits() >> (64 - w),
        "abs" => wrap(i128::from(sa).abs(), w),
        "smax" => wrap(i128::from(sa.max(sb)), w),
        "smin" => wrap(i128::from(sa.min(sb)), w),
        "umax" => a.max(b),
        "umin" => a.min(b),
        "sadd.sat" => clamp(i128::from(sa) + i128::from(sb), true),
        "ssub.sat" => clamp(i128::from(sa) - i128::from(sb), true),
        "uadd.sat" => clamp(i128::from(a) + i128::from(b), false),
        "usub.sat" => clamp(i128::from(a) - i128::from(b), false),
        "scmp" | "ucmp" => {
            let ord = if name == "scmp" { sa.cmp(&sb) } else { a.cmp(&b) };
            wrap(ord as i128, ret.bits())
        }
        "fshl" | "fshr" => {
            let s = c % u64::from(w);
            if s == 0 {
                if name == "fshl" { a } else { b }
            } else if name == "fshl" {
                ((a << s) | (b >> (u64::from(w) - s))) & m
            } else {
                ((a << (u64::from(w) - s)) | (b >> s)) & m
            }
        }
        "ptrmask" => a & b,
        "sqrt" => fp1(e, a, f32::sqrt, f64::sqrt),
        "floor" => fp1(e, a, f32::floor, f64::floor),
        "ceil" => fp1(e, a, f32::ceil, f64::ceil),
        "trunc" => fp1(e, a, f32::trunc, f64::trunc),
        "round" => fp1(e, a, f32::round, f64::round),
        "roundeven" | "nearbyint" | "rint" => fp1(e, a, f32::round_ties_even, f64::round_ties_even),
        "canonicalize" | "arithmetic.fence" => a,
        "fabs" => a & (m >> 1),
        "copysign" => (a & (m >> 1)) | (b & !(m >> 1)),
        "sin" => fp1(e, a, f32::sin, f64::sin),
        "cos" => fp1(e, a, f32::cos, f64::cos),
        "tan" => fp1(e, a, f32::tan, f64::tan),
        "asin" => fp1(e, a, f32::asin, f64::asin),
        "acos" => fp1(e, a, f32::acos, f64::acos),
        "atan" => fp1(e, a, f32::atan, f64::atan),
        "sinh" => fp1(e, a, f32::sinh, f64::sinh),
        "cosh" => fp1(e, a, f32::cosh, f64::cosh),
        "tanh" => fp1(e, a, f32::tanh, f64::tanh),
        "exp" => fp1(e, a, f32::exp, f64::exp),
        "exp2" => fp1(e, a, f32::exp2, f64::exp2),
        "log" => fp1(e, a, f32::ln, f64::ln),
        "log2" => fp1(e, a, f32::log2, f64::log2),
        "log10" => fp1(e, a, f32::log10, f64::log10),
        "pow" => fp2(e, a, b, f32::powf, f64::powf),
        "atan2" => fp2(e, a, b, f32::atan2, f64::atan2),
        "minnum" | "minimumnum" => min_max(e, a, b, false, true),
        "maxnum" | "maximumnum" => min_max(e, a, b, true, true),
        "minimum" => min_max(e, a, b, false, false),
        "maximum" => min_max(e, a, b, true, false),
        "fma" | "fmuladd" if e != Half => match e {
            Float => u64::from(
                f32::from_bits(a as u32)
                    .mul_add(f32::from_bits(b as u32), f32::from_bits(c as u32))
                    .to_bits(),
            ),
            _ => f64::from_bits(a).mul_add(f64::from_bits(b), f64::from_bits(c)).to_bits(),
        },
        // Through double, rounded once.
        "fma" => d2h(h2d(a).mul_add(h2d(b), h2d(c))),
        // A rounded `fmul`, then a rounded `fadd`.
        "fmuladd" => f2h(h2f(f2h(h2f(a) * h2f(b))) + h2f(c)),
        "powi" => fp1(e, a, |x| x.powi(exponent), |x| x.powi(exponent)),
        "ldexp" => fp1(e, a, |x| scalbnf(x, exponent), |x| scalbn(x, exponent)),
        "lround" | "llround" => return to_int(value(e, a).round()),
        "lrint" | "llrint" => return to_int(value(e, a).round_ties_even()),
        "fptosi.sat" | "fptoui.sat" => {
            let v = value(e, a);
            let signed = name == "fptosi.sat";
            let rw = ret.bits();
            // In integers: 2^64 - 1 is not a double.
            let (lo, hi) = if signed {
                (-(1i128 << (rw - 1)), (1i128 << (rw - 1)) - 1)
            } else {
                (0, (1i128 << rw) - 1)
            };
            if v.is_nan() { 0 } else { wrap((v.trunc() as i128).clamp(lo, hi), rw) }
        }
        "exp10" => return None,
        _ => unreachable!("{name}"),
    })
}

/// Whether `f` has a model (`exp10` is compared with `llc`).
fn has_model(f: &Func) -> bool {
    f.what != What::Intr("exp10")
}

// ---- inputs ----

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The edge values of `e`.
fn edges(e: E) -> Vec<u64> {
    if e.is_float() {
        let mut out: Vec<u64> = [
            0.0,
            -0.0,
            1.0,
            -1.0,
            0.5,
            -0.5,
            1.5,
            -2.5,
            2.5,
            3.0,
            -128.5,
            255.5,
            65504.0,
            -65536.0,
            2f64.powi(31),
            -(2f64.powi(31)),
            2f64.powi(63),
            1e-8,
            1e300,
            0.1,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ]
        .iter()
        .map(|&x| round_to(e, x))
        .collect();
        let (max, min_normal, min_sub) = match e {
            Half => (0x7bff, 0x0400, 0x0001),
            Float => (0x7f7f_ffff, 0x0080_0000, 0x0000_0001),
            _ => (0x7fef_ffff_ffff_ffff, 0x0010_0000_0000_0000, 1),
        };
        let sign = 1u64 << (e.bits() - 1);
        out.extend([max, max | sign, min_normal, min_normal | sign, min_sub, min_sub | sign]);
        out.push(round_to(e, f64::NAN) | sign);
        out
    } else {
        let w = e.bits();
        let m = e.mask();
        let min = 1u64 << (w - 1);
        vec![
            0,
            1,
            m,
            min,
            min - 1,
            2,
            u64::from(w) - 1,
            u64::from(w),
            u64::from(w) + 1,
            m - 1,
            0x80,
            0x7f,
        ]
        .into_iter()
        .map(|x| x & m)
        .collect()
    }
}

/// A signalling NaN made quiet; anything else unchanged.
fn quiet(e: E, b: u64) -> u64 {
    let bit = match e {
        Half => 9,
        Float => 22,
        _ => 51,
    };
    if is_nan(e, b) { b | (1 << bit) } else { b }
}

struct Case {
    func: usize,
    images: [Vec<u8>; 3],
}

/// The lane values of each operand of case `k` of `f`, made valid for
/// the operation (divisors, shift amounts).
fn lanes_for(f: &Func, k: usize, rng: &mut Rng) -> [Vec<u64>; 3] {
    let n = f.n as usize;
    let mut out: [Vec<u64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (j, &arg) in f.args.iter().enumerate() {
        let e = match arg {
            A::V(e) => e,
            A::S32 => {
                // `powi`'s exponent: small, negative, zero, large.
                let pick = [3u64, (-2i64) as u64, 0, 17, (-1i64) as u64, 1, 40][k % 7];
                out[j] = vec![pick & 0xffff_ffff];
                continue;
            }
        };
        let table = edges(e);
        for i in 0..n {
            let edge = table[(i * (2 * j + 1) + k * 5 + j * 3) % table.len()];
            let random = rng.next() & e.mask();
            let v = if k == 0 || rng.below(4) == 0 { edge } else { random };
            out[j].push(quiet(e, v));
        }
    }
    let e = match f.args[0] {
        A::V(e) => e,
        A::S32 => unreachable!(),
    };
    let w = e.bits();
    let m = e.mask();
    match f.what {
        What::Bin(op @ ("udiv" | "urem" | "sdiv" | "srem")) => {
            let signed = op.starts_with('s');
            for i in 0..n {
                let (a, b) = (out[0][i], &mut out[1][i]);
                if *b == 0 || (signed && a == 1 << (w - 1) && *b == m) {
                    *b = 1;
                }
            }
        }
        What::Bin("shl" | "lshr" | "ashr") => {
            for b in &mut out[1] {
                if rng.below(4) != 0 {
                    *b = rng.below(u64::from(w));
                }
            }
        }
        What::Intr("ldexp") => {
            // Exponents near the format's range, and the extremes.
            for (i, b) in out[1].iter_mut().enumerate() {
                if i % 3 != 0 {
                    *b = (rng.below(2200) as i64 - 1100) as u64 & 0xffff_ffff;
                }
            }
        }
        _ => {}
    }
    out
}

fn image(f: &Func, j: usize, lanes: &[u64]) -> Vec<u8> {
    let bytes = match f.args.get(j) {
        Some(A::V(e)) => e.bytes(),
        Some(A::S32) => 4,
        None => 0,
    };
    let mut out = vec![0; f.in_bytes()];
    for (i, &v) in lanes.iter().enumerate() {
        out[i * bytes..(i + 1) * bytes].copy_from_slice(&v.to_le_bytes()[..bytes]);
    }
    out
}

fn read_lane(img: &[u8], e: E, i: usize) -> u64 {
    let w = e.bytes();
    let mut b = [0u8; 8];
    b[..w].copy_from_slice(&img[i * w..(i + 1) * w]);
    u64::from_le_bytes(b)
}

fn cases(funcs: &[Func], seed: u64) -> Vec<Case> {
    let mut rng = Rng(seed);
    let mut out = Vec::new();
    for (func, f) in funcs.iter().enumerate() {
        for k in 0..2 {
            let lanes = lanes_for(f, k, &mut rng);
            let images = [image(f, 0, &lanes[0]), image(f, 1, &lanes[1]), image(f, 2, &lanes[2])];
            out.push(Case { func, images });
        }
    }
    out
}

/// The model's result lanes of `case`: `None` for a poison lane.
fn model(f: &Func, case: &Case) -> Vec<Option<u64>> {
    (0..f.n as usize)
        .map(|i| {
            let mut x = [0u64; 3];
            for (j, &arg) in f.args.iter().enumerate() {
                x[j] = match arg {
                    A::V(e) => read_lane(&case.images[j], e, i),
                    A::S32 => read_lane(&case.images[j], I32, 0),
                };
            }
            lane(f, x)
        })
        .collect()
}

fn stdin(funcs: &[Func], cases: &[Case]) -> Vec<u8> {
    let mut out = Vec::new();
    for case in cases {
        let f = &funcs[case.func];
        for word in [case.func, f.in_bytes(), f.out_bytes()] {
            out.extend_from_slice(&(word as u32).to_le_bytes());
        }
        for img in &case.images {
            out.extend_from_slice(img);
        }
    }
    out
}

// ---- C ----

fn driver(funcs: &[Func]) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <string.h>\n#include <stdint.h>\n\
         typedef void fn(void *, const void *, const void *, const void *);\n",
    );
    for k in 0..funcs.len() {
        writeln!(c, "extern fn f{k};").unwrap();
    }
    c.push_str("static fn *const funcs[] = {");
    for k in 0..funcs.len() {
        write!(c, "f{k},").unwrap();
    }
    write!(
        c,
        "}};\n\
         static unsigned char a[{MAX_IMAGE}] __attribute__((aligned(64)));\n\
         static unsigned char b[{MAX_IMAGE}] __attribute__((aligned(64)));\n\
         static unsigned char c[{MAX_IMAGE}] __attribute__((aligned(64)));\n\
         static unsigned char out[{MAX_IMAGE} + {two}] __attribute__((aligned(64)));\n\
         static char line[2 * ({MAX_IMAGE} + {two}) + 1];\n\
         int main(void) {{\n  uint32_t h[3];\n  static const char hex[] = \"0123456789abcdef\";\n  \
         while (fread(h, 4, 3, stdin) == 3) {{\n    \
         if (h[0] >= sizeof funcs / sizeof funcs[0] || h[1] > {MAX_IMAGE} || h[2] > {MAX_IMAGE}) return 3;\n    \
         if (fread(a, 1, h[1], stdin) != h[1] || fread(b, 1, h[1], stdin) != h[1]\n        \
         || fread(c, 1, h[1], stdin) != h[1]) return 2;\n    \
         memset(out, {SENTINEL}, sizeof out);\n    \
         funcs[h[0]](out + {MARGIN}, a, b, c);\n    \
         size_t n = h[2] + {two};\n    \
         for (size_t k = 0; k < n; k++) {{ line[2 * k] = hex[out[k] >> 4]; line[2 * k + 1] = hex[out[k] & 15]; }}\n    \
         line[2 * n] = '\\n';\n    fwrite(line, 1, 2 * n + 1, stdout);\n  }}\n  return 0;\n}}\n",
        two = 2 * MARGIN
    )
    .unwrap();
    c
}

/// Which C compiler a C leg is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cc {
    Clang,
    Gcc,
}

/// The C operation of `f`, if C vectors have it: the operand and result
/// element types (with their signedness) and the expression.
fn c_op(f: &Func) -> Option<(bool, bool, String)> {
    let e = match f.args[0] {
        A::V(e) => e,
        A::S32 => return None,
    };
    if !matches!(e, I8 | I16 | I32 | I64 | Float | Double) {
        return None;
    }
    let (arg_signed, ret_signed, expr) = match f.what {
        What::Bin(op) => {
            let (signed, sym) = match op {
                "add" => (false, "+"),
                "sub" | "fsub" => (false, "-"),
                "mul" | "fmul" => (false, "*"),
                "and" => (false, "&"),
                "or" => (false, "|"),
                "xor" => (false, "^"),
                "shl" => (false, "<<"),
                "lshr" => (false, ">>"),
                "ashr" => (true, ">>"),
                "udiv" => (false, "/"),
                "urem" => (false, "%"),
                "sdiv" | "fdiv" => (true, "/"),
                "srem" => (true, "%"),
                "fadd" => (false, "+"),
                _ => return None,
            };
            (signed, signed, format!("x0 {sym} x1"))
        }
        What::Fneg => (true, true, "-x0".into()),
        What::Cast(op) => {
            if !matches!(f.ret, I8 | I16 | I32 | I64 | Float | Double) {
                return None;
            }
            let (from, to) = match op {
                "trunc" | "zext" | "uitofp" => (false, false),
                "sext" | "sitofp" | "fpext" | "fptrunc" | "fptosi" => (true, true),
                "fptoui" => (true, false),
                _ => return None,
            };
            (from, to, "__builtin_convertvector(x0, R)".into())
        }
        What::Intr(_) => return None,
    };
    Some((arg_signed, ret_signed, expr))
}

/// The C module of `funcs` for `cc`, and which functions it defines.
fn c_module(funcs: &[Func], cc: Cc) -> (String, Vec<bool>) {
    let mut c = String::from("#include <string.h>\n");
    let mut compared = Vec::new();
    for (k, f) in funcs.iter().enumerate() {
        let name = format!("f{k}");
        let op = c_op(f).filter(|_| (cc == Cc::Clang || f.n.is_power_of_two()) && f.in_llc(22));
        let Some((arg_signed, ret_signed, expr)) = op else {
            writeln!(c, "void {name}(void *o, const void *a, const void *b, const void *c) {{}}")
                .unwrap();
            compared.push(false);
            continue;
        };
        let A::V(e) = f.args[0] else { unreachable!() };
        let vector = |e: E, signed: bool, t: &str| {
            let base = e.c(signed).unwrap();
            match cc {
                Cc::Clang => {
                    format!("typedef {base} {t} __attribute__((ext_vector_type({})));", f.n)
                }
                Cc::Gcc => format!(
                    "typedef {base} {t} __attribute__((vector_size({})));",
                    f.n as usize * e.bytes()
                ),
            }
        };
        writeln!(
            c,
            "void {name}(void *o, const void *a, const void *b, const void *c) {{\n  \
             {}\n  {}\n  X x0, x1;\n  memcpy(&x0, a, {in_bytes});\n  memcpy(&x1, b, {in_bytes});\n  \
             R r = {expr};\n  memcpy(o, &r, {out});\n}}",
            vector(e, arg_signed, "X"),
            vector(f.ret, ret_signed, "R"),
            in_bytes = f.n as usize * e.bytes(),
            out = f.out_bytes(),
        )
        .unwrap();
        compared.push(true);
    }
    (c, compared)
}

// ---- the legs ----

/// What a run found.
#[derive(Default)]
struct Report {
    legs: Vec<String>,
    compared: usize,
    wrong: Vec<String>,
}

fn parse_line(line: &str) -> Vec<u8> {
    (0..line.len() / 2).map(|i| u8::from_str_radix(&line[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether lane value `got` of `f` is `want`.
fn same(f: &Func, got: u64, want: u64) -> bool {
    got == want || (f.ret.is_float() && !f.exact_nan() && is_nan(f.ret, got) && is_nan(f.ret, want))
}

fn check_leg(
    funcs: &[Func],
    cases: &[Case],
    leg: &str,
    lines: &[Vec<u8>],
    reference: Option<&[Vec<u8>]>,
    compared: Option<&[bool]>,
    report: &mut Report,
) {
    assert_eq!(lines.len(), cases.len(), "{leg}: one line per case");
    for (c, (case, got)) in cases.iter().zip(lines).enumerate() {
        let f = &funcs[case.func];
        if compared.is_some_and(|yes| !yes[case.func]) {
            continue;
        }
        let n = f.out_bytes();
        let sentinels = got[..MARGIN].iter().chain(&got[MARGIN + n..]).all(|&b| b == SENTINEL);
        let want: Vec<Option<u64>> = if has_model(f) {
            model(f, case)
        } else if let Some(reference) = reference {
            (0..f.n as usize).map(|i| Some(read_lane(&reference[c][MARGIN..], f.ret, i))).collect()
        } else {
            continue;
        };
        let body = &got[MARGIN..MARGIN + n];
        let bad: Vec<usize> = want
            .iter()
            .enumerate()
            .filter(|&(i, w)| w.is_some_and(|w| !same(f, read_lane(body, f.ret, i), w)))
            .map(|(i, _)| i)
            .collect();
        report.compared += 1;
        if !sentinels || !bad.is_empty() {
            let i = bad.first().copied().unwrap_or(0);
            let lane_of = |j: usize| match f.args.get(j) {
                Some(A::V(e)) => format!("{:x}", read_lane(&case.images[j], *e, i)),
                Some(A::S32) => format!("{:x}", read_lane(&case.images[j], I32, 0)),
                None => String::new(),
            };
            report.wrong.push(format!(
                "{leg}: f{} {f:?}: lane {i} of {} wrong ({} lanes){}: operands {} {} {}, want {:x?}, got {:x}\n  got {}",
                case.func,
                f.n,
                bad.len(),
                if sentinels { "" } else { " (sentinels overwritten)" },
                lane_of(0),
                lane_of(1),
                lane_of(2),
                want[i],
                read_lane(body, f.ret, i),
                hex(body),
            ));
        }
    }
}

fn llc_object(scratch: &Scratch, major: u32, text: &str) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        return tool(&format!("llc-{major}"));
    }
    let input = scratch.write(&format!("llc{major}.ll"), text);
    let output = scratch.join(format!("llc{major}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major}:\n{}", outcome.stderr_text());
    Some(output)
}

fn c_object(scratch: &Scratch, cc: Cc, source: &str) -> Option<PathBuf> {
    let (compiler, name) = match cc {
        Cc::Clang => (tool("clang")?, "clang"),
        Cc::Gcc => (hod_testutil::Target::Aarch64.tool("gcc")?, "gcc"),
    };
    let input = scratch.write(&format!("c-{name}.c"), source);
    let output = scratch.join(format!("c-{name}.o"));
    let mut command = Command::new(compiler);
    command.args(["-c", "-O1", "-w", "-fno-strict-aliasing"]).arg(&input).arg("-o").arg(&output);
    // AArch64 objects on any host: clang's target, the cross gcc.
    if matches!(cc, Cc::Clang) {
        command.arg("--target=aarch64-linux-gnu");
    }
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "{name}:\n{}", outcome.stderr_text());
    Some(output)
}

fn run(name: &str, funcs: &[Func], seed: u64) -> Report {
    let text = module_text(funcs, 22, false);
    if let Ok(dir) = std::env::var("HOD_LANES_DUMP") {
        std::fs::write(format!("{dir}/{name}.ll"), &text).unwrap();
    }
    let (module, bodies) = parse(&text);
    let scratch = Scratch::new(&format!("a64-lanes-{name}"));
    let mut legs: Vec<(String, PathBuf, Option<Vec<bool>>)> = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let options = Options { stress, ..Options::default() };
        let emitted =
            compile_module(&module, &bodies, &options).unwrap_or_else(|e| panic!("{mode}: {e}"));
        legs.push((
            format!("hod-{mode}"),
            write_object(&scratch, &format!("hod-{mode}.o"), &emitted),
            None,
        ));
    }
    for major in [22, 19] {
        if let Some(object) = llc_object(&scratch, major, &module_text(funcs, major, true)) {
            let compared = funcs.iter().map(|f| f.in_llc(major)).collect();
            legs.push((format!("llc{major}"), object, Some(compared)));
        }
    }
    for (cc, label) in [(Cc::Clang, "clang"), (Cc::Gcc, "gcc")] {
        let (c, compared) = c_module(funcs, cc);
        if compared.iter().any(|&yes| yes)
            && let Some(object) = c_object(&scratch, cc, &c)
        {
            legs.push((label.into(), object, Some(compared)));
        }
    }
    let cases = cases(funcs, seed);
    let input = stdin(funcs, &cases);
    let mut report = Report::default();
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return report };
    let driver = toolchain.compile_c(&scratch, "driver", &driver(funcs), &[]);
    let mut outputs: Vec<(String, Vec<Vec<u8>>, Option<Vec<bool>>)> = Vec::new();
    for (leg, object, compared) in &legs {
        // hod's plain object under every linker; the others under one.
        let linkers =
            if leg == "hod-plain" { &toolchain.linkers[..] } else { &toolchain.linkers[..1] };
        for linker in linkers {
            let label = format!("{leg}/{}", linker.name);
            let program = scratch.join(label.replace('/', "-"));
            let inputs: [&Path; 2] = [&driver, object];
            toolchain.link_ok(linker, &["-Wl,--no-as-needed", "-lm"], &inputs, &program);
            let ran =
                run_with(&mut toolchain.runner_command(&program), &input, Duration::from_mins(3));
            assert!(ran.success(), "{label}: {:?}\n{}", ran.status, ran.stderr_text());
            let lines = ran.stdout_text().lines().map(parse_line).collect();
            outputs.push((label, lines, compared.clone()));
        }
    }
    let reference =
        outputs.iter().find(|(label, ..)| label.starts_with("llc22")).map(|o| o.1.clone());
    for (label, lines, compared) in &outputs {
        check_leg(
            funcs,
            &cases,
            label,
            lines,
            reference.as_deref(),
            compared.as_deref(),
            &mut report,
        );
        report.legs.push(label.clone());
    }
    report
}

fn assert_agrees(name: &str, funcs: &[Func], report: &Report) {
    eprintln!(
        "a64 lanes {name}: {} functions, {} comparisons over legs {:?}",
        funcs.len(),
        report.compared,
        report.legs
    );
    if report.wrong.is_empty() {
        return;
    }
    let mut message = format!("{} disagreements; the first:\n", report.wrong.len());
    for line in report.wrong.iter().take(12) {
        writeln!(message, "{line}").unwrap();
    }
    panic!("{message}");
}

fn check(name: &str, funcs: &[Func], seed: u64) {
    let report = run(name, funcs, seed);
    assert_agrees(name, funcs, &report);
}

// ---- tests ----

/// The integer binary operations, every width and lane count.
#[test]
fn integer_operations_agree() {
    check("int", &integer_ops(), 0x0800_0001);
}

/// `fadd` .. `frem` and `fneg` of `half`, `float` and `double`.
#[test]
fn floating_point_operations_agree() {
    check("float", &float_ops(), 0x0800_0002);
}

/// Every cast between vectors, within and across classes.
#[test]
fn casts_agree() {
    check("cast", &casts(), 0x0800_0003);
}

/// The integer intrinsics.
#[test]
fn integer_intrinsics_agree() {
    check("intr", &integer_intrinsics(), 0x0800_0004);
}

/// The floating-point intrinsics, inline and called.
#[test]
fn floating_point_intrinsics_agree() {
    check("fpintr", &float_intrinsics(), 0x0800_0005);
}

/// The generator itself: the model's edge cases are where they should
/// be (no tools needed).
#[test]
fn the_generator_covers_its_edges() {
    assert_eq!(d2h(65504.0), 0x7bff);
    assert_eq!(d2h(65519.99), 0x7bff);
    assert_eq!(d2h(65520.0), 0x7c00);
    assert_eq!(d2h(2f64.powi(-24)), 1);
    assert_eq!(d2h(2f64.powi(-25)), 0);
    assert_eq!(d2h(3.0 * 2f64.powi(-25)), 2);
    assert_eq!(d2h(1.0 + 2f64.powi(-11)), 0x3c00);
    assert_eq!(d2h(1.0 + 3.0 * 2f64.powi(-11)), 0x3c02);
    for b in [0u64, 1, 0x3ff, 0x400, 0x3c00, 0x7bff, 0x8001, 0xfbff] {
        assert_eq!(d2h(h2d(b)), b);
    }
    assert_eq!(scalbn(1.0, -1074), f64::from_bits(1));
    assert_eq!(scalbn(1.5, -1074), f64::from_bits(2));
    assert_eq!(scalbnf(1.0, 128), f32::INFINITY);
    let all: Vec<Func> =
        [integer_ops(), float_ops(), casts(), integer_intrinsics(), float_intrinsics()].concat();
    for n in LANES {
        assert!(all.iter().any(|f| f.n == n && f.what == What::Bin("udiv")));
    }
    let (m, bodies) = parse(&module_text(&all, 22, false));
    assert_eq!(bodies.len(), all.len());
    drop(m);
}

/// Every vector shape a lane operation does not handle is refused by its
/// name, one construct per function.
#[test]
fn what_is_refused_is_refused_by_name() {
    let binary = |t: &str, op: &str| {
        format!(
            "define void @f(ptr %p) {{\n  %v = load {t}, ptr %p\n  \
             %r = {op} {t} %v, %v\n  store {t} %r, ptr %p\n  ret void\n}}\n"
        )
    };
    let cases = [
        // WP-101: `i128` and `fp128` lanes compute; these stay refused.
        (binary("<2 x i128>", "shl"), hod_a64::tail3::I128_OP),
        (binary("<4 x i7>", "add"), vector::ODD_ELEMENT),
        (binary("<3 x i24>", "mul"), vector::ODD_ELEMENT),
        (binary("<257 x i8>", "add"), vector::LENGTH),
        (binary("<2 x i128>", "udiv"), hod_a64::tail3::I128_OP),
        // WP-101: `bfloat` lanes compute (`tests/tail3.rs`); a cast from
        // integer lanes stays refused.
        (
            "define void @f(ptr %p) {\n  %v = load <4 x i16>, ptr %p\n  \
             %r = sitofp <4 x i16> %v to <4 x bfloat>\n  store <4 x bfloat> %r, ptr %p\n  ret void\n}\n"
                .into(),
            hod_a64::tail3::BFLOAT_CAST,
        ),
        (
            "define void @f(ptr %p) {\n  %v = load <4 x i16>, ptr %p\n  \
             %r = sitofp <4 x i16> %v to <4 x fp128>\n  store <4 x fp128> %r, ptr %p\n  ret void\n}\n"
                .into(),
            hod_a64::tail3::WIDE_CAST,
        ),
        (
            "declare { <4 x float>, <4 x i32> } @llvm.frexp.v4f32.v4i32(<4 x float>)\n\
             define void @f(ptr %p) {\n  %v = load <4 x float>, ptr %p\n  \
             %r = call { <4 x float>, <4 x i32> } @llvm.frexp.v4f32.v4i32(<4 x float> %v)\n  \
             store { <4 x float>, <4 x i32> } %r, ptr %p\n  ret void\n}\n"
                .into(),
            intrinsic::VECTOR,
        ),
    ];
    let cases: Vec<(&str, &'static str)> = cases.iter().map(|(f, r)| (f.as_str(), *r)).collect();
    assert_refused(&cases);
}

/// The longest vectors compile, every family at 256 lanes, with the
/// operands and the result in far frame slots.
#[test]
fn the_longest_vectors_compile() {
    let funcs: Vec<Func> =
        [integer_ops(), float_ops(), casts(), integer_intrinsics(), float_intrinsics()]
            .concat()
            .into_iter()
            .filter(|f| f.n == 256)
            .collect();
    let text = module_text(&funcs, 22, false);
    let (m, bodies) = parse(&text);
    for stress in [Stress::default(), Stress::ALL] {
        let options = Options { stress, ..Options::default() };
        chunk(&m, &bodies, &options).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// The emitter's cost is linear in the lane count: the code of one lane
/// is the same number of bytes at every lane (the bytes are compared;
/// the time is reported, not asserted).
#[test]
fn the_code_is_linear_in_the_lanes() {
    let ops: [(&str, E, &[A]); 5] = [
        ("add", I32, &[A::V(I32), A::V(I32)]),
        ("udiv", I64, &[A::V(I64), A::V(I64)]),
        ("fadd", Half, &[A::V(Half), A::V(Half)]),
        ("sitofp", Double, &[A::V(I16)]),
        ("fshl", I8, &[A::V(I8), A::V(I8), A::V(I8)]),
    ];
    for (op, ret, args) in ops {
        let what = match op {
            "sitofp" => What::Cast(op),
            "fshl" => What::Intr(op),
            _ => What::Bin(op),
        };
        let mut sizes = Vec::new();
        for n in [16u32, 32, 64, 128, 256] {
            let f = Func::new(what, n, ret, args);
            let (m, bodies) = parse(&module_text(std::slice::from_ref(&f), 22, false));
            let start = Instant::now();
            let code = chunk(&m, &bodies, &Options::default()).unwrap().code.len();
            let ns = start.elapsed().as_nanos() / u128::from(n);
            eprintln!("a64 lanes cost: {op} <{n} x {}>: {code} bytes, {ns} ns per lane", ret.ll());
            sizes.push((n, code));
        }
        let per = |k: usize| (sizes[k + 1].1 - sizes[k].1) / (sizes[k + 1].0 - sizes[k].0) as usize;
        let first = per(0);
        for k in 1..sizes.len() - 1 {
            assert_eq!(per(k), first, "{op}: bytes per lane {sizes:?}");
        }
        eprintln!("a64 lanes cost: {op}: {first} bytes per lane");
    }
}
