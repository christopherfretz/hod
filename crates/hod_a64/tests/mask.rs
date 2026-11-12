//! Vector masks (WP-081, `spec/codegen.md` §5.5.1 "Vectors, masks
//! (WP-081)"): `<N x i1>` of up to 64 lanes as a bit-packed general
//! register, checked natively.
//!
//! The generator builds one function per construct, lane type and lane
//! count, `void f(ptr out, ptr a, ptr b, ptr c)`, which loads its operands
//! from `a`, `b` and `c` and stores its result at `out`, and a value-form
//! twin `R g(T x0, ...)` with the same body for `hod_interp`. The
//! constructs: vector `icmp`/`fcmp` (masks out of lanes), the logical and
//! arithmetic operations and `icmp` on masks, `select` on a mask, the
//! casts between masks and lanes, `extractelement`/`insertelement` with
//! constant and variable indices, shuffles, `phi` and scalar `select` of
//! masks, bitcasts between masks and integers or byte vectors, constants
//! with poison lanes, `llvm.vector.reduce.*` of masks and of lanes, and
//! the chains libc++'s `simd_utils` builds (`sext (icmp)`, `select
//! (fcmp)`, `reduce.or (icmp)`, `cttz (bitcast (icmp))`).
//!
//! Each pointer-form function is built by hod (plain and under every
//! stress option) and by `llc -O0 -global-isel=0 -fast-isel=0` of LLVM 22
//! and 19; one C driver reads the cases from its standard input and
//! prints each result with 16 sentinel bytes on either side; hod's plain
//! object is linked under bfd, lld and mold. Every leg, and `hod_interp`
//! on the twins (not for `half`, which it lacks), is compared with the
//! Rust model below, bit by bit:
//!
//! - a stored mask's bits from N up to the end of its last byte must be
//!   zero (what `llc` and the interpreter do; §5.5.1);
//! - a lane that is poison (a poison lane of a constant or a shuffle
//!   mask, an index out of range, `fptoui`/`fptosi` out of `i1`'s range)
//!   is not compared, and neither are the bits above `N` of a stored
//!   `iN` (`LangRef`: unspecified);
//! - a NaN result matches any NaN; `fmax`/`fmin` reductions may return
//!   either zero for a `+0`/`-0` pair.
//!
//! A second run feeds the hod legs masks whose padding bits are set: hod
//! assumes nothing of the bits above N it loads (`llc` assumes they are
//! zero, so it is not a leg there).

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
    reason = "a test generator over bit patterns: `.ll` and C text built by hand"
)]

mod common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::linking::Toolchain;
use common::{assert_refused, chunk, ll_header, parse, write_object};
use hod_a64::{compile_module, intrinsic, mask, vector};
use hod_cg::{Options, Stress};
use hod_interp::{Interp, Program, Stop, Trap, Val, Vector};
use hod_ir::TypeKind;
use hod_testutil::{Scratch, run_with, tool};

/// The bytes around each result.
const MARGIN: usize = 16;
/// What the driver fills them with.
const SENTINEL: u8 = 0xA5;
/// The largest operand or result image.
const MAX_IMAGE: usize = 1024;
/// Cases per function.
const CASES: usize = 6;

// ---- types ----

/// An element type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum E {
    I(u32),
    Ptr,
    Half,
    Float,
    Double,
}

use E::{Double, Float, Half, I, Ptr};

const INTS: [E; 4] = [I(8), I(16), I(32), I(64)];
const FLOATS: [E; 3] = [Half, Float, Double];

impl E {
    fn ll(self) -> String {
        match self {
            I(b) => format!("i{b}"),
            Ptr => "ptr".into(),
            Half => "half".into(),
            Float => "float".into(),
            Double => "double".into(),
        }
    }

    fn bits(self) -> u32 {
        match self {
            I(b) => b,
            Ptr | Double => 64,
            Half => 16,
            Float => 32,
        }
    }

    fn bytes(self) -> usize {
        self.bits().div_ceil(8) as usize
    }

    fn is_float(self) -> bool {
        matches!(self, Half | Float | Double)
    }

    /// The suffix of an overloaded intrinsic.
    fn mangle(self) -> String {
        match self {
            I(b) => format!("i{b}"),
            Ptr => "p0".into(),
            Half => "f16".into(),
            Float => "f32".into(),
            Double => "f64".into(),
        }
    }
}

/// An operand or result type: a vector, or a scalar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum T {
    V(E, u32),
    S(E),
}

use T::{S, V};

impl T {
    fn ll(self) -> String {
        match self {
            V(e, n) => format!("<{n} x {}>", e.ll()),
            S(e) => e.ll(),
        }
    }

    fn mangle(self) -> String {
        match self {
            V(e, n) => format!("v{n}{}", e.mangle()),
            S(e) => e.mangle(),
        }
    }

    /// The store size.
    fn size(self) -> usize {
        match self {
            V(I(1), n) => n.div_ceil(8) as usize,
            V(e, n) => n as usize * e.bytes(),
            S(e) => e.bytes(),
        }
    }

    fn lanes(self) -> u32 {
        match self {
            V(_, n) => n,
            S(_) => 1,
        }
    }

    fn elem(self) -> E {
        match self {
            V(e, _) | S(e) => e,
        }
    }

    fn is_mask(self) -> bool {
        matches!(self, V(I(1), _))
    }
}

fn low(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1 << bits) - 1 }
}

/// Lane `i` of the image `img` of type `t`, zero-extended.
fn get(t: T, img: &[u8], i: u32) -> u64 {
    if t.is_mask() {
        return u64::from(img[i as usize / 8] >> (i % 8) & 1);
    }
    let e = t.elem();
    let at = i as usize * e.bytes();
    let mut v = 0u64;
    for k in 0..e.bytes() {
        v |= u64::from(img[at + k]) << (8 * k);
    }
    v & low(e.bits())
}

// ---- functions ----

