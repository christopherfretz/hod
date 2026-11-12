//! The floating-point and vector group (WP-039, `spec/codegen.md` §5.5.1
//! "Floating point (WP-039)"): instructions on `Fpr`-class values.
//!
//! The evidence is native: every function below is compiled by hod twice
//! (plainly and with every stress option, `hod_cg::Stress::ALL`) and by
//! `llc -O0 -global-isel=0 -fast-isel=0`, the three are linked with one C
//! driver that calls each with every argument set and prints the result's
//! bit pattern, and the bits are compared: hod with `llc`, hod stressed
//! with hod plain, and, for the functions on `float`, `double` and
//! integers, hod with `hod_interp` (`NanPolicy::Host`: NaN payloads
//! included, `spec/interp.md` §5.1). Integer arguments arrive with junk
//! above their width (§2.2), so every extension the lowering needs is
//! exercised. Results that LLVM makes poison (an out-of-range `fptoui` /
//! `fptosi`) are not compared.

#![allow(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    reason = "a generator of test functions: operands are a, b, x, y; tables are long"
)]

mod common;

use std::fmt::Write as _;

use common::linking::{Toolchain, exit_code};
use common::{assert_refused, compile_ll, emit, ll_header, llc, parse, write_object};
use hod_a64::float;
use hod_cg::{CgError, Options, Stress};
use hod_interp::{Config, Interp, NanPolicy, Program, Val};

// ---- refusals ----

#[test]
fn what_the_group_does_not_lower_is_refused_by_name() {
    assert_refused(&[
        (
            "define <4 x i32> @f(<4 x i32> %a) { %c = icmp eq <4 x i32> %a, %a\n %r = sext <4 x i1> %c to <4 x i32>\n ret <4 x i32> %r }",
            float::VECTOR_ICMP,
        ),
        (
            "define <4 x i32> @f(<4 x i32> %a, <4 x i32> %b) { %r = sdiv <4 x i32> %a, %b\n ret <4 x i32> %r }",
            float::VECTOR_OP,
        ),
        (
            "define <2 x i64> @f(<2 x i64> %a, <2 x i64> %b) { %r = mul <2 x i64> %a, %b\n ret <2 x i64> %r }",
            float::VECTOR_OP,
        ),
        (
            "define <4 x float> @f(<4 x float> %a, <4 x float> %b) { %r = frem <4 x float> %a, %b\n ret <4 x float> %r }",
            float::VECTOR_OP,
        ),
        (
            "define <4 x half> @f(<4 x half> %a, <4 x half> %b) { %r = fadd <4 x half> %a, %b\n ret <4 x half> %r }",
            float::VECTOR_OP,
        ),
        (
            "define i1 @f(<2 x double> %a, <2 x double> %b) { %c = fcmp olt <2 x double> %a, %b\n %e = extractelement <2 x i1> %c, i64 0\n ret i1 %e }",
            float::VECTOR_FCMP,
        ),
        (
            "define <2 x double> @f(<2 x double> %a, <2 x double> %b) { %r = select <2 x i1> <i1 true, i1 false>, <2 x double> %a, <2 x double> %b\n ret <2 x double> %r }",
            float::VECTOR_SELECT,
        ),
        (
            "define half @f(half %a, half %b) { %r = frem half %a, %b\n ret half %r }",
            float::HALF_FREM,
        ),
        (
            "define fp128 @f(fp128 %a, fp128 %b) { %r = frem fp128 %a, %b\n ret fp128 %r }",
            float::FP128_LIBCALL,
        ),
        (
            "define i1 @f(fp128 %a, fp128 %b) { %r = fcmp olt fp128 %a, %b\n ret i1 %r }",
            float::FP128_LIBCALL,
        ),
        (
            "define double @f(fp128 %a) { %r = fptrunc fp128 %a to double\n ret double %r }",
            float::FP128_LIBCALL,
        ),
        (
            "define fp128 @f(i64 %a) { %r = sitofp i64 %a to fp128\n ret fp128 %r }",
            float::FP128_LIBCALL,
        ),
        (
            "define i32 @f(fp128 %a) { %r = fptosi fp128 %a to i32\n ret i32 %r }",
            float::FP128_LIBCALL,
        ),
        (
            "define bfloat @f(bfloat %a, bfloat %b) { %r = fadd bfloat %a, %b\n ret bfloat %r }",
            float::BFLOAT,
        ),
        (
            "define float @f(bfloat %a) { %r = fpext bfloat %a to float\n ret float %r }",
            float::BFLOAT,
        ),
        (
            "define i1 @f(bfloat %a, bfloat %b) { %r = fcmp oeq bfloat %a, %b\n ret i1 %r }",
            float::BFLOAT,
        ),
        (
            "define <2 x i32> @f(<2 x double> %a) { %r = fptosi <2 x double> %a to <2 x i32>\n ret <2 x i32> %r }",
            float::CAST,
        ),
    ]);
}

/// Fast-math flags license nothing hod uses: `hod_ir` keeps none, and the
/// code is the code without them.
#[test]
fn fast_math_flags_change_nothing() {
    let with = "define double @f(double %a, double %b) { %r = fadd fast double %a, %b\n %s = fmul nnan ninf nsz double %r, %a\n %c = fcmp fast olt double %s, %b\n %t = select nnan i1 %c, double %s, double %b\n ret double %t }";
    let without = "define double @f(double %a, double %b) { %r = fadd double %a, %b\n %s = fmul double %r, %a\n %c = fcmp olt double %s, %b\n %t = select i1 %c, double %s, double %b\n ret double %t }";
    let words = |text: &str| {
        let (m, bodies) = parse(&format!("{}{text}", ll_header(22)));
        common::code_words(&m, &bodies, &Options::default())
    };
    assert_eq!(words(with), words(without));
    assert_eq!(compile_ll(with), Ok(()));
}

/// §4.6: an `fcmp` whose only use is the next `select` is fused: one
/// `fcmp`, one `fcsel`, no `cset`.
#[test]
fn a_compare_into_a_select_is_fused() {
    let text = "define double @f(double %a, double %b, double %x, double %y) { %c = fcmp ogt double %a, %b\n %r = select i1 %c, double %x, double %y\n ret double %r }";
    let (m, bodies) = parse(&format!("{}{text}", ll_header(22)));
    let words = common::code_words(&m, &bodies, &Options::default());
    let is = |mask: u32, value: u32| words.iter().filter(|&&w| w & mask == value).count();
    assert_eq!(is(0xffe0_fc1f, 0x1e60_2000), 1, "one fcmp d: {words:08x?}");
    assert_eq!(is(0xffe0_0c00, 0x1e60_0c00), 1, "one fcsel d");
    assert_eq!(is(0xffff_0fe0, 0x1a9f_07e0), 0, "no cset");
}

