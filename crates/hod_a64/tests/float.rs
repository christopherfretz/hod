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
use common::{compile_ll, emit, ll_header, llc, parse, write_object};
use hod_cg::{CgError, Options, Stress};
use hod_interp::{Config, Interp, NanPolicy, Program, Val};

// ---- refusals ----

/// `bfloat` was refused here (`float::BFLOAT`) until WP-095 lowered it
/// (`tests/tail2.rs`): the three cases compile now.
#[test]
fn what_the_group_does_not_lower_is_refused_by_name() {
    for text in [
        "define bfloat @f(bfloat %a, bfloat %b) { %r = fadd bfloat %a, %b\n ret bfloat %r }",
        "define float @f(bfloat %a) { %r = fpext bfloat %a to float\n ret float %r }",
        "define i1 @f(bfloat %a, bfloat %b) { %r = fcmp oeq bfloat %a, %b\n ret i1 %r }",
    ] {
        assert_eq!(compile_ll(text), Ok(()), "{text}");
    }
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

/// Whether `fptosi`/`fptoui` of the `fp128` pattern `bits` to `i<width>`
/// is poison: exactly, from the bits (an `f64` would round).
fn quad_to_int_poison(bits: u128, width: u32, signed: bool) -> bool {
    let exp = ((bits >> 112) & 0x7fff) as i32;
    let negative = bits >> 127 != 0;
    if exp == 0x7fff {
        return true;
    }
    let e = exp - 16383;
    if e < 0 {
        return false; // |x| < 1 truncates to 0.
    }
    if e > 80 {
        return true;
    }
    let mant = (bits & ((1u128 << 112) - 1)) | (1u128 << 112);
    let int = if e >= 112 { mant << (e - 112) } else { mant >> (112 - e) };
    match (signed, negative) {
        (false, true) => int != 0,
        (false, false) => width < 128 && int >> width != 0,
        (true, false) => int >> (width - 1) != 0,
        (true, true) => int > 1u128 << (width - 1),
    }
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
            // 15..: the largest subnormal, the negative smallest one, a
            // negative NaN, the negative largest number (WP-067).
            0x0000_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            (1 << 127) | 1,
            0xffff_8000_u128 << 96,
            0xfffe_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            // 19..: the integer boundaries, 3 and 100.
            0x401e_u128 << 112,
            0x401d_ffff_fffc_u128 << 80,
            0x401f_u128 << 112,
            0x401e_ffff_fffe_u128 << 80,
            0x403e_u128 << 112,
            0x403d_ffff_ffff_ffff_fffc_u128 << 48,
            0x403f_u128 << 112,
            0x403e_ffff_ffff_ffff_fffe_u128 << 48,
            0xc01e_u128 << 112,
            0xc01e_0000_0002_u128 << 80,
            0xc03e_u128 << 112,
            0xc03e_0000_0000_0000_0002_u128 << 48,
            0x4000_8000_u128 << 96,
            0x4005_9000_u128 << 96,
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
        Q => &[0, 1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 15, 16, 17, 18],
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
    /// A C expression of the parameters `a0`, `a1`, ... that computes
    /// the result: the oracle for `fp128` (AArch64's `long double`) and
    /// `half`, which the interpreter does not have (WP-067).
    c: Option<String>,
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
            c: None,
        }
    }

    fn c(mut self, expr: impl Into<String>) -> Func {
        self.c = Some(expr.into());
        self
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
                    Q => return quad_to_int_poison(args[0], bits, signed),
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

/// A loop of `I & 7` + 1 iterations carrying a swap cycle and `live`
/// accumulators of type `t` through `phi`s; returns their sum.
fn phi_loop(t: Ty, live: usize) -> Func {
    let tl = t.ll();
    let op = match t {
        Ty::V(_, El::I(_)) => "add",
        _ => "fadd",
    };
    let konst = match t {
        H => "0xH3C00",
        F | D => "1.5",
        Q => "0xL00000000000000003FFF800000000000",
        _ => "zeroinitializer",
    };
    let mut b = String::from(
        "entry:\n%n = and i32 %a2, 7\nbr label %loop\nloop:\n%i = phi i32 [0, %entry], [%i1, %loop]\n",
    );
    let _ = writeln!(b, "%x0 = phi {tl} [%a0, %entry], [%x1, %loop]");
    let _ = writeln!(b, "%x1 = phi {tl} [%a1, %entry], [%x0, %loop]");
    for k in 0..live {
        let init = ["%a0", "%a1", konst][k % 3];
        let _ = writeln!(b, "%y{k} = phi {tl} [{init}, %entry], [%z{k}, %loop]");
    }
    for k in 0..live {
        let _ = writeln!(b, "%z{k} = {op} {tl} %y{k}, %y{}", (k + 1) % live);
    }
    b.push_str("%i1 = add i32 %i, 1\n%c = icmp sle i32 %i1, %n\nbr i1 %c, label %loop, label %exit\nexit:\n");
    let _ = writeln!(b, "%s0 = {op} {tl} %x0, %x1");
    for k in 0..live {
        let _ = writeln!(b, "%s{} = {op} {tl} %s{k}, %z{k}", k + 1);
    }
    let _ = write!(b, "ret {tl} %s{live}");
    Func::new(format!("phi_{}", n(&tl)), t, &[t, t, I(32)], b)
}

/// The C expression of `fcmp <p>` of `a0` and `a1`.
fn c_predicate(p: &str) -> String {
    match p {
        "false" => "0",
        "true" => "1",
        "oeq" => "a0 == a1",
        "une" => "!(a0 == a1)",
        "olt" => "a0 < a1",
        "ole" => "a0 <= a1",
        "ogt" => "a0 > a1",
        "oge" => "a0 >= a1",
        "ult" => "!(a0 >= a1)",
        "ule" => "!(a0 > a1)",
        "ugt" => "!(a0 <= a1)",
        "uge" => "!(a0 < a1)",
        "ord" => "!__builtin_isunordered(a0, a1)",
        "uno" => "__builtin_isunordered(a0, a1)",
        "one" => "__builtin_islessgreater(a0, a1)",
        "ueq" => "!__builtin_islessgreater(a0, a1)",
        _ => unreachable!("{p}"),
    }
    .into()
}

/// Every hand-written function of the group.
fn hand_written() -> Vec<Func> {
    let mut out = Vec::new();
    let mut add = |f: Func| out.push(f);
    let scalars = [H, F, D];
    // Binary operations, with two arguments and with constants.
    for t in [H, F, D, Q] {
        for op in ["fadd", "fsub", "fmul", "fdiv", "frem"] {
            let tl = t.ll();
            let make = if t == Q { Gen::Any } else { Gen::Pairs };
            let f = Func::new(
                format!("{op}_{tl}"),
                t,
                &[t, t],
                format!("%r = {op} {tl} %a0, %a1\nret {tl} %r"),
            )
            .with(make);
            let sym = match op {
                "fadd" => "+",
                "fsub" => "-",
                "fmul" => "*",
                _ => "/",
            };
            add(match (t, op) {
                (Q, "frem") => f.c("fmodl(a0, a1)"),
                (H, "frem") => f.c("(_Float16)fmodf(a0, a1)"),
                (Q, _) => f.c(format!("a0 {sym} a1")),
                _ => f,
            });
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
    for t in [H, F, D, Q] {
        let tl = t.ll();
        for p in PREDICATES {
            let c = c_predicate(p);
            let with_c = |f: Func| if t == Q { f.c(c.clone()) } else { f };
            add(with_c(
                Func::new(
                    format!("fcmp_{p}_{tl}"),
                    I(1),
                    &[t, t],
                    format!("%c = fcmp {p} {tl} %a0, %a1\nret i1 %c"),
                )
                .with(Gen::Pairs),
            ));
            add(with_c(Func::new(
                format!("fcmpb_{p}_{tl}"),
                I(1),
                &[t, t],
                format!("%c = fcmp {p} {tl} %a0, %a1\nbr label %next\nnext:\nret i1 %c"),
            )));
            let f = Func::new(
                format!("fsel_{p}_{tl}"),
                t,
                &[t, t, t, t],
                format!(
                    "%c = fcmp {p} {tl} %a0, %a1\n%r = select i1 %c, {tl} %a2, {tl} %a3\nret {tl} %r"
                ),
            )
            .with(Gen::Pairs);
            add(if t == Q { f.c(format!("({c}) ? a2 : a3")) } else { f });
        }
    }
    // `fp128` compares fused into selects of other types, and a compare
    // whose operands are one value (WP-067).
    for (p, sel) in [("ueq", D), ("one", F), ("olt", Ty::V(4, El::I(32))), ("uge", H)] {
        let sl = sel.ll();
        add(Func::new(
            format!("qsel_{p}_{}", n(&sl)),
            sel,
            &[Q, Q, sel, sel],
            format!(
                "%c = fcmp {p} fp128 %a0, %a1\n%r = select i1 %c, {sl} %a2, {sl} %a3\nret {sl} %r"
            ),
        ));
    }
    for p in ["oeq", "uno", "one", "ueq"] {
        add(Func::new(
            format!("qself_{p}"),
            I(1),
            &[Q],
            format!("%c = fcmp {p} fp128 %a0, %a0\nret i1 %c"),
        )
        .c(c_predicate(p).replace("a1", "a0")));
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
    for (op, from, to, c) in [
        ("fpext", H, Q, "(long double)a0"),
        ("fpext", F, Q, "(long double)a0"),
        ("fpext", D, Q, "(long double)a0"),
        ("fptrunc", Q, H, "(_Float16)a0"),
        ("fptrunc", Q, F, "(float)a0"),
        ("fptrunc", Q, D, "(double)a0"),
    ] {
        let (fl, tl) = (from.ll(), to.ll());
        add(Func::new(
            format!("{op}_{fl}_{tl}"),
            to,
            &[from],
            format!("%r = {op} {fl} %a0 to {tl}\nret {tl} %r"),
        )
        .c(c));
    }
    for w in WIDTHS {
        let mask = if w == 64 { u64::MAX } else { (1u64 << w) - 1 };
        for (op, signed) in [("fptosi", true), ("fptoui", false)] {
            let mut f = Func::new(
                format!("{op}_fp128_i{w}"),
                I(w),
                &[Q],
                format!("%r = {op} fp128 %a0 to i{w}\nret i{w} %r"),
            )
            .c(if signed { "(int64_t)a0" } else { "(uint64_t)a0" });
            f.poison = Poison::ToInt { from: Q, bits: w, signed };
            add(f);
        }
        let shift = 64 - w;
        for (op, c) in [
            ("sitofp", format!("(long double)((int64_t)((uint64_t)a0 << {shift}) >> {shift})")),
            ("uitofp", format!("(long double)((uint64_t)a0 & 0x{mask:x}ull)")),
        ] {
            add(Func::new(
                format!("{op}_i{w}_fp128"),
                Q,
                &[I(w)],
                format!("%r = {op} i{w} %a0 to fp128\nret fp128 %r"),
            )
            .c(c));
        }
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
        // Between `Fpr` values (WP-067: the matrix rows).
        (Ty::V(2, El::F), D),
        (D, Ty::V(4, El::H)),
        (Q, Ty::V(4, El::F)),
        (Ty::V(8, El::I(16)), Q),
        (Ty::V(1, El::D), Ty::V(8, El::I(8))),
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
    // Loops carrying `Fpr`-class values through `phi`s (WP-067 checks
    // WP-037's `control.rs` with them): more live values than the pool
    // has, a swap cycle, calls in the loop for `fp128`.
    for t in [D, F, H, Q, Ty::V(2, El::D), Ty::V(4, El::I(32)), Ty::V(8, El::I(8))] {
        add(phi_loop(t, 17));
    }
    // Non-zero vector constants (WP-067): `materialize_fpr_const`.
    for (t, op, c) in [
        (Ty::V(4, El::I(32)), "add", "<i32 1, i32 -1, i32 0, i32 2147483647>"),
        (Ty::V(2, El::I(64)), "xor", "<i64 -1, i64 0>"),
        (Ty::V(2, El::I(64)), "add", "<i64 0, i64 81985529216486895>"),
        (
            Ty::V(16, El::I(8)),
            "sub",
            "<i8 1, i8 2, i8 3, i8 4, i8 5, i8 6, i8 7, i8 8, i8 -1, i8 -2, i8 -3, i8 -4, i8 -5, i8 -6, i8 -7, i8 -128>",
        ),
        (Ty::V(8, El::I(8)), "or", "<i8 1, i8 0, i8 3, i8 0, i8 5, i8 0, i8 7, i8 -128>"),
        (Ty::V(4, El::I(16)), "mul", "<i16 -7, i16 -7, i16 -7, i16 -7>"),
        (Ty::V(1, El::I(64)), "and", "<i64 -81985529216486896>"),
        (Ty::V(2, El::I(32)), "shl", "<i32 1, i32 31>"),
        (Ty::V(2, El::D), "fadd", "<double 1.5, double -0.0>"),
        (Ty::V(4, El::F), "fmul", "<float 1.0, float 2.0, float 0.5, float -3.0>"),
        (Ty::V(2, El::F), "fsub", "<float 0x7FF8000000000000, float 0x36A0000000000000>"),
        (Ty::V(1, El::D), "fdiv", "<double 0x7FEFFFFFFFFFFFFF>"),
    ] {
        let tl = t.ll();
        add(Func::new(
            format!("vc_{op}_{}", n(&tl)),
            t,
            &[t],
            format!("%r = {op} {tl} %a0, {c}\nret {tl} %r"),
        ));
        add(Func::new(format!("vcret_{op}_{}", n(&tl)), t, &[], format!("ret {tl} {c}")));
    }
    add(Func::new(
        "vc_half".into(),
        Ty::V(4, El::H),
        &[],
        "%r = fneg <4 x half> <half 0xH3C00, half 0xH0001, half 0xH7E00, half 0xHFC00>\nret <4 x half> %r".into(),
    ));
    add(Func::new(
        "vc_ptrs".into(),
        Ty::V(2, El::I(64)),
        &[],
        "%r = ptrtoint <2 x ptr> <ptr @putchar, ptr getelementptr (i8, ptr @putchar, i64 16)> to <2 x i64>\nret <2 x i64> %r".into(),
    ));
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
#include <math.h>
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
    text.push_str("declare i32 @putchar(i32)\n");
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
        if let Some(expr) = &f.c {
            let params: Vec<String> =
                f.params.iter().enumerate().map(|(i, t)| format!("{} a{i}", t.c())).collect();
            let _ = writeln!(
                c,
                "static {} c_{}({}) {{ return ({}) ({expr}); }}",
                f.ret.c(),
                f.name,
                params.join(", "),
                f.ret.c()
            );
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
            if f.c.is_some() {
                let _ = write!(c, "{}(c_{}({list})); ", f.ret.c_print(), f.name);
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
            if f.c.is_some()
                && !f.poisoned(args)
                && let Some(&theirs) = got.last()
                && theirs != hod
                && !same_but_nan(f.ret, theirs, hod)
            {
                why.push(format!("C {theirs:x}"));
            }
            let llc_column = if f.c.is_some() { got.len() > 3 } else { got.len() > 2 };
            if llc_column
                && let Some(&theirs) = got.get(2)
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
    let by_c = calls.iter().filter(|(index, _)| funcs[*index].c.is_some()).count();
    eprintln!("{label}: {by_interp} calls also compared with hod_interp, {by_c} with C");
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
        let text = format!("declare i32 @putchar(i32)\n{}", f.text(""));
        assert_eq!(compile_ll(&text), Ok::<(), CgError>(()), "{text}");
    }
}

// ---- loads and stores of `Fpr`-class values (WP-067) ----
//
// Each function loads a value of type `T` through one addressing form
// and stores its argument through another; the C driver gives it a
// pointer into a buffer whose end is a guard page, fills the buffer and
// two variables (one through the GOT, one `dso_local`) with a pattern,
// and prints the loaded bits and every byte that changed. The model
// below computes the same lines (a load reads exactly the store size, a
// store writes exactly it), and so do hod plain, hod stressed and `llc`.

/// The buffer: `p` is `PBASE` bytes into it; it ends at a guard page.
const MEM_BUF: i64 = 0x30000;
const PBASE: i64 = 0x10000;
const MEM_END: i64 = MEM_BUF - PBASE;
/// The size of each variable.
const GSIZE: i64 = 256;

/// A type of class `Fpr` as the memory tests use it: its `.ll` name, its
/// store size, and the C type it travels as. `carrier` is the `.ll` type
/// it travels as when that is not the type itself, with the casts to and
/// from it.
#[derive(Clone, Copy, Debug)]
struct MemTy {
    ll: &'static str,
    size: i64,
    c: &'static str,
    carrier: Option<(&'static str, &'static str, &'static str)>,
}

const fn mt(ll: &'static str, size: i64, c: &'static str) -> MemTy {
    MemTy { ll, size, c, carrier: None }
}

const fn mtc(ll: &'static str, size: i64, c: &'static str, car: &'static str) -> MemTy {
    MemTy { ll, size, c, carrier: Some((car, "bitcast", "bitcast")) }
}

const MEM_TYPES: &[MemTy] = &[
    mt("half", 2, "_Float16"),
    mtc("bfloat", 2, "uint16_t", "i16"),
    mt("float", 4, "float"),
    mt("double", 8, "double"),
    mt("fp128", 16, "long double"),
    mt("<2 x float>", 8, "d_t"),
    mt("<4 x float>", 16, "q_t"),
    mt("<2 x double>", 16, "q_t"),
    mt("<1 x double>", 8, "d_t"),
    mt("<8 x i8>", 8, "d_t"),
    mt("<16 x i8>", 16, "q_t"),
    mt("<4 x i16>", 8, "d_t"),
    mt("<8 x i16>", 16, "q_t"),
    mt("<2 x i32>", 8, "d_t"),
    mt("<4 x i32>", 16, "q_t"),
    mt("<1 x i64>", 8, "d_t"),
    mt("<2 x i64>", 16, "q_t"),
    mt("<4 x half>", 8, "d_t"),
    mt("<8 x half>", 16, "q_t"),
    mtc("<16 x i4>", 8, "uint64_t", "i64"),
    mtc("<32 x i4>", 16, "q_t", "<2 x i64>"),
    mtc("<4 x bfloat>", 8, "uint64_t", "i64"),
    MemTy {
        ll: "<2 x ptr>",
        size: 16,
        c: "q_t",
        carrier: Some(("<2 x i64>", "inttoptr", "ptrtoint")),
    },
];

/// Where an address points.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Region {
    Buf,
    /// `@<prefix>g` (preemptible: through the GOT) or `@<prefix>d`
    /// (`dso_local`: direct).
    Global(bool),
}

/// An addressing form. `I` and `J` are the `i64 %i` and `i32 %j`
/// arguments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    /// `%p` itself.
    P,
    /// `%p` + a byte offset.
    C(i64),
    /// `%p` + `k` elements.
    CT(i64),
    /// `%p` + `I` elements.
    Xi,
    /// `%p` + `J` elements (`sxtw`).
    Xj,
    /// `%p` + `I` bytes.
    Xi8,
    /// `%p` + `I` × 3 elements + 1 element (a stride that is no power of
    /// two).
    Xi3,
    /// `%p` + `I` bytes + `k`, by two `ptradd`s.
    Two(i64),
    /// A variable + `k` bytes, as a constant expression.
    G(bool, i64),
    /// A variable + `I` elements.
    Gi(bool),
}

impl Form {
    fn region(self) -> Region {
        match self {
            Form::G(direct, _) | Form::Gi(direct) => Region::Global(direct),
            _ => Region::Buf,
        }
    }

    /// The offset as `a * x + b`, with `x` the index argument it uses
    /// (`Some(true)` for `I`, `Some(false)` for `J`).
    fn affine(self, size: i64) -> (Option<bool>, i64, i64) {
        match self {
            Form::P => (None, 0, 0),
            Form::C(k) | Form::G(_, k) => (None, 0, k),
            Form::CT(k) => (None, 0, k * size),
            Form::Xi | Form::Gi(_) => (Some(true), size, 0),
            Form::Xj => (Some(false), size, 0),
            Form::Xi8 => (Some(true), 1, 0),
            Form::Xi3 => (Some(true), 3 * size, size),
            Form::Two(k) => (Some(true), 1, k),
        }
    }

    /// The lines that form the address in `%<tag>`, and the operand.
    fn text(self, t: &MemTy, tag: &str, prefix: &str) -> (String, String) {
        let tl = t.ll;
        let gname = |direct: bool| format!("@{prefix}{}", if direct { "d" } else { "g" });
        let gep = |what: String| (format!("%{tag} = getelementptr {what}\n"), format!("%{tag}"));
        match self {
            Form::P => (String::new(), "%p".into()),
            Form::C(k) => gep(format!("i8, ptr %p, i64 {k}")),
            Form::CT(k) => gep(format!("{tl}, ptr %p, i64 {k}")),
            Form::Xi => gep(format!("{tl}, ptr %p, i64 %i")),
            Form::Xj => gep(format!("{tl}, ptr %p, i32 %j")),
            Form::Xi8 => gep("i8, ptr %p, i64 %i".to_string()),
            Form::Xi3 => gep(format!("[3 x {tl}], ptr %p, i64 %i, i64 1")),
            Form::Two(k) => (
                format!(
                    "%{tag}0 = getelementptr i8, ptr %p, i64 %i\n%{tag} = getelementptr i8, ptr %{tag}0, i64 {k}\n"
                ),
                format!("%{tag}"),
            ),
            Form::G(direct, k) => {
                (String::new(), format!("getelementptr (i8, ptr {}, i64 {k})", gname(direct)))
            }
            Form::Gi(direct) => gep(format!("{tl}, ptr {}, i64 %i", gname(direct))),
        }
    }
}

/// The byte forms of the buffer's and the variables' initial contents.
fn buf_init(k: i64) -> u8 {
    ((k as u64).wrapping_mul(0x9e37_79b1) >> 11) as u8
}

fn global_init(direct: bool, k: i64) -> u8 {
    (k * 37 + if direct { 101 } else { 11 }) as u8
}

/// What a memory function does.
#[derive(Clone, Debug)]
enum MemKind {
    /// Load through one form (if any), store the argument through another
    /// (if any); `early` computes the store's address before the load
    /// (no fusing).
    Access { load: Option<Form>, store: Option<Form>, volatile: (bool, bool), early: bool },
    /// Through a frame slot of `count` elements aligned to `align`, at
    /// element `I & (count - 1)` or at constant element `at`.
    Slot { count: i64, align: u64, at: Option<i64> },
    /// Store the constant (its `.ll` text and its bytes) to the buffer at
    /// `%p` and return a volatile load of it.
    Const(String, Vec<u8>),
}

struct MemFunc {
    name: String,
    ty: MemTy,
    kind: MemKind,
}

impl MemFunc {
    fn text(&self, prefix: &str) -> String {
        let t = &self.ty;
        let tl = t.ll;
        let (pl, conv_in, conv_out) = match t.carrier {
            Some((car, to, from)) => (car, Some(to), Some(from)),
            None => (tl, None, None),
        };
        let mut b = String::new();
        let v = if let Some(to) = conv_in {
            let _ = writeln!(b, "%v = {to} {pl} %x to {tl}");
            "%v"
        } else {
            "%x"
        };
        let r = match &self.kind {
            MemKind::Access { load, store, volatile, early } => {
                let st = store.map(|f| f.text(t, "s", prefix));
                if *early && let Some((pre, _)) = &st {
                    b.push_str(pre);
                }
                let r = if let Some(f) = load {
                    let (pre, op) = f.text(t, "l", prefix);
                    b.push_str(&pre);
                    let vol = if volatile.0 { "volatile " } else { "" };
                    let _ = writeln!(b, "%r = load {vol}{tl}, ptr {op}");
                    "%r"
                } else {
                    v
                };
                if let Some((pre, op)) = st {
                    if !*early {
                        b.push_str(&pre);
                    }
                    let vol = if volatile.1 { "volatile " } else { "" };
                    let _ = writeln!(b, "store {vol}{tl} {v}, ptr {op}");
                }
                r
            }
            MemKind::Slot { count, align, at } => {
                let _ = writeln!(b, "%s = alloca [{count} x {tl}], align {align}");
                if let Some(k) = at {
                    let _ = writeln!(b, "%q = getelementptr {tl}, ptr %s, i64 {k}");
                } else {
                    let _ = writeln!(b, "%m = and i64 %i, {}", count - 1);
                    let _ = writeln!(b, "%q = getelementptr {tl}, ptr %s, i64 %m");
                }
                let _ = writeln!(b, "store {tl} {v}, ptr %q\n%r = load {tl}, ptr %q");
                "%r"
            }
            MemKind::Const(c, _) => {
                let _ = writeln!(b, "store {tl} {c}, ptr %p\n%r = load volatile {tl}, ptr %p");
                "%r"
            }
        };
        let ret = if let Some(from) = conv_out {
            let _ = writeln!(b, "%o = {from} {tl} {r} to {pl}");
            "%o"
        } else {
            r
        };
        let _ = write!(b, "ret {pl} {ret}");
        format!("define {pl} @{prefix}{}(ptr %p, i64 %i, i32 %j, {pl} %x) {{\n{b}\n}}\n", self.name)
    }

    /// The forms that use index argument `I` (`true`) or `J`.
    fn forms(&self) -> Vec<Form> {
        match &self.kind {
            MemKind::Access { load, store, .. } => {
                load.iter().chain(store.iter()).copied().collect()
            }
            _ => Vec::new(),
        }
    }

    /// The range of index argument `I` (`which`) or `J` every form allows.
    fn range(&self, which: bool) -> (i64, i64) {
        let size = self.ty.size;
        let (mut lo, mut hi) = if which { (-(1 << 40), 1 << 40) } else { (-(1 << 30), 1 << 30) };
        for f in self.forms() {
            let (x, a, b) = f.affine(size);
            if x != Some(which) {
                continue;
            }
            let (min, max) = match f.region() {
                Region::Buf => (-PBASE, MEM_END - size),
                Region::Global(_) => (0, GSIZE - size),
            };
            lo = lo.max((min - b).div_euclid(a) + i64::from((min - b).rem_euclid(a) != 0));
            hi = hi.min((max - b).div_euclid(a));
        }
        (lo, hi)
    }

    /// The driver's line for arguments `i`, `j` and value bytes `x`.
    fn expect(&self, i: i64, j: i64, x: &[u8]) -> String {
        let size = self.ty.size;
        let mut buf: Vec<(i64, u8)> = Vec::new();
        let mut globals: [Vec<(i64, u8)>; 2] = [Vec::new(), Vec::new()];
        let at = |f: Form| {
            let (which, a, b) = f.affine(size);
            let arg = match which {
                Some(true) => i,
                Some(false) => j,
                None => 0,
            };
            (f.region(), a * arg + b)
        };
        let init = |region: Region, k: i64| match region {
            Region::Buf => buf_init(k + PBASE),
            Region::Global(direct) => global_init(direct, k),
        };
        let result: Vec<u8> = match &self.kind {
            MemKind::Access { load, store, .. } => {
                let r = match load {
                    Some(f) => {
                        let (region, off) = at(*f);
                        (0..size).map(|k| init(region, off + k)).collect()
                    }
                    None => x.to_vec(),
                };
                if let Some(f) = store {
                    let (region, off) = at(*f);
                    let list = match region {
                        Region::Buf => &mut buf,
                        Region::Global(direct) => &mut globals[usize::from(direct)],
                    };
                    for (k, &byte) in x.iter().enumerate() {
                        let pos = off + k as i64;
                        if byte != init(region, pos) {
                            list.push((
                                if region == Region::Buf { pos + PBASE } else { pos },
                                byte,
                            ));
                        }
                    }
                }
                r
            }
            MemKind::Slot { .. } => x.to_vec(),
            MemKind::Const(_, bytes) => {
                for (k, &byte) in bytes.iter().enumerate() {
                    let pos = k as i64;
                    if byte != init(Region::Buf, pos) {
                        buf.push((pos + PBASE, byte));
                    }
                }
                bytes.clone()
            }
        };
        let list = |l: &[(i64, u8)]| {
            l.iter().fold(String::new(), |mut s, (o, b)| {
                let _ = write!(s, " {o:x}:{b:02x}");
                s
            })
        };
        format!(
            "r {} b{} g{} d{}",
            hex_bytes(&result),
            list(&buf),
            list(&globals[0]),
            list(&globals[1])
        )
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The memory functions: for each type, every form as the load's with a
/// random store form and as the store's with a random load form, slots,
/// both accesses at one address.
fn mem_funcs() -> Vec<MemFunc> {
    let mut rng = Rng(67);
    let mut out = Vec::new();
    for (ti, t) in MEM_TYPES.iter().enumerate() {
        let s = t.size;
        let mut forms = vec![
            Form::P,
            Form::C(1),
            Form::C(s),
            Form::C(255),
            Form::C(256),
            Form::C(-256),
            Form::C(-257),
            Form::C(4095 * s),
            Form::C(4096 * s),
            Form::C(65520),
            Form::C(-PBASE),
            Form::C(MEM_END - s),
            Form::C(0x1_fff1 - s),
            Form::CT(1),
            Form::CT(-1),
            Form::CT(4095),
            Form::CT(-16),
            Form::Xi,
            Form::Xj,
            Form::Xi8,
            Form::Xi3,
            Form::Two(3),
            Form::Two(-70000 + PBASE),
            Form::G(false, 0),
            Form::G(true, 3),
            Form::G(false, GSIZE - s),
            Form::Gi(true),
            Form::Gi(false),
        ];
        forms.dedup();
        let pick = |rng: &mut Rng| forms[rng.below(forms.len() as u64) as usize];
        let mut add = |name: String, kind: MemKind| {
            out.push(MemFunc { name: format!("{name}_t{ti}"), ty: *t, kind });
        };
        for (k, &f) in forms.iter().enumerate() {
            let other = pick(&mut rng);
            let vol = (rng.below(4) == 0, rng.below(4) == 0);
            let early = rng.below(3) == 0;
            add(
                format!("ld{k}"),
                MemKind::Access { load: Some(f), store: Some(other), volatile: vol, early },
            );
            let other = pick(&mut rng);
            add(
                format!("st{k}"),
                MemKind::Access { load: Some(other), store: Some(f), volatile: vol, early: !early },
            );
        }
        add(
            "ldonly".into(),
            MemKind::Access {
                load: Some(Form::P),
                store: None,
                volatile: (true, false),
                early: false,
            },
        );
        add(
            "stonly".into(),
            MemKind::Access {
                load: None,
                store: Some(Form::P),
                volatile: (false, true),
                early: false,
            },
        );
        add(
            "same".into(),
            MemKind::Access {
                load: Some(Form::Xi),
                store: Some(Form::Xi),
                volatile: (false, false),
                early: true,
            },
        );
        add("slot1".into(), MemKind::Slot { count: 1, align: 1, at: None });
        add("slot8".into(), MemKind::Slot { count: 8, align: 64, at: None });
        add("slotbig".into(), MemKind::Slot { count: 8192, align: 16, at: Some(8191) });
        add("slotmid".into(), MemKind::Slot { count: 8192, align: 128, at: Some(4095) });
        for (k, (text, bytes)) in mem_constants(t).into_iter().enumerate() {
            add(format!("const{k}"), MemKind::Const(text, bytes));
        }
    }
    out
}

/// Constants of `t` to store: `.ll` text and bytes.
fn mem_constants(t: &MemTy) -> Vec<(String, Vec<u8>)> {
    let size = t.size as usize;
    let mut out = vec![("zeroinitializer".to_string(), vec![0; size])];
    let le = |v: u128| v.to_le_bytes()[..size].to_vec();
    match t.ll {
        "half" => out.push(("0xH3C01".into(), le(0x3c01))),
        "bfloat" => out.push(("0xR8001".into(), le(0x8001))),
        "float" => out.push(("-1.5".into(), le(0xbfc0_0000))),
        "double" => out.push(("0x7FF4000000000001".into(), le(0x7ff4_0000_0000_0001))),
        "fp128" => out.push((
            "0xL0123456789ABCDEF7FFF800000000001".into(),
            le(0x7fff_8000_0000_0001_0123_4567_89ab_cdef),
        )),
        _ => {
            if let Some(v) = vector_constant(t) {
                out.push(v);
            }
        }
    }
    out
}

/// A non-zero constant of a vector type of `MEM_TYPES`, lane by lane.
fn vector_constant(t: &MemTy) -> Option<(String, Vec<u8>)> {
    let ll = t.ll.strip_prefix('<')?.strip_suffix('>')?;
    let (n, elem) = ll.split_once(" x ")?;
    let n: u32 = n.parse().ok()?;
    let mut bits = 0u128;
    let mut lanes = Vec::new();
    let width = match elem {
        "half" | "bfloat" => 16,
        "float" => 32,
        "double" => 64,
        "ptr" => return None,
        _ => elem.strip_prefix('i')?.parse().ok()?,
    };
    for k in 0..n {
        let (text, lane) = match elem {
            "half" => {
                (format!("half 0xH{:04X}", 0x3c00 + k * 0x111), u128::from(0x3c00 + k * 0x111))
            }
            "bfloat" => (format!("bfloat 0xR{:04X}", 0xbf80 + k), u128::from(0xbf80 + k)),
            "float" => {
                let x = 1.5f32 - k as f32 * 0.75;
                (format!("float 0x{:016X}", f64::from(x).to_bits()), u128::from(x.to_bits()))
            }
            "double" => {
                let x = -2.25f64 + f64::from(k);
                (format!("double 0x{:016X}", x.to_bits()), u128::from(x.to_bits()))
            }
            _ => {
                let v = (u128::from(k) * 0x9e37_79b9_7f4a_7c15 + 1) & ((1u128 << width) - 1);
                (format!("{elem} {v}"), v)
            }
        };
        bits |= lane << (k * width);
        lanes.push(text);
    }
    Some((format!("<{}>", lanes.join(", ")), bits.to_le_bytes()[..t.size as usize].to_vec()))
}

/// The C literal of a value of `t` with bytes `x`.
fn mem_c_value(t: &MemTy, x: &[u8]) -> String {
    let word = |k: usize| {
        let mut w = [0u8; 8];
        let end = (k + 8).min(x.len());
        w[..end - k].copy_from_slice(&x[k..end]);
        u64::from_le_bytes(w)
    };
    match (t.c, t.size) {
        ("_Float16", _) => format!("H(0x{:x})", word(0)),
        ("float", _) => format!("F(0x{:x}u)", word(0)),
        ("double", _) => format!("D(0x{:x}ull)", word(0)),
        ("long double", _) => format!("Q(0x{:x}ull, 0x{:x}ull)", word(0), word(8)),
        ("d_t", _) => format!("VD(0x{:x}ull)", word(0)),
        ("q_t", _) => format!("VQ(0x{:x}ull, 0x{:x}ull)", word(0), word(8)),
        ("uint16_t", _) => format!("(uint16_t)0x{:x}", word(0)),
        ("uint64_t", _) => format!("0x{:x}ull", word(0)),
        _ => unreachable!("{t:?}"),
    }
}

const MEM_DRIVER: &str = r#"
#include <sys/mman.h>
#include <unistd.h>
static unsigned char *buf;
static void pb(const void *v, int n) { const unsigned char *b = v; printf("r "); for (int k = 0; k < n; k++) printf("%02x", b[k]); }
static unsigned char ib(long k) { return (unsigned char)(((unsigned long)k * 0x9e3779b1ul) >> 11); }
static unsigned char ig(int direct, long k) { return (unsigned char)(k * 37 + (direct ? 101 : 11)); }
static void fill(unsigned char *g, unsigned char *d) {
  for (long k = 0; k < 256; k++) { g[k] = ig(0, k); d[k] = ig(1, k); }
}
static void diff(unsigned char *g, unsigned char *d) {
  printf(" b");
  for (long k = 0; k < 0x30000; k++) if (buf[k] != ib(k)) { printf(" %lx:%02x", k, buf[k]); buf[k] = ib(k); }
  printf(" g");
  for (long k = 0; k < 256; k++) if (g[k] != ig(0, k)) printf(" %lx:%02x", k, g[k]);
  printf(" d");
  for (long k = 0; k < 256; k++) if (d[k] != ig(1, k)) printf(" %lx:%02x", k, d[k]);
  printf("\n");
  fill(g, d);
}
"#;

/// Runs the memory functions natively; returns the calls compared.
fn run_mem(label: &str, funcs: &[MemFunc], per: usize) -> usize {
    let dir = hod_testutil::Scratch::new(label);
    let Some(toolchain) = Toolchain::host(&dir) else { return 0 };
    let globals = |prefix: &str| {
        format!(
            "@{prefix}g = global [{GSIZE} x i8] zeroinitializer, align 16\n@{prefix}d = dso_local global [{GSIZE} x i8] zeroinitializer, align 16\n"
        )
    };
    let text = |prefix: &str| {
        let mut text = globals(prefix);
        for f in funcs {
            text.push_str(&f.text(prefix));
        }
        text
    };
    let mut objects = Vec::new();
    for (prefix, stress) in [("h_", Stress::default()), ("s_", Stress::ALL)] {
        let (m, bodies) = parse(&format!("{}{}", ll_header(22), text(prefix)));
        let emitted = emit(&m, &bodies, &Options { stress, ..Options::default() });
        objects.push(write_object(&dir, &format!("{prefix}hod.o"), &emitted));
    }
    let mut prefixes = vec!["h_", "s_"];
    if let Some(object) = llc(
        &dir,
        "llc",
        &format!("{}{}", ll_header(common::llc_major()), text("l_")),
        &["-relocation-model=pic"],
    ) {
        objects.push(object);
        prefixes.push("l_");
    }
    eprintln!("{label}: compared {}", prefixes.join(", "));
    let mut c = String::from(DRIVER_HEAD);
    c.push_str(MEM_DRIVER);
    for prefix in &prefixes {
        let _ = writeln!(c, "extern unsigned char {prefix}g[{GSIZE}], {prefix}d[{GSIZE}];");
        for f in funcs {
            let pc = f.ty.c;
            let _ =
                writeln!(c, "{pc} {prefix}{}(unsigned char *, int64_t, int32_t, {pc});", f.name);
        }
    }
    let mut rng = Rng(670);
    let mut expected: Vec<String> = Vec::new();
    let mut calls = 0;
    for (index, f) in funcs.iter().enumerate() {
        let _ = writeln!(c, "static void t{index}(void) {{");
        let (ilo, ihi) = f.range(true);
        let (jlo, jhi) = f.range(false);
        let pick = |rng: &mut Rng, lo: i64, hi: i64, k: usize| match k {
            0 => lo,
            1 => hi,
            2 if lo <= 0 && 0 <= hi => 0,
            _ => lo + (rng.next() % ((hi - lo) as u64 + 1).min(1 << 40)) as i64,
        };
        for k in 0..per {
            let (i, j) = (pick(&mut rng, ilo, ihi, k), pick(&mut rng, jlo, jhi, (k + 1) % per));
            let x: Vec<u8> = (0..f.ty.size).map(|_| rng.next() as u8).collect();
            let lit = mem_c_value(&f.ty, &x);
            for prefix in &prefixes {
                let _ = writeln!(
                    c,
                    "{{ {} r = {prefix}{}(buf + 0x{PBASE:x}, {i}ll, {j}, {lit}); pb(&r, {}); diff({prefix}g, {prefix}d); }}",
                    f.ty.c, f.name, f.ty.size
                );
                expected.push(f.expect(i, j, &x));
            }
            calls += 1;
        }
        let _ = writeln!(c, "}}");
    }
    c.push_str("int main(void) {\n  long pg = sysconf(_SC_PAGESIZE);\n");
    c.push_str("  unsigned char *m = mmap(0, 0x30000 + pg, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);\n");
    c.push_str(
        "  if (m == MAP_FAILED || mprotect(m + 0x30000, pg, PROT_NONE)) return 2;\n  buf = m;\n",
    );
    c.push_str("  for (long k = 0; k < 0x30000; k++) buf[k] = ib(k);\n");
    for prefix in &prefixes {
        let _ = writeln!(c, "  fill({prefix}g, {prefix}d);");
    }
    for index in 0..funcs.len() {
        let _ = writeln!(c, "  t{index}();");
    }
    c.push_str("  return 0;\n}\n");
    let driver = toolchain.compile_c(&dir, "driver", &c, &[]);
    let mut inputs: Vec<&std::path::Path> = vec![&driver];
    inputs.extend(objects.iter().map(std::path::PathBuf::as_path));
    let mut failures = Vec::new();
    for (k, linker) in toolchain.linkers.iter().enumerate() {
        let program = dir.join(format!("mem-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &[], &inputs, &program);
        assert_eq!(exit_code(&ran), Some(0), "{}: {}", linker.name, ran.stderr_text());
        if k > 0 {
            continue;
        }
        let stdout = ran.stdout_text();
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), expected.len(), "the driver's output");
        for (n, line) in lines.iter().enumerate() {
            let call = n / prefixes.len();
            let f = &funcs[call / per];
            let first = lines[n - n % prefixes.len()];
            let want = &expected[n];
            if want != line {
                failures.push(format!(
                    "{}{}: {line}\n   want {want}",
                    prefixes[n % prefixes.len()],
                    f.name
                ));
            } else if *line != first {
                failures.push(format!(
                    "{}{}: {line}\n   but {first}",
                    prefixes[n % prefixes.len()],
                    f.name
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} lines differ:\n{}\n{}",
        failures.len(),
        failures.iter().take(30).cloned().collect::<Vec<_>>().join("\n"),
        funcs[0].text("h_"),
    );
    calls
}

/// Loads and stores of every `Fpr`-class type through every addressing
/// form, natively against the model and `llc`.
#[test]
fn loads_and_stores_agree_with_the_model_and_llc() {
    let funcs = mem_funcs();
    let calls = run_mem("a64-float-mem", &funcs, 6);
    eprintln!("{} functions, {calls} calls compared", funcs.len());
}

/// An atomic load or store of an `Fpr`-class value compiles since
/// WP-074 (`tests/atomic.rs` checks what it does).
#[test]
fn atomic_fp_accesses_compile() {
    for text in [
        "define double @f(ptr %p) { %r = load atomic double, ptr %p seq_cst, align 8\n ret double %r }",
        "define void @f(ptr %p, float %v) { store atomic float %v, ptr %p release, align 4\n ret void }",
    ] {
        assert_eq!(compile_ll(text), Ok(()), "{text}");
    }
}