/// What a function computes (its body is built from it).
#[derive(Clone, PartialEq, Debug)]
enum What {
    Icmp(&'static str),
    Fcmp(&'static str),
    MaskBin(&'static str),
    MaskIcmp(&'static str),
    Select,
    Cast(&'static str),
    Extract(Option<u32>),
    Insert(Option<u32>),
    Shuffle(Vec<i32>),
    Reduce(&'static str),
    /// A `phi` of two masks (`false`), or a scalar `select` (`true`).
    Phi(bool),
    MaskTo,
    MaskFrom,
    ConstPoison,
    CmpSext,
    CmpMin,
    CmpAny,
    CmpCttz,
}

#[derive(Clone, Debug)]
struct Func {
    what: What,
    /// The lane count of the mask.
    n: u32,
    args: Vec<T>,
    ret: T,
}

const ICMP: [&str; 10] = ["eq", "ne", "ugt", "uge", "ult", "ule", "sgt", "sge", "slt", "sle"];
const FCMP: [&str; 16] = [
    "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "ueq", "ugt", "uge", "ult", "ule",
    "une", "uno", "true",
];
const MASK_BIN: [&str; 10] =
    ["add", "sub", "mul", "and", "or", "xor", "shl", "lshr", "ashr", "udiv"];
const REDUCE_INT: [&str; 9] = ["add", "mul", "and", "or", "xor", "smax", "smin", "umax", "umin"];
/// The lane counts of the mask families.
const MASK_LANES: [u32; 11] = [1, 2, 3, 4, 7, 8, 13, 16, 32, 33, 64];
/// The lane counts of the lane families.
const LANES: [u32; 8] = [1, 2, 3, 4, 8, 16, 32, 64];

/// Lane `i` of the constant pattern.
fn pattern_bit(i: u32) -> u64 {
    u64::from((i * 7 + 3) % 5 < 2)
}

/// The pattern as a mask constant; with `poison`, every fifth lane is
/// poison.
fn pattern(n: u32, poison: bool) -> String {
    let lanes: Vec<String> = (0..n)
        .map(|i| {
            if poison && i % 5 == 4 {
                "i1 poison".into()
            } else if pattern_bit(i) == 1 {
                "i1 true".into()
            } else {
                "i1 false".into()
            }
        })
        .collect();
    format!("<{}>", lanes.join(", "))
}

fn ones(n: u32) -> String {
    format!("<{}>", vec!["i1 true"; n as usize].join(", "))
}

impl Func {
    fn new(what: What, n: u32, args: &[T], ret: T) -> Func {
        Func { what, n, args: args.to_vec(), ret }
    }

    fn mask(&self) -> T {
        V(I(1), self.n)
    }

    /// Whether `hod_interp` runs the twin: it has no `half`, and no
    /// `fmax`/`fmin` reductions.
    fn interp(&self) -> bool {
        !self.args.iter().chain([&self.ret]).any(|t| t.elem() == Half)
            && !matches!(self.what, What::Reduce("fmax" | "fmin"))
    }

    /// The body: the operands are `%x0`.., the result `%r`; declarations
    /// go to `decls`.
    fn body(&self, decls: &mut Vec<String>) -> String {
        let m = self.mask().ll();
        let n = self.n;
        let t0 = self.args.first().map(|t| t.ll()).unwrap_or_default();
        let ret = self.ret.ll();
        let mut declare = |text: String| {
            if !decls.contains(&text) {
                decls.push(text);
            }
        };
        match &self.what {
            What::Icmp(p) | What::MaskIcmp(p) => format!("  %r = icmp {p} {t0} %x0, %x1\n"),
            What::Fcmp(p) => format!("  %r = fcmp {p} {t0} %x0, %x1\n"),
            What::MaskBin(op) => {
                let rhs = match *op {
                    "shl" | "lshr" | "ashr" => "zeroinitializer".into(),
                    "udiv" => ones(n),
                    _ => "%x1".into(),
                };
                format!("  %r = {op} {m} %x0, {rhs}\n")
            }
            What::Select => format!("  %r = select {m} %x0, {ret} %x1, {ret} %x2\n"),
            What::Cast(op) => format!("  %r = {op} {t0} %x0 to {ret}\n"),
            What::Extract(Some(k)) => format!("  %r = extractelement {m} %x0, i32 {k}\n"),
            What::Extract(None) => format!("  %r = extractelement {m} %x0, i32 %x1\n"),
            What::Insert(Some(k)) => format!("  %r = insertelement {m} %x0, i1 %x1, i32 {k}\n"),
            What::Insert(None) => format!("  %r = insertelement {m} %x0, i1 %x1, i32 %x2\n"),
            What::Shuffle(sel) => {
                let lanes: Vec<String> = sel
                    .iter()
                    .map(|&s| if s < 0 { "i32 poison".into() } else { format!("i32 {s}") })
                    .collect();
                format!(
                    "  %r = shufflevector {m} %x0, {m} %x1, <{} x i32> <{}>\n",
                    sel.len(),
                    lanes.join(", ")
                )
            }
            What::Reduce(op) => {
                let v = self.args[0];
                let e = v.elem();
                let name = format!("llvm.vector.reduce.{op}.{}", v.mangle());
                if matches!(*op, "fadd" | "fmul") {
                    declare(format!("declare {} @{name}({}, {})", e.ll(), e.ll(), v.ll()));
                    format!("  %r = call {} @{name}({} %x1, {} %x0)\n", e.ll(), e.ll(), v.ll())
                } else {
                    declare(format!("declare {} @{name}({})", e.ll(), v.ll()));
                    format!("  %r = call {} @{name}({} %x0)\n", e.ll(), v.ll())
                }
            }
            What::Phi(scalar) => {
                let head = format!(
                    "  %m0 = icmp slt {t0} %x0, %x1\n  %m1 = icmp eq {t0} %x0, %x1\n  \
                     %c = extractelement {m} %m0, i32 0\n"
                );
                if *scalar {
                    format!("{head}  %r = select i1 %c, {m} %m1, {m} %m0\n")
                } else {
                    format!(
                        "{head}  br i1 %c, label %t, label %e\nt:\n  br label %j\ne:\n  br label %j\n\
                         j:\n  %r = phi {m} [ %m1, %t ], [ %m0, %e ]\n"
                    )
                }
            }
            What::MaskTo => {
                format!("  %m = icmp ult {t0} %x0, %x1\n  %r = bitcast {m} %m to {ret}\n")
            }
            What::MaskFrom => format!(
                "  %m = bitcast {t0} %x0 to {m}\n  %r = xor {m} %m, {}\n",
                pattern(n, false)
            ),
            What::ConstPoison => format!("  %r = or {m} %x0, {}\n", pattern(n, true)),
            What::CmpSext => {
                format!("  %m = icmp sgt {t0} %x0, %x1\n  %r = sext {m} %m to {ret}\n")
            }
            What::CmpMin => {
                format!("  %m = fcmp olt {t0} %x0, %x1\n  %r = select {m} %m, {t0} %x0, {t0} %x1\n")
            }
            What::CmpAny => {
                let name = format!("llvm.vector.reduce.or.v{n}i1");
                declare(format!("declare i1 @{name}({m})"));
                format!("  %m = icmp eq {t0} %x0, %x1\n  %r = call i1 @{name}({m} %m)\n")
            }
            What::CmpCttz => {
                let name = format!("llvm.cttz.i{n}");
                declare(format!("declare i{n} @{name}(i{n}, i1)"));
                format!(
                    "  %m = icmp ne {t0} %x0, %x1\n  %b = bitcast {m} %m to i{n}\n  \
                     %r = call i{n} @{name}(i{n} %b, i1 false)\n"
                )
            }
        }
    }
}

fn icmps() -> Vec<Func> {
    let mut out = Vec::new();
    for e in INTS {
        for n in LANES {
            let preds: &[&str] =
                if matches!(n, 1 | 3 | 4 | 16 | 64) { &ICMP } else { &["eq", "slt", "ugt"] };
            for p in preds {
                out.push(Func::new(What::Icmp(p), n, &[V(e, n), V(e, n)], V(I(1), n)));
            }
        }
    }
    for n in [2, 3, 8] {
        for p in ["eq", "ult", "sge"] {
            out.push(Func::new(What::Icmp(p), n, &[V(Ptr, n), V(Ptr, n)], V(I(1), n)));
        }
    }
    out
}

fn fcmps() -> Vec<Func> {
    let mut out = Vec::new();
    for e in FLOATS {
        for n in LANES {
            let preds: &[&str] =
                if matches!(n, 1 | 3 | 4 | 64) { &FCMP } else { &["oeq", "ult", "une", "uno"] };
            for p in preds {
                out.push(Func::new(What::Fcmp(p), n, &[V(e, n), V(e, n)], V(I(1), n)));
            }
        }
    }
    out
}

fn mask_ops() -> Vec<Func> {
    let mut out = Vec::new();
    for n in MASK_LANES {
        let m = V(I(1), n);
        for op in MASK_BIN {
            out.push(Func::new(What::MaskBin(op), n, &[m, m], m));
        }
        for p in ICMP {
            out.push(Func::new(What::MaskIcmp(p), n, &[m, m], m));
        }
        out.push(Func::new(What::Select, n, &[m, m, m], m));
        out.push(Func::new(What::ConstPoison, n, &[m], m));
        out.push(Func::new(What::Phi(false), n, &[V(I(32), n), V(I(32), n)], m));
        out.push(Func::new(What::Phi(true), n, &[V(I(32), n), V(I(32), n)], m));
        let edge = [0, n / 2, n - 1];
        for k in edge {
            out.push(Func::new(What::Extract(Some(k)), n, &[m], S(I(1))));
            out.push(Func::new(What::Insert(Some(k)), n, &[m, S(I(1))], m));
        }
        out.push(Func::new(What::Extract(None), n, &[m, S(I(32))], S(I(1))));
        out.push(Func::new(What::Insert(None), n, &[m, S(I(1)), S(I(32))], m));
        for op in REDUCE_INT {
            out.push(Func::new(What::Reduce(op), n, &[m], S(I(1))));
        }
        // Integers and byte vectors of the mask's size.
        let mut sides = vec![S(I(n))];
        if n % 8 == 0 {
            sides.push(V(I(8), n / 8));
        }
        if n % 16 == 0 {
            sides.push(V(I(16), n / 16));
        }
        if n == 64 {
            sides.extend([S(Double), V(Float, 2), V(I(32), 2)]);
        }
        if n == 32 {
            sides.push(S(Float));
        }
        if n == 16 {
            sides.push(S(Half));
        }
        for side in sides {
            out.push(Func::new(What::MaskTo, n, &[V(I(8), n), V(I(8), n)], side));
            out.push(Func::new(What::MaskFrom, n, &[side], m));
        }
    }
    // A <24 x i1> as <3 x i8>, a vector of class Mem.
    out.push(Func::new(What::MaskTo, 24, &[V(I(8), 24), V(I(8), 24)], V(I(8), 3)));
    out.push(Func::new(What::MaskFrom, 24, &[V(I(8), 3)], V(I(1), 24)));
    out
}

fn shuffles() -> Vec<Func> {
    let mut out = Vec::new();
    for n in [1u32, 3, 4, 8, 13, 32, 64] {
        let m = V(I(1), n);
        let n2 = 2 * n as i32;
        let n = n as i32;
        let sels: Vec<Vec<i32>> = vec![
            (0..n).rev().collect(),
            (0..n).map(|i| if i % 2 == 0 { i } else { n + i }).collect(),
            (0..n).map(|i| if i % 3 == 1 { -1 } else { (i * 5 + 1) % n2 }).collect(),
            (0..(n2.min(64))).collect(),
            (0..n.min(5)).map(|i| n + i).collect(),
            vec![0; 7],
        ];
        for sel in sels {
            let r = V(I(1), sel.len() as u32);
            out.push(Func::new(What::Shuffle(sel), n as u32, &[m, m], r));
        }
    }
    out
}

fn casts() -> Vec<Func> {
    let mut out = Vec::new();
    for n in LANES {
        let m = V(I(1), n);
        for e in INTS {
            out.push(Func::new(What::Cast("zext"), n, &[m], V(e, n)));
            out.push(Func::new(What::Cast("sext"), n, &[m], V(e, n)));
            out.push(Func::new(What::Cast("trunc"), n, &[V(e, n)], m));
            out.push(Func::new(What::CmpSext, n, &[V(e, n), V(e, n)], V(e, n)));
        }
        for e in FLOATS {
            out.push(Func::new(What::Cast("uitofp"), n, &[m], V(e, n)));
            out.push(Func::new(What::Cast("sitofp"), n, &[m], V(e, n)));
            out.push(Func::new(What::Cast("fptoui"), n, &[V(e, n)], m));
            out.push(Func::new(What::Cast("fptosi"), n, &[V(e, n)], m));
        }
    }
    out
}

fn lane_ops() -> Vec<Func> {
    let mut out = Vec::new();
    for n in LANES {
        let m = V(I(1), n);
        for e in INTS.into_iter().chain(FLOATS).chain([Ptr]) {
            out.push(Func::new(What::Select, n, &[m, V(e, n), V(e, n)], V(e, n)));
        }
        for e in INTS {
            out.push(Func::new(What::CmpAny, n, &[V(e, n), V(e, n)], S(I(1))));
            if n >= 8 {
                out.push(Func::new(What::CmpCttz, n, &[V(e, n), V(e, n)], S(I(n))));
            }
        }
        for e in FLOATS {
            out.push(Func::new(What::CmpMin, n, &[V(e, n), V(e, n)], V(e, n)));
        }
    }
    out
}

fn reductions() -> Vec<Func> {
    let mut out = Vec::new();
    for n in [1, 2, 3, 4, 8, 16, 64] {
        for e in INTS {
            for op in REDUCE_INT {
                out.push(Func::new(What::Reduce(op), n, &[V(e, n)], S(e)));
            }
        }
        for e in FLOATS {
            for op in ["fmax", "fmin"] {
                out.push(Func::new(What::Reduce(op), n, &[V(e, n)], S(e)));
            }
            if e != Half {
                for op in ["fadd", "fmul"] {
                    out.push(Func::new(What::Reduce(op), n, &[V(e, n), S(e)], S(e)));
                }
            }
        }
    }
    out
}

fn all() -> Vec<Func> {
    [icmps(), fcmps(), mask_ops(), shuffles(), casts(), lane_ops(), reductions()].concat()
}

/// The pointer-form module (`value`: the value-form twins).
fn module_text(funcs: &[Func], major: u32, value: bool) -> String {
    let mut decls = Vec::new();
    let mut text = String::new();
    for (k, f) in funcs.iter().enumerate() {
        let body = f.body(&mut decls);
        if value {
            let params: Vec<String> =
                f.args.iter().enumerate().map(|(j, t)| format!("{} %x{j}", t.ll())).collect();
            writeln!(
                text,
                "define {} @g{k}({}) {{\nentry:\n{body}  ret {} %r\n}}\n",
                f.ret.ll(),
                params.join(", "),
                f.ret.ll()
            )
            .unwrap();
        } else {
            let loads: String = f
                .args
                .iter()
                .enumerate()
                .map(|(j, t)| format!("  %x{j} = load {}, ptr %a{j}\n", t.ll()))
                .collect();
            writeln!(
                text,
                "define void @f{k}(ptr %o, ptr %a0, ptr %a1, ptr %a2) {{\nentry:\n{loads}{body}  \
                 store {} %r, ptr %o\n  ret void\n}}\n",
                f.ret.ll()
            )
            .unwrap();
        }
    }
    format!("{}\n{}\n\n{text}", ll_header(major), decls.join("\n"))
}

// ---- the model ----

fn sext(x: u64, bits: u32) -> i64 {
    let s = 64 - bits;
    ((x << s) as i64) >> s
}

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

fn value(e: E, b: u64) -> f64 {
    match e {
        Half => h2d(b),
        Float => f64::from(f32::from_bits(b as u32)),
        _ => f64::from_bits(b),
    }
}

/// `x` (exactly representable) in `e`.
fn encode(e: E, x: f64) -> u64 {
    match e {
        Half => match x {
            0.0 => 0,
            1.0 => 0x3c00,
            -1.0 => 0xbc00,
            _ => unreachable!(),
        },
        Float => u64::from((x as f32).to_bits()),
        _ => x.to_bits(),
    }
}

fn icmp(p: &str, bits: u32, a: u64, b: u64) -> u64 {
    let (sa, sb) = (sext(a, bits), sext(b, bits));
    u64::from(match p {
        "eq" => a == b,
        "ne" => a != b,
        "ugt" => a > b,
        "uge" => a >= b,
        "ult" => a < b,
        "ule" => a <= b,
        "sgt" => sa > sb,
        "sge" => sa >= sb,
        "slt" => sa < sb,
        _ => sa <= sb,
    })
}

fn fcmp(p: &str, e: E, a: u64, b: u64) -> u64 {
    let (x, y) = (value(e, a), value(e, b));
    let uno = x.is_nan() || y.is_nan();
    u64::from(match p {
        "false" => false,
        "true" => true,
        "oeq" => !uno && x == y,
        "ogt" => !uno && x > y,
        "oge" => !uno && x >= y,
        "olt" => !uno && x < y,
        "ole" => !uno && x <= y,
        "one" => !uno && x != y,
        "ord" => !uno,
        "ueq" => uno || x == y,
        "ugt" => uno || x > y,
        "uge" => uno || x >= y,
        "ult" => uno || x < y,
        "ule" => uno || x <= y,
        "une" => uno || x != y,
        _ => uno,
    })
}

/// How a scalar float result is compared.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Loose {
    Exact,
    /// Any NaN matches any NaN.
    Nan,
    /// And `+0` matches `-0`.
    NanZero,
}

/// The model's result: the image, which of its bits are compared, and
/// how a float scalar is.
struct Out {
    t: T,
    want: Vec<u8>,
    care: Vec<u8>,
    loose: Loose,
}

impl Out {
    fn new(t: T) -> Out {
        let size = t.size();
        let mut care = vec![0; size];
        if let V(I(1), n) = t
            && n % 8 != 0
        {
            care[size - 1] = !((1u8 << (n % 8)) - 1);
        }
        Out { t, want: vec![0; size], care, loose: Loose::Exact }
    }

    fn bit(&mut self, k: u32, v: u64) {
        let (byte, at) = (k as usize / 8, k % 8);
        self.want[byte] |= ((v & 1) as u8) << at;
        self.care[byte] |= 1 << at;
    }

    /// Lane `i` is `v`.
    fn set(&mut self, i: u32, v: u64) {
        if self.t.is_mask() {
            self.bit(i, v);
            return;
        }
        let e = self.t.elem();
        for k in 0..e.bits() {
            self.bit(i * e.bits().next_multiple_of(8) + k, v >> k);
        }
    }

    /// A stored mask with a poison lane: memory is poison byte by byte,
    /// so no bit of the byte that holds it is compared (`llc` lets the
    /// bits of an out-of-range `fptoui` to `i1` spill into its
    /// neighbours).
    fn poison_bytes(&mut self) {
        if let V(I(1), n) = self.t {
            for i in 0..n {
                if !self.cared(i) {
                    self.care[i as usize / 8] = 0;
                }
            }
        }
    }

    /// Whether any bit of lane `i` is compared.
    fn cared(&self, i: u32) -> bool {
        if self.t.is_mask() {
            return self.care[i as usize / 8] >> (i % 8) & 1 == 1;
        }
        let w = self.t.elem().bytes();
        self.care[i as usize * w..][..w].iter().any(|&c| c != 0)
    }

    /// Whether `got` is the result.
    fn matches(&self, got: &[u8]) -> bool {
        if let S(e) = self.t
            && e.is_float()
            && self.loose != Loose::Exact
        {
            let (g, w) = (get(self.t, got, 0), get(self.t, &self.want, 0));
            let (x, y) = (value(e, g), value(e, w));
            return g == w
                || (x.is_nan() && y.is_nan())
                || (self.loose == Loose::NanZero && x == 0.0 && y == 0.0);
        }
        got.iter().zip(&self.want).zip(&self.care).all(|((g, w), c)| (g ^ w) & c == 0)
    }
}

/// A float fold of `e` lanes in `e`'s precision (not `half`).
fn fold(e: E, op: &str, acc: u64, x: u64) -> u64 {
    if e == Float {
        let (a, b) = (f32::from_bits(acc as u32), f32::from_bits(x as u32));
        u64::from(if op == "fadd" { a + b } else { a * b }.to_bits())
    } else {
        let (a, b) = (f64::from_bits(acc), f64::from_bits(x));
        if op == "fadd" { a + b } else { a * b }.to_bits()
    }
}

fn model(f: &Func, x: &[Vec<u8>]) -> Out {
    let n = f.n;
    let lane = |j: usize, i: u32| get(f.args[j], &x[j], i);
    let mut out = Out::new(f.ret);
    match &f.what {
        What::Icmp(p) | What::MaskIcmp(p) => {
            let bits = f.args[0].elem().bits();
            for i in 0..n {
                out.set(i, icmp(p, bits, lane(0, i), lane(1, i)));
            }
        }
        What::Fcmp(p) => {
            for i in 0..n {
                out.set(i, fcmp(p, f.args[0].elem(), lane(0, i), lane(1, i)));
            }
        }
        What::MaskBin(op) => {
            for i in 0..n {
                let (a, b) = (lane(0, i), lane(1, i));
                out.set(
                    i,
                    match *op {
                        "add" | "sub" | "xor" => a ^ b,
                        "mul" | "and" => a & b,
                        "or" => a | b,
                        _ => a,
                    },
                );
            }
        }
        What::Select => {
            for i in 0..f.ret.lanes() {
                out.set(i, if lane(0, i) == 1 { lane(1, i) } else { lane(2, i) });
            }
        }
        What::Cast(op) => {
            let (from, to) = (f.args[0].elem(), f.ret.elem());
            for i in 0..n {
                let a = lane(0, i);
                let v = match *op {
                    "zext" | "trunc" => Some(a & 1),
                    "sext" => Some(if a == 1 { low(to.bits()) } else { 0 }),
                    "uitofp" => Some(encode(to, a as f64)),
                    "sitofp" => Some(encode(to, if a == 1 { -1.0 } else { 0.0 })),
                    _ => {
                        let t = value(from, a).trunc();
                        let signed = *op == "fptosi";
                        if t == 0.0 {
                            Some(0)
                        } else if t == if signed { -1.0 } else { 1.0 } {
                            Some(1)
                        } else {
                            None
                        }
                    }
                };
                if let Some(v) = v {
                    out.set(i, v);
                }
            }
        }
        What::Extract(k) => {
            let at = k.unwrap_or_else(|| lane(1, 0) as u32);
            if at < n {
                out.set(0, lane(0, at));
            }
        }
        What::Insert(k) => {
            let at = k.unwrap_or_else(|| lane(2, 0) as u32);
            if at < n {
                for i in 0..n {
                    out.set(i, if i == at { lane(1, 0) & 1 } else { lane(0, i) });
                }
            } else {
                out.care.fill(0);
            }
        }
        What::Shuffle(sel) => {
            for (j, &s) in sel.iter().enumerate() {
                if s >= 0 {
                    let s = s as u32;
                    out.set(j as u32, if s < n { lane(0, s) } else { lane(1, s - n) });
                }
            }
        }
        What::Reduce(op) => {
            let e = f.args[0].elem();
            let lanes: Vec<u64> = (0..f.args[0].lanes()).map(|i| lane(0, i)).collect();
            if e.is_float() {
                let v = if matches!(*op, "fadd" | "fmul") {
                    out.loose = Loose::Nan;
                    lanes.iter().fold(lane(1, 0), |acc, &l| fold(e, op, acc, l))
                } else {
                    out.loose = Loose::NanZero;
                    let max = *op == "fmax";
                    lanes[1..].iter().fold(lanes[0], |acc, &l| {
                        let (a, b) = (value(e, acc), value(e, l));
                        if a.is_nan() || (!b.is_nan() && ((b > a) == max) && b != a) {
                            l
                        } else {
                            acc
                        }
                    })
                };
                out.set(0, v);
            } else {
                let bits = e.bits();
                let mut acc = lanes[0];
                for &l in &lanes[1..] {
                    acc = match *op {
                        "add" => acc.wrapping_add(l),
                        "mul" => acc.wrapping_mul(l),
                        "and" => acc & l,
                        "or" => acc | l,
                        "xor" => acc ^ l,
                        "smax" => {
                            if sext(l, bits) > sext(acc, bits) {
                                l
                            } else {
                                acc
                            }
                        }
                        "smin" => {
                            if sext(l, bits) < sext(acc, bits) {
                                l
                            } else {
                                acc
                            }
                        }
                        "umax" => acc.max(l),
                        _ => acc.min(l),
                    } & low(bits);
                }
                out.set(0, acc);
            }
        }
        What::Phi(_) => {
            let slt = |i| icmp("slt", 32, lane(0, i), lane(1, i));
            let eq = |i| icmp("eq", 32, lane(0, i), lane(1, i));
            let c = slt(0) == 1;
            for i in 0..n {
                out.set(i, if c { eq(i) } else { slt(i) });
            }
        }
        What::MaskTo => {
            for i in 0..n {
                out.bit(i, icmp("ult", 8, lane(0, i), lane(1, i)));
            }
            // A vector or a float image is all the mask's bits.
            if !matches!(f.ret, S(I(_))) {
                out.care.fill(0xff);
            }
        }
        What::MaskFrom => {
            for i in 0..n {
                out.set(i, u64::from(x[0][i as usize / 8] >> (i % 8) & 1) ^ pattern_bit(i));
            }
        }
        What::ConstPoison => {
            for i in 0..n {
                if i % 5 != 4 {
                    out.set(i, lane(0, i) | pattern_bit(i));
                }
            }
        }
        What::CmpSext => {
            let e = f.args[0].elem();
            for i in 0..n {
                let c = icmp("sgt", e.bits(), lane(0, i), lane(1, i));
                out.set(i, if c == 1 { low(e.bits()) } else { 0 });
            }
        }
        What::CmpMin => {
            let e = f.args[0].elem();
            for i in 0..n {
                let c = fcmp("olt", e, lane(0, i), lane(1, i));
                out.set(i, if c == 1 { lane(0, i) } else { lane(1, i) });
            }
        }
        What::CmpAny => {
            let bits = f.args[0].elem().bits();
            out.set(0, u64::from((0..n).any(|i| icmp("eq", bits, lane(0, i), lane(1, i)) == 1)));
        }
        What::CmpCttz => {
            let bits = f.args[0].elem().bits();
            let first = (0..n).find(|&i| icmp("ne", bits, lane(0, i), lane(1, i)) == 1);
            out.set(0, u64::from(first.unwrap_or(n)));
        }
    }
    out.poison_bytes();
    out
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

/// A float pattern of `e` that is not a signalling NaN.
fn quiet(e: E, b: u64) -> u64 {
    let (exp, frac, q) = match e {
        Half => (0x7c00, 0x3ff, 0x200),
        Float => (0x7f80_0000, 0x7f_ffff, 0x40_0000),
        _ => (0x7ff0_0000_0000_0000, 0xf_ffff_ffff_ffff, 0x8_0000_0000_0000),
    };
    if b & exp == exp && b & frac != 0 { b | q } else { b }
}

fn edges(e: E) -> Vec<u64> {
    match e {
        Half => vec![0, 0x8000, 0x3c00, 0xbc00, 0x7c00, 0xfc00, 0x7e00, 1, 0x7bff, 0x3800],
        Float => [0.0f32, -0.0, 1.0, -1.0, f32::INFINITY, -f32::INFINITY, f32::NAN, 0.5, f32::MAX]
            .iter()
            .map(|x| u64::from(x.to_bits()))
            .chain([1])
            .collect(),
        Double => [0.0f64, -0.0, 1.0, -1.0, f64::INFINITY, -f64::INFINITY, f64::NAN, 0.5, f64::MAX]
            .iter()
            .map(|x| x.to_bits())
            .chain([1])
            .collect(),
        _ => {
            let b = e.bits();
            let min = 1u64 << (b - 1);
            vec![0, 1, low(b), min, min - 1, min + 1, 2]
        }
    }
}

/// Floats around the range of `i1`, for `fptoui`/`fptosi`.
fn near_i1(e: E, rng: &mut Rng) -> u64 {
    let xs = [0.0, -0.0, 0.5, -0.5, 1.0, 1.5, -1.0, -1.5, 1.75, -1.75, 2.0, -2.0, 0.25];
    let k = rng.below(xs.len() as u64 + 1) as usize;
    let x: f64 = xs.get(k).copied().unwrap_or(f64::NAN);
    match e {
        Half => {
            // Each is a few bits of half.
            let s = if x.is_sign_negative() { 0x8000 } else { 0 };
            s | match x.abs() {
                0.0 => 0,
                0.25 => 0x3400,
                0.5 => 0x3800,
                1.0 => 0x3c00,
                1.5 => 0x3e00,
                1.75 => 0x3f00,
                2.0 => 0x4000,
                _ => 0x7e00,
            }
        }
        Float => u64::from((x as f32).to_bits()),
        _ => x.to_bits(),
    }
}

/// The image of operand `j` of `f`; `junk` sets the padding bits of
/// masks.
fn image(f: &Func, j: usize, prev: &[Vec<u8>], rng: &mut Rng, junk: bool) -> Vec<u8> {
    let t = f.args[j];
    let mut img = vec![0u8; t.size()];
    match t {
        V(I(1), n) => {
            for b in &mut img {
                *b = rng.next() as u8;
            }
            if n % 8 != 0 && !junk {
                *img.last_mut().unwrap() &= (1u8 << (n % 8)) - 1;
            }
            return img;
        }
        S(I(1)) => {
            img[0] = (rng.next() & 1) as u8;
            return img;
        }
        S(I(32)) if matches!(f.what, What::Extract(None) | What::Insert(None)) => {
            let at = rng.below(u64::from(f.n) + 2) as u32;
            img.copy_from_slice(&at.to_le_bytes());
            return img;
        }
        _ => {}
    }
    let e = t.elem();
    let w = e.bytes();
    let near = matches!(f.what, What::Cast("fptoui" | "fptosi"));
    let edges = edges(e);
    for i in 0..t.lanes() as usize {
        let v = if j > 0 && prev[0].len() == img.len() && rng.below(3) == 0 {
            get(t, &prev[0], i as u32)
        } else if near {
            near_i1(e, rng)
        } else if rng.below(2) == 0 {
            edges[rng.below(edges.len() as u64) as usize]
        } else {
            let r = rng.next() & low(e.bits());
            // Small integers too, so that lanes often compare equal.
            if !e.is_float() && rng.below(2) == 0 { r % 4 } else { r }
        };
        let v = if e.is_float() { quiet(e, v) } else { v };
        img[i * w..][..w].copy_from_slice(&v.to_le_bytes()[..w]);
    }
    img
}

struct Case {
    func: usize,
    images: Vec<Vec<u8>>,
}

fn cases(funcs: &[Func], seed: u64, junk: bool) -> Vec<Case> {
    let mut rng = Rng(seed);
    let mut out = Vec::new();
    for (func, f) in funcs.iter().enumerate() {
        for _ in 0..CASES {
            let mut images: Vec<Vec<u8>> = Vec::new();
            for j in 0..f.args.len() {
                let img = image(f, j, &images, &mut rng, junk);
                images.push(img);
            }
            out.push(Case { func, images });
        }
    }
    out
}

fn stdin(funcs: &[Func], cases: &[Case]) -> Vec<u8> {
    let mut out = Vec::new();
    for case in cases {
        let f = &funcs[case.func];
        let size = |j: usize| f.args.get(j).map_or(0, |t| t.size());
        for word in [case.func, size(0), size(1), size(2), f.ret.size()] {
            out.extend_from_slice(&(word as u32).to_le_bytes());
        }
        for img in &case.images {
            out.extend_from_slice(img);
        }
    }
    out
}

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
         static unsigned char in[3][{MAX_IMAGE}] __attribute__((aligned(64)));\n\
         static unsigned char out[{MAX_IMAGE} + {two}] __attribute__((aligned(64)));\n\
         static char line[2 * ({MAX_IMAGE} + {two}) + 1];\n\
         int main(void) {{\n  uint32_t h[5];\n  static const char hex[] = \"0123456789abcdef\";\n  \
         while (fread(h, 4, 5, stdin) == 5) {{\n    \
         if (h[0] >= sizeof funcs / sizeof funcs[0]) return 3;\n    \
         for (int j = 0; j < 3; j++) {{\n      \
         if (h[1 + j] > {MAX_IMAGE} || fread(in[j], 1, h[1 + j], stdin) != h[1 + j]) return 2;\n    }}\n    \
         if (h[4] > {MAX_IMAGE}) return 3;\n    \
         memset(out, {SENTINEL}, sizeof out);\n    \
         funcs[h[0]](out + {MARGIN}, in[0], in[1], in[2]);\n    \
         size_t n = h[4] + {two};\n    \
         for (size_t k = 0; k < n; k++) {{ line[2 * k] = hex[out[k] >> 4]; line[2 * k + 1] = hex[out[k] & 15]; }}\n    \
         line[2 * n] = '\\n';\n    fwrite(line, 1, 2 * n + 1, stdout);\n  }}\n  return 0;\n}}\n",
        two = 2 * MARGIN
    )
    .unwrap();
    c
}

// ---- the legs ----

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

fn describe(f: &Func, case: &Case) -> String {
    let images: Vec<String> = case.images.iter().map(|i| hex(i)).collect();
    format!("{:?} n={} {:?} -> {:?}: operands [{}]", f.what, f.n, f.args, f.ret, images.join(" "))
}

fn check_leg(funcs: &[Func], cases: &[Case], leg: &str, lines: &[Vec<u8>], report: &mut Report) {
    assert_eq!(lines.len(), cases.len(), "{leg}: one line per case");
    for (c, (case, got)) in cases.iter().zip(lines).enumerate() {
        let f = &funcs[case.func];
        let n = f.ret.size();
        let sentinels = got[..MARGIN].iter().chain(&got[MARGIN + n..]).all(|&b| b == SENTINEL);
        let out = model(f, &case.images);
        let body = &got[MARGIN..MARGIN + n];
        report.compared += 1;
        if !sentinels || !out.matches(body) {
            report.wrong.push(format!(
                "{leg}: case {c} f{} {}{}: want {} care {}, got {}",
                case.func,
                describe(f, case),
                if sentinels { "" } else { " (sentinels overwritten)" },
                hex(&out.want),
                hex(&out.care),
                hex(body),
            ));
        }
    }
}

/// `hod_interp` on the value-form twins.
fn interp_leg(funcs: &[Func], cases: &[Case], report: &mut Report) {
    let text = module_text(funcs, 22, true);
    let (module, bodies) = parse(&text);
    let program = Program::with_bodies(&module, &bodies).expect("the twins");
    for (c, case) in cases.iter().enumerate() {
        let f = &funcs[case.func];
        if !f.interp() {
            continue;
        }
        let sym = module.find_symbol(format!("g{}", case.func).as_bytes()).unwrap();
        let TypeKind::Function { params, .. } = module.types.kind(module.symbol(sym).ty()) else {
            unreachable!()
        };
        let args: Vec<Val> = f
            .args
            .iter()
            .zip(params)
            .zip(&case.images)
            .map(|((t, &ty), img)| match t {
                V(..) => Val::Agg(Vector::from_image(&module.types, ty, img).unwrap()),
                S(Float) => Val::Float(get(*t, img, 0) as u32),
                S(Double) => Val::Double(get(*t, img, 0)),
                S(e) => Val::int(e.bits(), u128::from(get(*t, img, 0))),
            })
            .collect();
        let out = model(f, &case.images);
        let result = Interp::new(&program).run(sym, &args).result;
        let (got, poison): (Vec<u8>, Vec<u32>) = match result {
            Ok(Some(Val::Agg(v))) => {
                let poison = (0..v.lane_count()).filter(|&i| v.is_poison(i)).collect();
                (v.bytes().to_vec(), poison)
            }
            Ok(Some(Val::Int { value, .. })) => {
                (value.to_le_bytes()[..f.ret.size()].to_vec(), Vec::new())
            }
            Ok(Some(Val::Float(b))) => (b.to_le_bytes().to_vec(), Vec::new()),
            Ok(Some(Val::Double(b))) => (b.to_le_bytes().to_vec(), Vec::new()),
            Err(Stop::Unsupported(reason)) => {
                report.wrong.push(format!("interp: f{} unsupported {reason}", case.func));
                continue;
            }
            // A poison scalar where the model compares nothing; a NaN
            // result (the interpreter stops: its payload is open).
            Err(Stop::Trap(_)) if out.care.iter().all(|&c| c == 0) => continue,
            Err(Stop::Trap(Trap::NanResult)) => continue,
            other => {
                report.wrong.push(format!("interp: case {c} {}: {other:?}", describe(f, case)));
                continue;
            }
        };
        report.compared += 1;
        let spurious = poison.iter().any(|&i| out.cared(i));
        if spurious || !out.matches(&got) {
            report.wrong.push(format!(
                "interp: case {c} f{} {}: want {} care {}, got {} poison {poison:?}",
                case.func,
                describe(f, case),
                hex(&out.want),
                hex(&out.care),
                hex(&got),
            ));
        }
    }
    report.legs.push("interp".into());
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

/// Runs `funcs` on every leg; `junk`: the hod legs only, masks with their
/// padding bits set.
fn run(name: &str, funcs: &[Func], seed: u64, junk: bool) -> Report {
    let text = module_text(funcs, 22, false);
    if let Ok(dir) = std::env::var("HOD_MASK_DUMP") {
        std::fs::write(format!("{dir}/{name}.ll"), &text).unwrap();
    }
    let (module, bodies) = parse(&text);
    let scratch = Scratch::new(&format!("a64-mask-{name}"));
    let mut legs: Vec<(String, PathBuf)> = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let options = Options { stress, ..Options::default() };
        let emitted =
            compile_module(&module, &bodies, &options).unwrap_or_else(|e| panic!("{mode}: {e}"));
        legs.push((
            format!("hod-{mode}"),
            write_object(&scratch, &format!("hod-{mode}.o"), &emitted),
        ));
    }
    if !junk {
        for major in [22, 19] {
            if let Some(object) = llc_object(&scratch, major, &module_text(funcs, major, false)) {
                legs.push((format!("llc{major}"), object));
            }
        }
    }
    let cases = cases(funcs, seed, junk);
    let input = stdin(funcs, &cases);
    let mut report = Report::default();
    if !junk {
        interp_leg(funcs, &cases, &mut report);
    }
    let Some(toolchain) = Toolchain::host(&scratch) else { return report };
    let driver = toolchain.compile_c(&scratch, "driver", &driver(funcs), &[]);
    for (leg, object) in &legs {
        let linkers =
            if leg == "hod-plain" { &toolchain.linkers[..] } else { &toolchain.linkers[..1] };
        for linker in linkers {
            let label = format!("{leg}/{}", linker.name);
            let program = scratch.join(label.replace('/', "-"));
            let inputs: [&Path; 2] = [&driver, object];
            toolchain.link_ok(linker, &["-Wl,--no-as-needed", "-lm"], &inputs, &program);
            let ran = run_with(&mut Command::new(&program), &input, Duration::from_mins(3));
            assert!(ran.success(), "{label}: {:?}\n{}", ran.status, ran.stderr_text());
            let lines: Vec<Vec<u8>> = ran.stdout_text().lines().map(parse_line).collect();
            check_leg(funcs, &cases, &label, &lines, &mut report);
            report.legs.push(label);
        }
    }
    report
}

fn check(name: &str, funcs: &[Func], seed: u64, junk: bool) {
    let report = run(name, funcs, seed, junk);
    eprintln!(
        "a64 mask {name}: {} functions, {} comparisons over legs {:?}",
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

// ---- tests ----

/// `icmp` of integer and pointer lanes, every predicate.
#[test]
fn integer_compares_agree() {
    check("icmp", &icmps(), 0x0810_0001, false);
}

/// `fcmp` of `half`, `float` and `double` lanes, every predicate.
#[test]
fn float_compares_agree() {
    check("fcmp", &fcmps(), 0x0810_0002, false);
}

/// Operations on masks: arithmetic, logic, `icmp`, `select`, `phi`,
/// element access, reductions, bitcasts, poison lanes.
#[test]
fn mask_operations_agree() {
    check("ops", &mask_ops(), 0x0810_0003, false);
}

/// Shuffles of masks, poison lanes and lengths that change included.
#[test]
fn mask_shuffles_agree() {
    check("shuffle", &shuffles(), 0x0810_0004, false);
}

/// The casts between masks and lanes.
#[test]
fn mask_casts_agree() {
    check("cast", &casts(), 0x0810_0005, false);
}

/// `select` of lanes on a mask, and the chains of libc++'s vector paths.
#[test]
fn selects_and_chains_agree() {
    check("lanes", &lane_ops(), 0x0810_0006, false);
}

/// `llvm.vector.reduce.*` of integer and float lanes.
#[test]
fn lane_reductions_agree() {
    check("reduce", &reductions(), 0x0810_0007, false);
}

/// Loaded masks whose padding bits are set: hod ignores them, and every
/// stored mask has its padding zero.
#[test]
fn the_padding_of_loaded_masks_is_ignored() {
    let funcs: Vec<Func> = [mask_ops(), shuffles(), casts(), lane_ops()]
        .concat()
        .into_iter()
        .filter(|f| f.args.iter().any(|t| t.is_mask()) && f.n % 8 != 0)
        .collect();
    check("junk", &funcs, 0x0810_0008, true);
}

/// The generator itself (no tools needed): every function parses, and
/// the model's padding rule is what the interpreter's images are.
#[test]
fn the_generator_parses() {
    let funcs = all();
    let (_, bodies) = parse(&module_text(&funcs, 22, false));
    assert_eq!(bodies.len(), funcs.len());
    let (_, bodies) = parse(&module_text(&funcs, 22, true));
    assert_eq!(bodies.len(), funcs.len());
    let out = Out::new(V(I(1), 3));
    assert_eq!(out.care, [0xf8]);
    assert_eq!(get(V(I(1), 13), &[0, 0x10], 12), 1);
}

/// What a mask operation does not handle is refused by its name, one
/// construct per function.
#[test]
fn what_is_refused_is_refused_by_name() {
    let one = |t: &str, body: &str| {
        format!("define void @f(ptr %p) {{\n  %x = load {t}, ptr %p\n  {body}\n  ret void\n}}\n")
    };
    let cases = [
        (
            one("<65 x i8>", "%m = icmp eq <65 x i8> %x, %x\n  store <65 x i1> %m, ptr %p"),
            vector::MASK_WIDE,
        ),
        (
            one("<128 x i1>", "%m = and <128 x i1> %x, %x\n  store <128 x i1> %m, ptr %p"),
            vector::MASK_WIDE,
        ),
        (
            one("<4 x bfloat>", "%m = fcmp olt <4 x bfloat> %x, %x\n  store <4 x i1> %m, ptr %p"),
            vector::BFLOAT,
        ),
        (
            "declare <4 x i1> @llvm.umax.v4i1(<4 x i1>, <4 x i1>)\ndefine void @f(ptr %p) {\n  \
             %x = load <4 x i1>, ptr %p\n  %m = call <4 x i1> @llvm.umax.v4i1(<4 x i1> %x, <4 x i1> %x)\n  \
             store <4 x i1> %m, ptr %p\n  ret void\n}\n"
                .into(),
            intrinsic::VECTOR,
        ),
        (
            "define <4 x i1> @f(<4 x i1> %m) {\n  ret <4 x i1> %m\n}\n".into(),
            vector::CALL,
        ),
        (
            "declare void @g(<8 x i1>)\ndefine void @f(ptr %p) {\n  %m = load <8 x i1>, ptr %p\n  \
             call void @g(<8 x i1> %m)\n  ret void\n}\n"
                .into(),
            vector::CALL,
        ),
        (
            "declare <16 x i1> @g()\ndefine void @f(ptr %p) {\n  %m = call <16 x i1> @g()\n  \
             store <16 x i1> %m, ptr %p\n  ret void\n}\n"
                .into(),
            vector::CALL,
        ),
    ];
    let cases: Vec<(&str, &'static str)> = cases.iter().map(|(f, r)| (f.as_str(), *r)).collect();
    assert_refused(&cases);
    assert_eq!(mask::REDUCE, "a64:vector-reduce");
}

/// The widest masks and the longest lane vectors of every family compile,
/// plain and under every stress option.
#[test]
fn the_widest_masks_compile() {
    let mut funcs: Vec<Func> = all().into_iter().filter(|f| f.n == 64).collect();
    for op in REDUCE_INT {
        funcs.push(Func::new(What::Reduce(op), 256, &[V(I(8), 256)], S(I(8))));
    }
    funcs.push(Func::new(What::Reduce("fadd"), 256, &[V(Double, 256), S(Double)], S(Double)));
    funcs.push(Func::new(What::Reduce("fmax"), 256, &[V(Half, 256)], S(Half)));
    let (m, bodies) = parse(&module_text(&funcs, 22, false));
    for stress in [Stress::default(), Stress::ALL] {
        let options = Options { stress, ..Options::default() };
        chunk(&m, &bodies, &options).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// The emitter's cost is linear in the lane count: the code of one lane
/// is the same number of bytes at every lane count (the bytes are
/// compared; the time is reported, not asserted).
#[test]
fn the_code_is_linear_in_the_lanes() {
    let families: [(&str, fn(u32) -> Func); 5] = [
        ("icmp", |n| Func::new(What::Icmp("slt"), n, &[V(I(32), n), V(I(32), n)], V(I(1), n))),
        ("fcmp", |n| Func::new(What::Fcmp("ueq"), n, &[V(Double, n), V(Double, n)], V(I(1), n))),
        ("select", |n| {
            Func::new(What::Select, n, &[V(I(1), n), V(I(16), n), V(I(16), n)], V(I(16), n))
        }),
        ("sext", |n| Func::new(What::Cast("sext"), n, &[V(I(1), n)], V(I(64), n))),
        ("reduce", |n| Func::new(What::Reduce("umax"), n, &[V(I(16), n)], S(I(16)))),
    ];
    for (name, make) in families {
        let mut sizes = Vec::new();
        for n in [16u32, 32, 64] {
            let f = make(n);
            let (m, bodies) = parse(&module_text(std::slice::from_ref(&f), 22, false));
            let start = Instant::now();
            let code = chunk(&m, &bodies, &Options::default()).unwrap().code.len();
            let ns = start.elapsed().as_nanos() / u128::from(n);
            eprintln!("a64 mask cost: {name} {n} lanes: {code} bytes, {ns} ns per lane");
            sizes.push((n, code));
        }
        let per = |k: usize| (sizes[k + 1].1 - sizes[k].1) / (sizes[k + 1].0 - sizes[k].0) as usize;
        for k in 1..sizes.len() - 1 {
            assert_eq!(per(k), per(0), "{name}: bytes per lane {sizes:?}");
        }
    }
}