// ---- types and values ----

/// The element of a vector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum El {
    I(u32),
    H,
    F,
    D,
}

impl El {
    fn bits(self) -> u32 {
        match self {
            El::I(w) => w,
            El::H => 16,
            El::F => 32,
            El::D => 64,
        }
    }

    fn ll(self) -> String {
        match self {
            El::I(w) => format!("i{w}"),
            El::H => "half".into(),
            El::F => "float".into(),
            El::D => "double".into(),
        }
    }
}

/// A type the tests pass, return or compute in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    H,
    F,
    D,
    Q,
    I(u32),
    V(u32, El),
}

use Ty::{D, F, H, I, Q};

impl Ty {
    fn ll(self) -> String {
        match self {
            H => "half".into(),
            F => "float".into(),
            D => "double".into(),
            Q => "fp128".into(),
            I(w) => format!("i{w}"),
            Ty::V(n, e) => format!("<{n} x {}>", e.ll()),
        }
    }

    fn bits(self) -> u32 {
        match self {
            H => 16,
            F => 32,
            D => 64,
            Q => 128,
            I(w) => w,
            Ty::V(n, e) => n * e.bits(),
        }
    }

    fn mask(self) -> u128 {
        if self.bits() >= 128 { u128::MAX } else { (1u128 << self.bits()) - 1 }
    }

    /// The C type it travels as (AAPCS64 places a short vector in a
    /// vector register whatever its elements).
    fn c(self) -> &'static str {
        match self {
            H => "_Float16",
            F => "float",
            D => "double",
            Q => "long double",
            I(w) if w <= 32 => "uint32_t",
            I(_) => "uint64_t",
            Ty::V(..) if self.bits() == 64 => "d_t",
            Ty::V(..) => "q_t",
        }
    }

    /// The C expression of an argument of bit pattern `bits`, with `junk`
    /// above an integer's width.
    fn c_arg(self, bits: u128, junk: u64) -> String {
        let lo = bits as u64;
        let hi = (bits >> 64) as u64;
        match self {
            H => format!("H(0x{lo:x})"),
            F => format!("F(0x{lo:x}u)"),
            D => format!("D(0x{lo:x}ull)"),
            Q => format!("Q(0x{lo:x}ull, 0x{hi:x}ull)"),
            I(w) => {
                let carrier = if w <= 32 { 32 } else { 64 };
                let junked =
                    if w < carrier { lo | (junk << w) & (u64::MAX >> (64 - carrier)) } else { lo };
                format!("(({})0x{junked:x}ull)", self.c())
            }
            Ty::V(..) if self.bits() == 64 => format!("VD(0x{lo:x}ull)"),
            Ty::V(..) => format!("VQ(0x{lo:x}ull, 0x{hi:x}ull)"),
        }
    }

    /// The C printer of a result.
    fn c_print(self) -> &'static str {
        match self {
            H => "P_h",
            F => "P_f",
            D => "P_d",
            Q => "P_q",
            I(w) if w <= 32 => "P_w",
            I(_) => "P_x",
            Ty::V(..) if self.bits() == 64 => "P_vd",
            Ty::V(..) => "P_vq",
        }
    }

    fn interp_ok(self) -> bool {
        matches!(self, F | D | I(_))
    }

    fn val(self, bits: u128) -> Val {
        match self {
            F => Val::Float(bits as u32),
            D => Val::Double(bits as u64),
            I(w) => Val::int(w, bits & self.mask()),
            _ => unreachable!("{self:?} is not an interpreter type"),
        }
    }
}

fn val_bits(v: &Val) -> u128 {
    match v {
        Val::Float(b) => u128::from(*b),
        Val::Double(b) => u128::from(*b),
        Val::Int { value, .. } => *value,
        other => panic!("result {other:?}"),
    }
}

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

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }
}

/// Whether `bits` of floating-point type `ty` (or each lane of a vector
/// of floats) is a NaN.
fn is_nan(ty: Ty, bits: u128) -> bool {
    let (exp_bits, frac_bits) = match ty {
        H => (5, 10),
        F => (8, 23),
        D => (11, 52),
        Q => (15, 112),
        _ => return false,
    };
    let exp = (bits >> frac_bits) & ((1 << exp_bits) - 1);
    exp == (1 << exp_bits) - 1 && bits & ((1u128 << frac_bits) - 1) != 0
}

/// Whether two results differ only in NaNs: both NaN, lane by lane
/// (`LangRef` "Behavior of Floating-Point NaN values" leaves a computed
/// NaN's sign and payload open; `spec/codegen.md` §5.5.1).
fn same_but_nan(ty: Ty, a: u128, b: u128) -> bool {
    let (lanes, lane) = match ty {
        Ty::V(n, El::H) => (n, H),
        Ty::V(n, El::F) => (n, F),
        Ty::V(n, El::D) => (n, D),
        Ty::V(..) | I(_) => return false,
        _ => (1, ty),
    };
    let width = lane.bits();
    (0..lanes).all(|k| {
        let (x, y) = ((a >> (k * width)) & lane.mask(), (b >> (k * width)) & lane.mask());
        x == y || (is_nan(lane, x) && is_nan(lane, y))
    })
}

/// The value of a `half` pattern.
fn half_value(bits: u128) -> f64 {
    let b = bits as u32;
    let sign = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (b >> 10) & 0x1f;
    let frac = f64::from(b & 0x3ff);
    sign * match exp {
        0 => frac * 2f64.powi(-24),
        31 => {
            if frac == 0.0 {
                f64::INFINITY
            } else {
                f64::NAN
            }
        }
        _ => (1.0 + frac / 1024.0) * 2f64.powi(exp as i32 - 15),
    }
}

/// The special values of a floating-point type: zeros, ones, extremes,
/// denormals, infinities, quiet and signalling NaNs with payloads and
/// signs, integer boundaries.
fn specials(ty: Ty) -> Vec<u128> {
    let v: Vec<u128> = match ty {
        H => vec![
            0, 0x8000, 0x3c00, 0xbc00, 0x3e00, 0x3800, 0xb800, 0x7bff, 0xfbff, 0x0400, 0x0001,
            0x03ff, 0x8001, 0x7c00, 0xfc00, 0x7e00, 0x7c01, 0x7e55, 0xfe01, 0x5640, 0xc7c0, 0x4200,
            0x5bff, 0x7800,
        ],
        F => [
            0u32,
            0x8000_0000,
            0x3f80_0000,
            0xbf80_0000,
            0x3fc0_0000,
            0x3f00_0000,
            0xbf00_0000,
            0x7f7f_ffff,
            0xff7f_ffff,
            0x0080_0000,
            0x0000_0001,
            0x007f_ffff,
            0x8000_0001,
            0x7f80_0000,
            0xff80_0000,
            0x7fc0_0000,
            0x7f80_0001,
            0x7fc1_2345,
            0xffc0_0001,
            0x7fa0_0000,
            0x4f00_0000,
            0x4f80_0000,
            0xcf00_0000,
            0x5f00_0000,
            0x5f80_0000,
            0xdf00_0000,
            0x4040_0000,
            0x42c8_8000,
            0xc0f8_0000,
            0x4b80_0001,
            0x3f7f_ffff,
        ]
        .into_iter()
        .map(u128::from)
        .collect(),
        D => [
            0u64,
            0x8000_0000_0000_0000,
            0x3ff0_0000_0000_0000,
            0xbff0_0000_0000_0000,
            0x3ff8_0000_0000_0000,
            0x3fe0_0000_0000_0000,
            0xbfe0_0000_0000_0000,
            0x7fef_ffff_ffff_ffff,
            0xffef_ffff_ffff_ffff,
            0x0010_0000_0000_0000,
            0x0000_0000_0000_0001,
            0x000f_ffff_ffff_ffff,
            0x8000_0000_0000_0001,
            0x7ff0_0000_0000_0000,
            0xfff0_0000_0000_0000,
            0x7ff8_0000_0000_0000,
            0x7ff0_0000_0000_0001,
            0x7ff8_dead_beef_0123,
            0xfff8_0000_0000_0001,
            0x7ff4_0000_0000_0000,
            0x41e0_0000_0000_0000,
            0x41f0_0000_0000_0000,
            0xc1e0_0000_0000_0000,
            0x43e0_0000_0000_0000,
            0x43f0_0000_0000_0000,
            0xc3e0_0000_0000_0000,
            0x4008_0000_0000_0000,
            0x4059_1000_0000_0000,
            0xc01f_0000_0000_0000,
            0x4340_0000_0000_0001,
            0x3fef_ffff_ffff_ffff,
            0x47ef_ffff_e000_0000,
            0x3810_0000_0000_0000,
            0x36a0_0000_0000_0000,
        ]
        .into_iter()
        .map(u128::from)
        .collect(),
        Q => vec![
            0,
            1 << 127,
            0x3fff_u128 << 112,
            0xbfff_u128 << 112,
            0x3fff_8000_u128 << 96,
            0x3ffb_9999_9999_9999_9999_9999_9999_999a,
            0x7ffe_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            0x0001_u128 << 112,
            1,
            0x7fff_u128 << 112,
            0xffff_u128 << 112,
            0x7fff_8000_u128 << 96,
            (0x7fff_u128 << 112) | 1,
            (0x7fff_8000_u128 << 96) | 0x1234_5678,
            0x4000_921f_b544_42d1_8469_898c_c517_01b8,
        ],
        _ => panic!("no specials for {ty:?}"),
    };
    v
}

/// The specials compared pairwise: a NaN of each kind, both zeros and
/// infinities, denormals, ordinary numbers, the extremes.
fn pair_specials(ty: Ty) -> Vec<u128> {
    let all = specials(ty);
    let pick: &[usize] = match ty {
        H | F | D => &[0, 1, 2, 3, 4, 7, 9, 10, 12, 13, 14, 15, 16, 18],
        _ => return all,
    };
    pick.iter().map(|&i| all[i]).collect()
}

/// A random value of `ty`: a raw pattern, or an ordinary number.
fn random(ty: Ty, rng: &mut Rng) -> u128 {
    let raw = rng.next();
    let ordinary = rng.below(3) != 0;
    match ty {
        H if ordinary => u128::from(raw & 0x83ff) | (u128::from(rng.below(16) as u32 + 8) << 10),
        H => u128::from(raw as u16),
        F if ordinary => {
            u128::from((raw as u32 & 0x807f_ffff) | ((rng.below(60) as u32 + 97) << 23))
        }
        F => u128::from(raw as u32),
        D if ordinary => u128::from((raw & 0x800f_ffff_ffff_ffff) | ((rng.below(120) + 963) << 52)),
        D => u128::from(raw),
        Q if ordinary => {
            (u128::from(raw) | (u128::from(rng.next()) << 64)) & !(0x7fff_u128 << 112)
                | (u128::from(rng.below(200) + 16283) << 112)
        }
        Q => u128::from(raw) | (u128::from(rng.next()) << 64),
        I(_) => {
            let edges = [0u128, 1, ty.mask(), ty.mask() >> 1, (ty.mask() >> 1) + 1, 2, 3];
            if rng.below(3) == 0 {
                rng.pick(&edges) & ty.mask()
            } else {
                let shift = rng.below(u64::from(ty.bits())) as u32;
                (u128::from(raw) | (u128::from(rng.next()) << 64)) & (ty.mask() >> shift)
            }
        }
        Ty::V(n, e) => {
            let mut out = 0u128;
            for lane in 0..n {
                let scalar = match e {
                    El::F => random(F, rng),
                    El::D => random(D, rng),
                    El::H => random(H, rng),
                    El::I(w) => random(I(w), rng),
                };
                out |= scalar << (lane * e.bits());
            }
            out
        }
    }
}

/// A special or a random value.
fn value(ty: Ty, rng: &mut Rng) -> u128 {
    match ty {
        H | F | D | Q if rng.below(2) == 0 => rng.pick(&specials(ty)),
        _ => random(ty, rng),
    }
}

// ---- the functions ----

/// How the arguments of a function are made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Gen {
    /// Specials and random values.
    Any,
    /// Every pair of the specials, then random ones.
    Pairs,
    /// Lanes of shift amounts below the element width.
    Shift,
    /// Floating-point lanes of small numbers (quarters), or integer
    /// lanes of small integers.
    Small,
    /// As `Small`, not negative.
    SmallPos,
}

/// Which results are poison.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Poison {
    Never,
    /// `fptoui`/`fptosi` from the first argument's type to `i<bits>`.
    ToInt {
        from: Ty,
        bits: u32,
        signed: bool,
    },
}

struct Func {
    name: String,
    ret: Ty,
    params: Vec<Ty>,
    /// The body, naming the parameters `%a0`, `%a1`, ...
    body: String,
    make: Gen,
    poison: Poison,
    /// Whether `llc`'s result is comparable: not for `freeze undef`,
    /// which may be any value.
    llc: bool,
}

impl Func {
    fn new(name: String, ret: Ty, params: &[Ty], body: String) -> Func {
        Func {
            name,
            ret,
            params: params.to_vec(),
            body,
            make: Gen::Any,
            poison: Poison::Never,
            llc: true,
        }
    }

    fn with(mut self, make: Gen) -> Func {
        self.make = make;
        self
    }

    fn text(&self, prefix: &str) -> String {
        let params: Vec<String> =
            self.params.iter().enumerate().map(|(i, t)| format!("{} %a{i}", t.ll())).collect();
        format!(
            "define {} @{prefix}{}({}) {{\n{}\n}}\n",
            self.ret.ll(),
            self.name,
            params.join(", "),
            self.body
        )
    }

    fn interp_ok(&self) -> bool {
        self.ret.interp_ok()
            && self.params.iter().all(|t| t.interp_ok())
            && !["half", "bfloat", "fp128", "<"].iter().any(|t| self.body.contains(t))
    }

    fn poisoned(&self, args: &[u128]) -> bool {
        match self.poison {
            Poison::Never => false,
            Poison::ToInt { from, bits, signed } => {
                let x = match from {
                    H => half_value(args[0]),
                    F => f64::from(f32::from_bits(args[0] as u32)),
                    D => f64::from_bits(args[0] as u64),
                    _ => unreachable!(),
                };
                hod_interp::ops::float_to_int(x, bits, signed).is_err()
            }
        }
    }

    fn arg_sets(&self, rng: &mut Rng, count: usize) -> Vec<Vec<u128>> {
        let mut out = Vec::new();
        if self.make == Gen::Pairs {
            let s = pair_specials(self.params[0]);
            for &a in &s {
                for &b in &s {
                    let mut set = vec![a, b];
                    set.extend(self.params[2..].iter().map(|&t| random(t, rng)));
                    out.push(set);
                }
            }
        }
        for _ in 0..count {
            let set = self
                .params
                .iter()
                .enumerate()
                .map(|(i, &t)| match (self.make, t) {
                    (Gen::Shift, Ty::V(n, e)) if i == 1 => {
                        let mut v = 0u128;
                        for lane in 0..n {
                            v |= u128::from(rng.below(u64::from(e.bits()))) << (lane * e.bits());
                        }
                        v
                    }
                    (Gen::Small | Gen::SmallPos, Ty::V(n, e)) => {
                        let mut v = 0u128;
                        let low = if self.make == Gen::SmallPos { 0.0 } else { 500.0 };
                        for lane in 0..n {
                            let k = rng.below(4001) as f32 / 4.0 - low;
                            let x = match e {
                                El::F => u128::from(k.to_bits()),
                                El::D => u128::from(f64::from(k).to_bits()),
                                El::I(w) => {
                                    // Small integers, sign-extended to the lane.
                                    (rng.below(2001) as i64 - 1000) as u128 & ((1u128 << w) - 1)
                                }
                                El::H => unreachable!("no small half lanes"),
                            };
                            v |= x << (lane * e.bits());
                        }
                        v
                    }
                    _ => value(t, rng),
                })
                .collect();
            out.push(set);
        }
        out
    }
}

const PREDICATES: [&str; 16] = [
    "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "uno", "ueq", "ugt", "uge", "ult",
    "ule", "une", "true",
];

const WIDTHS: [u32; 12] = [1, 7, 8, 16, 17, 31, 32, 33, 53, 63, 64, 24];

fn n(name: &str) -> String {
    name.replace(['<', '>', ' '], "")
}

/// Every hand-written function of the group.
fn hand_written() -> Vec<Func> {
    let mut out = Vec::new();
    let mut add = |f: Func| out.push(f);
    let scalars = [H, F, D];
    // Binary operations, with two arguments and with constants.
    for t in [H, F, D, Q] {
        for op in ["fadd", "fsub", "fmul", "fdiv", "frem"] {
            if op == "frem" && matches!(t, H | Q) {
                continue;
            }
            let tl = t.ll();
            let make = if t == Q { Gen::Any } else { Gen::Pairs };
            add(Func::new(
                format!("{op}_{tl}"),
                t,
                &[t, t],
                format!("%r = {op} {tl} %a0, %a1\nret {tl} %r"),
            )
            .with(make));
        }
    }
    let constants: &[(Ty, &[&str])] = &[
        (H, &["0xH0000", "0xH8000", "0xH3E00", "0xH0001", "0xH7E00", "0xH7C00", "0xHC7C0"]),
        (
            F,
            &[
                "0.0",
                "-0.0",
                "1.5",
                "0x36A0000000000000",
                "0x7FF8000000000000",
                "0x7FF0000000000000",
                "-7.75",
                "0x3FB99999A0000000",
            ],
        ),
        (
            D,
            &[
                "0.0",
                "-0.0",
                "1.5",
                "0x0000000000000001",
                "0x7FF8000000000000",
                "0xFFF0000000000000",
                "0.1",
                "1.0e300",
            ],
        ),
        (Q, &["0xL00000000000000003FFF800000000000", "0xL00000000000000000000000000000000"]),
    ];
    for &(t, list) in constants {
        let tl = t.ll();
        for (k, c) in list.iter().enumerate() {
            for op in ["fadd", "fmul", "fdiv"] {
                add(Func::new(
                    format!("{op}c{k}_{tl}"),
                    t,
                    &[t],
                    format!("%r = {op} {tl} %a0, {c}\nret {tl} %r"),
                ));
            }
            add(Func::new(
                format!("fsubc{k}_{tl}"),
                t,
                &[t],
                format!("%r = fsub {tl} {c}, %a0\nret {tl} %r"),
            ));
            add(Func::new(format!("retc{k}_{tl}"), t, &[], format!("ret {tl} {c}")));
        }
    }
    for t in [H, F, D, Q] {
        let tl = t.ll();
        add(Func::new(format!("fneg_{tl}"), t, &[t], format!("%r = fneg {tl} %a0\nret {tl} %r")));
        add(Func::new(format!("frz_{tl}"), t, &[t], format!("%r = freeze {tl} %a0\nret {tl} %r")));
        add(Func::new(
            format!("sel_{tl}"),
            t,
            &[I(1), t, t],
            format!("%r = select i1 %a0, {tl} %a1, {tl} %a2\nret {tl} %r"),
        ));
        // Multiple uses: homes, and the operands of one instruction twice.
        add(Func::new(
            format!("twice_{tl}"),
            t,
            &[t, t],
            format!(
                "%r = fadd {tl} %a0, %a0\n%s = fneg {tl} %a0\n%u = fmul {tl} %r, %s\n%v = fsub {tl} %u, %a1\nret {tl} %v"
            ),
        ));
    }
    let mut f = Func::new(
        "frzundef_double".into(),
        D,
        &[D],
        "%u = freeze double undef\n%r = fadd double %u, %a0\nret double %r".into(),
    );
    f.llc = false;
    add(f);
    // Compares: every predicate, every pair of specials; returned
    // (deferred, then asked for by `ret`), across a block (`cset` without
    // deferral) and fused into a `select` of each width.
    for t in scalars {
        let tl = t.ll();
        for p in PREDICATES {
            add(Func::new(
                format!("fcmp_{p}_{tl}"),
                I(1),
                &[t, t],
                format!("%c = fcmp {p} {tl} %a0, %a1\nret i1 %c"),
            )
            .with(Gen::Pairs));
            add(Func::new(
                format!("fcmpb_{p}_{tl}"),
                I(1),
                &[t, t],
                format!("%c = fcmp {p} {tl} %a0, %a1\nbr label %next\nnext:\nret i1 %c"),
            ));
            add(Func::new(
                format!("fsel_{p}_{tl}"),
                t,
                &[t, t, t, t],
                format!(
                    "%c = fcmp {p} {tl} %a0, %a1\n%r = select i1 %c, {tl} %a2, {tl} %a3\nret {tl} %r"
                ),
            )
            .with(Gen::Pairs));
        }
    }
    for (p, cmp, sel) in [
        ("one", D, Ty::V(4, El::I(32))),
        ("ueq", D, Ty::V(4, El::I(32))),
        ("olt", F, Ty::V(2, El::F)),
        ("ueq", F, Ty::V(2, El::F)),
        ("one", H, Q),
        ("ult", H, Q),
        ("uno", D, Ty::V(2, El::D)),
        ("ogt", F, D),
        ("une", D, H),
    ] {
        let (cl, sl) = (cmp.ll(), sel.ll());
        add(Func::new(
            format!("fselx_{p}_{}_{}", n(&cl), n(&sl)),
            sel,
            &[cmp, cmp, sel, sel],
            format!(
                "%c = fcmp {p} {cl} %a0, %a1\n%r = select i1 %c, {sl} %a2, {sl} %a3\nret {sl} %r"
            ),
        ));
    }
    // An `fcmp` whose result is used twice: not deferred.
    add(Func::new(
        "fcmp2_double".into(),
        D,
        &[D, D],
        "%c = fcmp olt double %a0, %a1\n%r = select i1 %c, double %a0, double %a1\n%s = select i1 %c, double %a1, double %a0\n%t = fsub double %r, %s\nret double %t".into(),
    ));
    // Conversions.
    for (op, from, to) in [
        ("fpext", H, F),
        ("fpext", H, D),
        ("fpext", F, D),
        ("fptrunc", F, H),
        ("fptrunc", D, H),
        ("fptrunc", D, F),
    ] {
        let (fl, tl) = (from.ll(), to.ll());
        add(Func::new(
            format!("{op}_{fl}_{tl}"),
            to,
            &[from],
            format!("%r = {op} {fl} %a0 to {tl}\nret {tl} %r"),
        ));
    }
    for t in scalars {
        let tl = t.ll();
        for w in WIDTHS {
            for (op, signed) in [("fptosi", true), ("fptoui", false)] {
                let mut f = Func::new(
                    format!("{op}_{tl}_i{w}"),
                    I(w),
                    &[t],
                    format!("%r = {op} {tl} %a0 to i{w}\nret i{w} %r"),
                );
                f.poison = Poison::ToInt { from: t, bits: w, signed };
                add(f);
            }
            for op in ["sitofp", "uitofp"] {
                add(Func::new(
                    format!("{op}_i{w}_{tl}"),
                    t,
                    &[I(w)],
                    format!("%r = {op} i{w} %a0 to {tl}\nret {tl} %r"),
                ));
            }
        }
    }
    // Bit casts.
    let bitcasts: &[(Ty, Ty)] = &[
        (F, I(32)),
        (I(32), F),
        (D, I(64)),
        (I(64), D),
        (H, I(16)),
        (I(16), H),
        (D, Ty::V(2, El::I(32))),
        (Ty::V(2, El::F), I(64)),
        (I(64), Ty::V(8, El::I(8))),
        (Ty::V(4, El::I(32)), Ty::V(2, El::D)),
        (Ty::V(2, El::I(64)), Q),
        (Q, Ty::V(2, El::I(64))),
        (Ty::V(4, El::H), D),
    ];
    for &(from, to) in bitcasts {
        let (fl, tl) = (from.ll(), to.ll());
        add(Func::new(
            format!("bitcast_{}_{}", n(&fl), n(&tl)),
            to,
            &[from],
            format!("%r = bitcast {fl} %a0 to {tl}\nret {tl} %r"),
        ));
    }
    // `bfloat` moves, through `i16` at the boundary.
    add(Func::new(
        "bf16_moves".into(),
        I(16),
        &[I(1), I(16), I(16)],
        "%x = bitcast i16 %a1 to bfloat\n%y = bitcast i16 %a2 to bfloat\n%f = freeze bfloat %x\n%s = select i1 %a0, bfloat %f, bfloat %y\n%n = fneg bfloat %s\n%r = bitcast bfloat %n to i16\nret i16 %r".into(),
    ));
    // Vectors.
    let ints = [
        Ty::V(16, El::I(8)),
        Ty::V(8, El::I(16)),
        Ty::V(4, El::I(32)),
        Ty::V(2, El::I(64)),
        Ty::V(8, El::I(8)),
        Ty::V(4, El::I(16)),
        Ty::V(2, El::I(32)),
        Ty::V(1, El::I(64)),
    ];
    for t in ints {
        let tl = t.ll();
        let Ty::V(_, e) = t else { unreachable!() };
        for op in ["add", "sub", "mul", "and", "or", "xor", "shl", "lshr", "ashr"] {
            if op == "mul" && e.bits() == 64 {
                continue;
            }
            let make = if matches!(op, "shl" | "lshr" | "ashr") { Gen::Shift } else { Gen::Any };
            add(Func::new(
                format!("{op}_{}", n(&tl)),
                t,
                &[t, t],
                format!("%r = {op} {tl} %a0, %a1\nret {tl} %r"),
            )
            .with(make));
        }
        add(Func::new(
            format!("sel_{}", n(&tl)),
            t,
            &[I(1), t, t],
            format!("%r = select i1 %a0, {tl} %a1, {tl} %a2\nret {tl} %r"),
        ));
        add(Func::new(
            format!("zero_{}", n(&tl)),
            t,
            &[t],
            format!("%r = add {tl} %a0, zeroinitializer\nret {tl} %r"),
        ));
    }
    for t in [Ty::V(2, El::F), Ty::V(4, El::F), Ty::V(2, El::D), Ty::V(1, El::D)] {
        let tl = t.ll();
        for op in ["fadd", "fsub", "fmul", "fdiv"] {
            add(Func::new(
                format!("{op}_{}", n(&tl)),
                t,
                &[t, t],
                format!("%r = {op} {tl} %a0, %a1\nret {tl} %r"),
            ));
        }
        add(Func::new(
            format!("fneg_{}", n(&tl)),
            t,
            &[t],
            format!("%r = fneg {tl} %a0\nret {tl} %r"),
        ));
        add(Func::new(
            format!("frz_{}", n(&tl)),
            t,
            &[t],
            format!("%r = freeze {tl} %a0\nret {tl} %r"),
        ));
    }
    for t in [Ty::V(4, El::H), Ty::V(8, El::H)] {
        let tl = t.ll();
        add(Func::new(
            format!("fneg_{}", n(&tl)),
            t,
            &[t],
            format!("%r = fneg {tl} %a0\nret {tl} %r"),
        ));
    }
    let vcasts: &[(&str, Ty, Ty, Gen)] = &[
        ("fpext", Ty::V(2, El::F), Ty::V(2, El::D), Gen::Any),
        ("fptrunc", Ty::V(2, El::D), Ty::V(2, El::F), Gen::Any),
        ("sitofp", Ty::V(4, El::I(32)), Ty::V(4, El::F), Gen::Any),
        ("uitofp", Ty::V(4, El::I(32)), Ty::V(4, El::F), Gen::Any),
        ("sitofp", Ty::V(2, El::I(32)), Ty::V(2, El::F), Gen::Any),
        ("uitofp", Ty::V(2, El::I(64)), Ty::V(2, El::D), Gen::Any),
        ("sitofp", Ty::V(2, El::I(64)), Ty::V(2, El::D), Gen::Any),
        ("fptosi", Ty::V(4, El::F), Ty::V(4, El::I(32)), Gen::Small),
        ("fptosi", Ty::V(2, El::D), Ty::V(2, El::I(64)), Gen::Small),
        ("fptosi", Ty::V(2, El::F), Ty::V(2, El::I(32)), Gen::Small),
        ("fptoui", Ty::V(4, El::F), Ty::V(4, El::I(32)), Gen::SmallPos),
        ("fptoui", Ty::V(2, El::D), Ty::V(2, El::I(64)), Gen::SmallPos),
        ("trunc", Ty::V(8, El::I(16)), Ty::V(8, El::I(8)), Gen::Any),
        ("trunc", Ty::V(4, El::I(32)), Ty::V(4, El::I(16)), Gen::Any),
        ("trunc", Ty::V(2, El::I(64)), Ty::V(2, El::I(32)), Gen::Any),
        ("zext", Ty::V(8, El::I(8)), Ty::V(8, El::I(16)), Gen::Any),
        ("sext", Ty::V(8, El::I(8)), Ty::V(8, El::I(16)), Gen::Any),
        ("zext", Ty::V(4, El::I(16)), Ty::V(4, El::I(32)), Gen::Any),
        ("sext", Ty::V(4, El::I(16)), Ty::V(4, El::I(32)), Gen::Any),
        ("zext", Ty::V(2, El::I(32)), Ty::V(2, El::I(64)), Gen::Any),
        ("sext", Ty::V(2, El::I(32)), Ty::V(2, El::I(64)), Gen::Any),
    ];
    for &(op, from, to, make) in vcasts {
        let (fl, tl) = (from.ll(), to.ll());
        add(Func::new(
            format!("{op}_{}_{}", n(&fl), n(&tl)),
            to,
            &[from],
            format!("%r = {op} {fl} %a0 to {tl}\nret {tl} %r"),
        )
        .with(make));
    }
    // `fptoui` of non-negative small lanes.
    add(Func::new(
        "ptrvec".into(),
        Ty::V(2, El::I(64)),
        &[Ty::V(2, El::I(64))],
        "%p = inttoptr <2 x i64> %a0 to <2 x ptr>\n%r = ptrtoint <2 x ptr> %p to <2 x i64>\nret <2 x i64> %r".into(),
    ));
    out
}

/// Random straight-line functions on `float` and `double`: chains of the
/// group's instructions over the arguments and constants, values used
/// once (register values, fused compares) and several times (homes),
/// calls of `fmod` among them (everything held is displaced).
fn random_chain(seed: u64) -> Func {
    let mut rng = Rng(seed);
    let nparams = 2 + rng.below(3) as usize;
    let params: Vec<Ty> = (0..nparams).map(|_| rng.pick(&[F, D, D, I(32), I(17)])).collect();
    let mut vals: Vec<(String, Ty)> =
        params.iter().enumerate().map(|(i, &t)| (format!("%a{i}"), t)).collect();
    let mut body = String::new();
    let steps = 3 + rng.below(9);
    let mut fresh = 0;
    let name = |fresh: &mut u32| {
        *fresh += 1;
        format!("%v{fresh}")
    };
    let float_of =
        |vals: &[(String, Ty)], rng: &mut Rng, want: Option<Ty>| -> Option<(String, Ty)> {
            let cands: Vec<&(String, Ty)> = vals
                .iter()
                .filter(|(_, t)| matches!(t, F | D) && want.is_none_or(|w| w == *t))
                .collect();
            if cands.is_empty() {
                None
            } else {
                Some(cands[rng.below(cands.len() as u64) as usize].clone())
            }
        };
    for _ in 0..steps {
        let kind = rng.below(10);
        let Some((a, t)) = float_of(&vals, &mut rng, None) else {
            // Make a float from an integer.
            let (x, xt) = vals[0].clone();
            let v = name(&mut fresh);
            let _ = writeln!(body, "{v} = sitofp {} {x} to double", xt.ll());
            vals.push((v, D));
            continue;
        };
        let tl = t.ll();
        let v = name(&mut fresh);
        match kind {
            0..=3 => {
                let op = rng.pick(&["fadd", "fsub", "fmul", "fdiv", "fadd", "fmul", "frem"]);
                let b = if rng.below(4) == 0 {
                    rng.pick(&["1.5", "-0.0", "0.25", "3.0"]).to_string()
                } else {
                    float_of(&vals, &mut rng, Some(t)).map(|x| x.0).unwrap_or(a.clone())
                };
                let _ = writeln!(body, "{v} = {op} {tl} {a}, {b}");
            }
            4 => {
                let _ = writeln!(body, "{v} = fneg {tl} {a}");
            }
            5 => {
                let (op, to) = if t == F { ("fpext", D) } else { ("fptrunc", F) };
                let _ = writeln!(body, "{v} = {op} {tl} {a} to {}", to.ll());
                vals.push((v, to));
                continue;
            }
            6 | 7 => {
                let b = float_of(&vals, &mut rng, Some(t)).map_or(a.clone(), |x| x.0);
                let x = float_of(&vals, &mut rng, None).unwrap();
                let y = float_of(&vals, &mut rng, Some(x.1)).map_or(x.0.clone(), |y| y.0);
                let p = rng.pick(&PREDICATES);
                let c = name(&mut fresh);
                let _ = writeln!(body, "{c} = fcmp {p} {tl} {a}, {b}");
                if kind == 7 {
                    // Something between the compare and its use.
                    let z = name(&mut fresh);
                    let _ = writeln!(body, "{z} = fadd {} {}, {}", x.1.ll(), x.0, y);
                    let _ = writeln!(
                        body,
                        "{v} = select i1 {c}, {} {z}, {} {}",
                        x.1.ll(),
                        x.1.ll(),
                        x.0
                    );
                } else {
                    let _ = writeln!(
                        body,
                        "{v} = select i1 {c}, {} {}, {} {y}",
                        x.1.ll(),
                        x.0,
                        x.1.ll()
                    );
                }
                vals.push((v, x.1));
                continue;
            }
            8 => {
                let ints: Vec<&(String, Ty)> =
                    vals.iter().filter(|(_, t)| matches!(t, I(_))).collect();
                if let Some((x, xt)) = ints.first().map(|x| (*x).clone()) {
                    let op = rng.pick(&["sitofp", "uitofp"]);
                    let _ = writeln!(body, "{v} = {op} {} {x} to {tl}", xt.ll());
                } else {
                    let _ = writeln!(body, "{v} = freeze {tl} {a}");
                }
            }
            _ => {
                let _ = writeln!(body, "{v} = freeze {tl} {a}");
            }
        }
        vals.push((v, t));
    }
    let (last, t) = vals.last().cloned().unwrap();
    let (ret, last) = if matches!(t, F | D) {
        (t, last)
    } else {
        let v = name(&mut fresh);
        let _ = writeln!(body, "{v} = sitofp {} {last} to double", t.ll());
        (D, v)
    };
    let _ = write!(body, "ret {} {last}", ret.ll());
    Func::new(format!("chain{seed}"), ret, &params, body)
}

// ---- the native run ----

const DRIVER_HEAD: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef uint64_t q_t __attribute__((vector_size(16)));
typedef uint64_t d_t __attribute__((vector_size(8)));
static inline _Float16 H(uint16_t b) { _Float16 v; memcpy(&v, &b, 2); return v; }
static inline float F(uint32_t b) { float v; memcpy(&v, &b, 4); return v; }
static inline double D(uint64_t b) { double v; memcpy(&v, &b, 8); return v; }
static inline long double Q(uint64_t lo, uint64_t hi) { uint64_t b[2] = { lo, hi }; long double v; memcpy(&v, b, 16); return v; }
static inline q_t VQ(uint64_t lo, uint64_t hi) { q_t v = { lo, hi }; return v; }
static inline d_t VD(uint64_t lo) { d_t v = { lo }; return v; }
static void out(uint64_t hi, uint64_t lo) { printf(" %016llx%016llx", (unsigned long long)hi, (unsigned long long)lo); }
static void P_h(_Float16 v) { uint16_t b; memcpy(&b, &v, 2); out(0, b); }
static void P_f(float v) { uint32_t b; memcpy(&b, &v, 4); out(0, b); }
static void P_d(double v) { uint64_t b; memcpy(&b, &v, 8); out(0, b); }
static void P_q(long double v) { uint64_t b[2]; memcpy(b, &v, 16); out(b[1], b[0]); }
static void P_w(uint32_t v) { out(0, v); }
static void P_x(uint64_t v) { out(0, v); }
static void P_vq(q_t v) { out(v[1], v[0]); }
static void P_vd(d_t v) { out(0, v[0]); }
static void nl(void) { putchar('\n'); }
"#;

/// The module text of `funcs` with every symbol prefixed by `prefix`.
fn module_text(funcs: &[Func], prefix: &str) -> String {
    let mut text = ll_header(22);
    for f in funcs {
        text.push_str(&f.text(prefix));
    }
    text
}

/// Runs `funcs` natively, compiled by hod (plain and stressed) and by
/// `llc`, with `per` random argument sets each (plus the pairs of
/// specials where the function asks for them), and compares the bits
/// with each other and with `hod_interp`. Returns the number of calls
/// compared.
fn run_native(label: &str, funcs: &[Func], per: usize, seed: u64) -> usize {
    let dir = hod_testutil::Scratch::new(label);
    let Some(toolchain) = Toolchain::host(&dir) else { return 0 };
    let variants: [(&str, Stress); 2] = [("h_", Stress::default()), ("s_", Stress::ALL)];
    let mut objects = Vec::new();
    let mut hod_text = String::new();
    for (prefix, stress) in variants {
        let text = module_text(funcs, prefix);
        let (m, bodies) = parse(&text);
        let emitted = emit(&m, &bodies, &Options { stress, ..Options::default() });
        objects.push(write_object(&dir, &format!("{prefix}hod.o"), &emitted));
        if prefix == "h_" {
            hod_text = text;
        }
    }
    let llc_object = {
        let major = common::llc_major();
        let text = module_text(funcs, "l_").replacen(&ll_header(22), &ll_header(major), 1);
        llc(&dir, "llc", &text, &["-relocation-model=pic"])
    };
    let mut prefixes = vec!["h_", "s_"];
    if let Some(object) = &llc_object {
        objects.push(object.clone());
        prefixes.push("l_");
    }
    // The calls, and the C that makes them.
    let mut rng = Rng(seed);
    let mut calls: Vec<(usize, Vec<u128>)> = Vec::new();
    let mut c = String::from(DRIVER_HEAD);
    for f in funcs {
        for prefix in &prefixes {
            let params: Vec<&str> = f.params.iter().map(|t| t.c()).collect();
            let params = if params.is_empty() { "void".to_string() } else { params.join(", ") };
            let _ = writeln!(c, "{} {prefix}{}({params});", f.ret.c(), f.name);
        }
    }
    for (index, f) in funcs.iter().enumerate() {
        let _ = writeln!(c, "static void t{index}(void) {{");
        let sets = if f.params.is_empty() { vec![Vec::new()] } else { f.arg_sets(&mut rng, per) };
        for args in sets {
            let junk = rng.next();
            let list: Vec<String> =
                f.params.iter().zip(&args).map(|(t, &a)| t.c_arg(a, junk)).collect();
            let list = list.join(", ");
            for prefix in &prefixes {
                let _ = write!(c, "{}({prefix}{}({list})); ", f.ret.c_print(), f.name);
            }
            let _ = writeln!(c, "nl();");
            calls.push((index, args));
        }
        let _ = writeln!(c, "}}");
    }
    c.push_str("int main(void) {\n");
    for index in 0..funcs.len() {
        let _ = writeln!(c, "  t{index}();");
    }
    c.push_str("  return 0;\n}\n");
    let driver = toolchain.compile_c(&dir, "driver", &c, &[]);
    let mut inputs: Vec<&std::path::Path> = vec![&driver];
    inputs.extend(objects.iter().map(std::path::PathBuf::as_path));
    // The interpreter's results, once.
    let (m, bodies) = parse(&hod_text);
    let program = Program::with_bodies(&m, &bodies).unwrap();
    let config = Config { nan: NanPolicy::Host, ..Config::default() };
    let expected: Vec<Option<u128>> = calls
        .iter()
        .map(|(index, args)| {
            let f = &funcs[*index];
            if !f.interp_ok() || f.poisoned(args) {
                return None;
            }
            let vals: Vec<Val> = f.params.iter().zip(args).map(|(t, &a)| t.val(a)).collect();
            let outcome =
                Interp::new(&program).with_config(config).run(bodies[*index].symbol(), &vals);
            match outcome.result {
                Ok(Some(v)) => Some(val_bits(&v)),
                other => panic!("{}: the interpreter: {other:?} for {args:x?}", f.name),
            }
        })
        .collect();
    let mut failures = Vec::new();
    let mut nan_only = 0usize;
    for (k, linker) in toolchain.linkers.iter().enumerate() {
        let program = dir.join(format!("float-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &["-Wl,--no-as-needed", "-lm"], &inputs, &program);
        assert_eq!(exit_code(&ran), Some(0), "{}: {}", linker.name, ran.stderr_text());
        let stdout = ran.stdout_text();
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), calls.len(), "{}: the driver's output", linker.name);
        if k > 0 {
            continue; // Every linker ran the same program; one is compared.
        }
        for (line, ((index, args), want)) in lines.iter().zip(calls.iter().zip(&expected)) {
            let f = &funcs[*index];
            let mask = f.ret.mask();
            let got: Vec<u128> = line
                .split_whitespace()
                .map(|h| u128::from_str_radix(h, 16).unwrap() & mask)
                .collect();
            let (hod, stressed) = (got[0], got[1]);
            let mut why = Vec::new();
            if hod != stressed {
                why.push(format!("stressed {stressed:x}"));
            }
            if let Some(&theirs) = got.get(2)
                && theirs != hod
                && f.llc
                && !f.poisoned(args)
            {
                if same_but_nan(f.ret, theirs, hod) {
                    nan_only += 1;
                } else {
                    why.push(format!("llc {theirs:x}"));
                }
            }
            if let Some(want) = want
                && *want != hod
            {
                why.push(format!("interp {want:x}"));
            }
            if !why.is_empty() {
                failures.push(format!("{}({args:x?}) = {hod:x}, but {}", f.name, why.join(", ")));
            }
        }
    }
    let by_interp = expected.iter().filter(|e| e.is_some()).count();
    eprintln!("{label}: {by_interp} calls also compared with hod_interp");
    eprintln!(
        "{label}: {nan_only} results where llc's NaN differs from hod's (and the interpreter's)"
    );
    assert!(
        failures.is_empty(),
        "{} of {} calls differ:\n{}",
        failures.len(),
        calls.len(),
        failures.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
    calls.len()
}

/// Every hand-written function, natively against `llc` and the
/// interpreter.
#[test]
fn every_form_agrees_with_llc_and_the_interpreter() {
    let funcs = hand_written();
    let calls = run_native("a64-float-forms", &funcs, 24, 39);
    eprintln!("{} functions, {calls} calls compared", funcs.len());
}

/// Random chains (`HOD_FLOAT_SEEDS`, default 300 functions).
#[test]
fn random_chains_agree_with_llc_and_the_interpreter() {
    let seeds: u64 =
        std::env::var("HOD_FLOAT_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    let funcs: Vec<Func> = (0..seeds).map(random_chain).collect();
    let calls = run_native("a64-float-chains", &funcs, 12, 7);
    eprintln!("{} functions, {calls} calls compared", funcs.len());
}

/// What the generator makes compiles.
#[test]
fn generated_functions_compile() {
    for f in hand_written().iter().chain((0..50).map(random_chain).collect::<Vec<_>>().iter()) {
        let text = f.text("");
        assert_eq!(compile_ll(&text), Ok::<(), CgError>(()), "{text}");
    }
}
