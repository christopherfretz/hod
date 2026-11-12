//! The floating-point intrinsics' own programs (WP-070): one function per
//! intrinsic and type (`half`, `float`, `double`, `fp128`), run natively
//! from one C driver, compiled by hod plainly (`h_`) and with every
//! stress option (`s_`) and by `llc -O0 -global-isel=0 -fast-isel=0`
//! (`l_`), linked under every linker with `-lm`, and compared bit for
//! bit:
//!
//! - with `hod_interp` where it evaluates the intrinsic (`float` and
//!   `double` `fabs`, `copysign`, `sqrt`, the rounding family, `fma`);
//! - with a C expression of the same operation (gcc, `_Float16`, `long
//!   double` = IEEE quad): exactly for the bit operations, NaN for NaN
//!   elsewhere (a computed NaN's payload is open, `LangRef`);
//! - with a Rust model where the operation is defined bit for bit
//!   (`fptosi.sat`/`fptoui.sat`, `is.fpclass`);
//! - with `llc`, a difference only in a NaN's bits counted, not failed.
//!
//! The values, the C helpers and the comparison follow `tests/float.rs`
//! (WP-039), whose harness lives in a test file of its own.

#![allow(dead_code, reason = "each test uses part of it")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    clippy::type_complexity,
    reason = "a test generator over bit patterns"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;

use hod_cg::{Options, Stress};
use hod_interp::{Config, Interp, NanPolicy, Program, Val};

use crate::common::linking::{Toolchain, exit_code};
use crate::common::{emit, ll_header, llc, llc_major, parse, write_object};

// ---- types and values ----

/// A type the functions pass, return or compute in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    H,
    F,
    D,
    Q,
    I(u32),
}

pub use Ty::{D, F, H, I, Q};

/// The floating-point types.
pub const FLOATS: [Ty; 4] = [H, F, D, Q];

impl Ty {
    pub fn ll(self) -> String {
        match self {
            H => "half".into(),
            F => "float".into(),
            D => "double".into(),
            Q => "fp128".into(),
            I(w) => format!("i{w}"),
        }
    }

    /// The intrinsic name suffix.
    pub fn suffix(self) -> String {
        match self {
            H => "f16".into(),
            F => "f32".into(),
            D => "f64".into(),
            Q => "f128".into(),
            I(w) => format!("i{w}"),
        }
    }

    pub fn bits(self) -> u32 {
        match self {
            H => 16,
            F => 32,
            D => 64,
            Q => 128,
            I(w) => w,
        }
    }

    pub fn mask(self) -> u128 {
        if self.bits() >= 128 { u128::MAX } else { (1u128 << self.bits()) - 1 }
    }

    /// The C type it travels as.
    pub fn c(self) -> &'static str {
        match self {
            H => "_Float16",
            F => "float",
            D => "double",
            Q => "long double",
            I(w) if w <= 32 => "uint32_t",
            I(_) => "uint64_t",
        }
    }

    /// The `float` function suffix of C (`sqrtf`, `sqrt`, `sqrtl`): `half`
    /// computes in `float`.
    pub fn libm(self, name: &str) -> String {
        match self {
            H | F => format!("{name}f"),
            D => name.into(),
            Q => format!("{name}l"),
            I(_) => unreachable!(),
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
        }
    }

    fn c_print(self) -> &'static str {
        match self {
            H => "P_h",
            F => "P_f",
            D => "P_d",
            Q => "P_q",
            I(w) if w <= 32 => "P_w",
            I(_) => "P_x",
        }
    }

    fn val(self, bits: u128) -> Val {
        match self {
            F => Val::Float(bits as u32),
            D => Val::Double(bits as u64),
            I(w) => Val::int(w, bits & self.mask()),
            _ => unreachable!("{self:?} is not an interpreter type"),
        }
    }

    /// (exponent bits, fraction bits) of a floating-point type.
    pub fn fields(self) -> (u32, u32) {
        match self {
            H => (5, 10),
            F => (8, 23),
            D => (11, 52),
            Q => (15, 112),
            I(_) => unreachable!(),
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
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }
}

/// Whether `bits` of `ty` is a NaN.
pub fn is_nan(ty: Ty, bits: u128) -> bool {
    let I(_) = ty else {
        let (e, f) = ty.fields();
        let exp = (bits >> f) & ((1 << e) - 1);
        return exp == (1 << e) - 1 && bits & ((1u128 << f) - 1) != 0;
    };
    false
}

fn same_but_nan(ty: Ty, a: u128, b: u128) -> bool {
    a == b || (is_nan(ty, a) && is_nan(ty, b))
}

/// The value of a `half`, `float` or `double` pattern, exactly.
pub fn to_f64(ty: Ty, bits: u128) -> f64 {
    match ty {
        F => f64::from(f32::from_bits(bits as u32)),
        D => f64::from_bits(bits as u64),
        H => {
            let b = bits as u32;
            let sign = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
            let exp = (b >> 10) & 0x1f;
            let frac = f64::from(b & 0x3ff);
            sign * match exp {
                0 => frac * 2f64.powi(-24),
                31 if frac == 0.0 => f64::INFINITY,
                31 => f64::NAN,
                _ => (1.0 + frac / 1024.0) * 2f64.powi(exp as i32 - 15),
            }
        }
        _ => unreachable!("{ty:?}"),
    }
}

/// The pattern of `x` in `ty`, which must hold it exactly (zero, normal
/// numbers).
pub fn from_f64(ty: Ty, x: f64) -> u128 {
    if ty == F {
        return u128::from((x as f32).to_bits());
    }
    let b = x.to_bits();
    if ty == D {
        return u128::from(b);
    }
    let sign = u128::from(b >> 63);
    if x == 0.0 {
        return sign << (ty.bits() - 1);
    }
    let exp = ((b >> 52) & 0x7ff) as i64 - 1023;
    let frac = u128::from(b & ((1 << 52) - 1));
    let (e, f) = ty.fields();
    let bias = (1i64 << (e - 1)) - 1;
    let frac = if f >= 52 { frac << (f - 52) } else { frac >> (52 - f) };
    (sign << (ty.bits() - 1)) | (((exp + bias) as u128) << f) | frac
}

/// The special values: zeros, ones, halves, extremes, subnormals,
/// infinities, quiet and signalling NaNs of both signs, integer
/// boundaries.
pub fn specials(ty: Ty) -> Vec<u128> {
    match ty {
        H => vec![
            0, 0x8000, 0x3c00, 0xbc00, 0x3e00, 0x3800, 0xb800, 0x7bff, 0xfbff, 0x0400, 0x0001,
            0x03ff, 0x8001, 0x7c00, 0xfc00, 0x7e00, 0x7c01, 0x7e55, 0xfe01, 0x5640, 0xc7c0, 0x4200,
            0x5bff, 0x7800, 0x3a00, 0xba00, 0x8400, 0x83ff, 0xfd00,
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
            0x4020_0000,
            0xc020_0000,
            0x8080_0000,
            0x807f_ffff,
            0xffa0_0000,
            0x4eff_ffff,
            0x5f7f_ffff,
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
            0x4004_0000_0000_0000,
            0xc004_0000_0000_0000,
            0x41df_ffff_ffc0_0000,
            0x43df_ffff_ffff_ffff,
            0x41ef_ffff_ffe0_0000,
            0xfff4_0000_0000_0000,
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
            0x3ffe_u128 << 112,
            0x7ffe_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            0x0001_u128 << 112,
            1,
            0x7fff_u128 << 112,
            0xffff_u128 << 112,
            0x7fff_8000_u128 << 96,
            (0x7fff_u128 << 112) | 1,
            (0x7fff_8000_u128 << 96) | 0x1234_5678,
            0x4000_921f_b544_42d1_8469_898c_c517_01b8,
            0x0000_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            (1 << 127) | 1,
            0xffff_8000_u128 << 96,
            0xfffe_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            0x401e_u128 << 112,
            0x403e_u128 << 112,
            0xc03e_u128 << 112,
            0x4000_8000_u128 << 96,
            0x4005_9000_u128 << 96,
            0xbffe_u128 << 112,
            0x4000_4000_u128 << 96,
            (0x7fff_u128 << 112) | (1 << 64),
            0xffff_0000_0000_0000_0000_0000_0000_0001,
            0x8000_ffff_ffff_ffff_ffff_ffff_ffff_ffff,
            0x3fff_0000_0000_0000_0000_0000_0000_0001,
        ],
        I(_) => unreachable!(),
    }
}

/// The specials compared pairwise: a NaN of each kind, both zeros and
/// infinities, subnormals, ordinary numbers, the extremes.
fn pair_specials(ty: Ty) -> Vec<u128> {
    let all = specials(ty);
    let pick: &[usize] = match ty {
        H | F | D => &[0, 1, 2, 3, 4, 7, 9, 10, 12, 13, 14, 15, 16, 18, 19],
        Q => &[0, 1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 15, 16, 17, 18],
        I(_) => unreachable!(),
    };
    pick.iter().map(|&i| all[i]).collect()
}

/// A random value of `ty`: a raw pattern, or an ordinary number.
pub fn random(ty: Ty, rng: &mut Rng) -> u128 {
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
        I(w) => {
            let m = ty.mask();
            let edges = [0u128, 1, m, m >> 1, (m >> 1) + 1, 2, 3, m - 1];
            if rng.below(3) == 0 {
                rng.pick(&edges) & m
            } else {
                let shift = rng.below(u64::from(w)) as u32;
                (u128::from(raw) | (u128::from(rng.next()) << 64)) & (m >> shift)
            }
        }
    }
}

/// A special or a random value.
pub fn value(ty: Ty, rng: &mut Rng) -> u128 {
    match ty {
        H | F | D | Q if rng.below(2) == 0 => rng.pick(&specials(ty)),
        _ => random(ty, rng),
    }
}

/// A number in (-512, 512) in quarters, often a half-way point.
fn small(ty: Ty, rng: &mut Rng) -> u128 {
    let k = rng.below(4095) as f64 / 4.0 - 511.75;
    let x = if rng.below(2) == 0 { k.trunc() + 0.5 * k.signum() } else { k };
    from_f64(ty, x)
}

/// Exponents of `ldexp`/`powi`: around the formats' ranges and the
/// `int` bounds.
fn exponent(ty: Ty, rng: &mut Rng) -> u128 {
    let picks: [i64; 24] = [
        0,
        1,
        -1,
        2,
        -2,
        3,
        10,
        -10,
        15,
        -24,
        24,
        127,
        -126,
        -149,
        -150,
        128,
        1023,
        -1022,
        -1074,
        -1075,
        16383,
        -16494,
        0x7fff_ffff,
        -0x8000_0000,
    ];
    let k = if rng.below(4) == 0 { rng.next() as i64 >> rng.below(64) } else { rng.pick(&picks) };
    (k as u128) & ty.mask()
}

// ---- the functions ----

/// How the arguments of a function are made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gen {
    /// Specials and random values.
    Any,
    /// Every pair of the specials for the first two, then random ones.
    Pairs,
    /// Small numbers, half-way points (`lround` and friends).
    Small,
    /// The second an exponent (`ldexp`, `powi`).
    Exp,
    /// The second an exponent in `int` range only.
    ExpInt,
}

/// A function and its oracles.
#[allow(clippy::struct_excessive_bools, reason = "one switch per oracle")]
pub struct Func {
    pub name: String,
    pub ret: Ty,
    pub params: Vec<Ty>,
    /// The body, naming the parameters `%a0`, `%a1`, ...
    pub body: String,
    /// The `declare` lines it needs.
    pub decls: Vec<String>,
    pub make: Gen,
    /// Whether `hod_interp` evaluates it.
    pub interp: bool,
    /// Whether `llc`'s result is compared.
    pub llc: bool,
    /// Whether `llc` compiles it at all (not where it crashes).
    pub llc_builds: bool,
    /// A C expression of `a0`, `a1`, ... computing the result.
    pub c: Option<String>,
    /// Whether the C result must match NaN bits too.
    pub exact_c: bool,
    /// The result from the argument patterns.
    pub model: Option<Box<dyn Fn(&[u128]) -> u128>>,
}

impl Func {
    pub fn new(name: impl Into<String>, ret: Ty, params: &[Ty], body: String) -> Func {
        Func {
            name: name.into(),
            ret,
            params: params.to_vec(),
            body,
            decls: Vec::new(),
            make: Gen::Any,
            interp: false,
            llc: true,
            llc_builds: true,
            c: None,
            exact_c: false,
            model: None,
        }
    }

    pub fn c(mut self, expr: impl Into<String>) -> Func {
        self.c = Some(expr.into());
        self
    }

    pub fn exact(mut self) -> Func {
        self.exact_c = true;
        self
    }

    pub fn with(mut self, make: Gen) -> Func {
        self.make = make;
        self
    }

    pub fn interp(mut self, on: bool) -> Func {
        self.interp = on;
        self
    }

    pub fn no_llc(mut self) -> Func {
        self.llc = false;
        self
    }

    /// Not compiled by `llc`, which crashes on it.
    pub fn llc_crashes(mut self) -> Func {
        self.llc = false;
        self.llc_builds = false;
        self
    }

    pub fn decl(mut self, d: String) -> Func {
        self.decls.push(d);
        self
    }

    pub fn model(mut self, m: impl Fn(&[u128]) -> u128 + 'static) -> Func {
        self.model = Some(Box::new(m));
        self
    }

    /// The definition, named `<prefix><name>`.
    pub fn text_of(&self, prefix: &str) -> String {
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

    fn arg_sets(&self, rng: &mut Rng, count: usize) -> Vec<Vec<u128>> {
        let mut out = Vec::new();
        if self.make == Gen::Pairs {
            let s = pair_specials(self.params[0]);
            for &a in &s {
                for &b in &s {
                    let mut set = vec![a, b];
                    set.extend(self.params[2..].iter().map(|&t| value(t, rng)));
                    out.push(set);
                }
            }
        }
        if self.make == Gen::Any && self.params.len() == 1 {
            out.extend(specials(self.params[0]).into_iter().map(|v| vec![v]));
        }
        for _ in 0..count {
            let set = self
                .params
                .iter()
                .enumerate()
                .map(|(i, &t)| match (self.make, t) {
                    (Gen::Small, I(_)) => random(t, rng),
                    (Gen::Small, _) => small(t, rng),
                    (Gen::Exp, I(_)) if i == 1 => exponent(t, rng),
                    (Gen::ExpInt, I(w)) if i == 1 => {
                        let k = (exponent(I(32), rng) as u32) as i32;
                        (i128::from(k) as u128) & I(w).mask()
                    }
                    _ => value(t, rng),
                })
                .collect();
            out.push(set);
        }
        out
    }
}

/// `%r = call <ret> @llvm.<name>(<params>)`; `ret <ret> %r`, declared.
pub fn intrinsic(name: &str, full: &str, ret: Ty, params: &[Ty]) -> Func {
    let decl_params: Vec<String> = params.iter().map(|t| t.ll()).collect();
    let args: Vec<String> =
        params.iter().enumerate().map(|(i, t)| format!("{} %a{i}", t.ll())).collect();
    let body = format!(
        "  %r = call {} @llvm.{full}({})\n  ret {} %r",
        ret.ll(),
        args.join(", "),
        ret.ll()
    );
    Func::new(name, ret, params, body).decl(format!(
        "declare {} @llvm.{full}({})",
        ret.ll(),
        decl_params.join(", ")
    ))
}

// ---- the native run ----

const DRIVER_HEAD: &str = r#"
#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <math.h>
static inline _Float16 H(uint16_t b) { _Float16 v; memcpy(&v, &b, 2); return v; }
static inline float F(uint32_t b) { float v; memcpy(&v, &b, 4); return v; }
static inline double D(uint64_t b) { double v; memcpy(&v, &b, 8); return v; }
static inline long double Q(uint64_t lo, uint64_t hi) { uint64_t b[2] = { lo, hi }; long double v; memcpy(&v, b, 16); return v; }
static inline uint16_t BH(_Float16 v) { uint16_t b; memcpy(&b, &v, 2); return b; }
static inline uint32_t BF(float v) { uint32_t b; memcpy(&b, &v, 4); return b; }
static inline uint64_t BD(double v) { uint64_t b; memcpy(&b, &v, 8); return b; }
static inline uint64_t QL(long double v) { uint64_t b[2]; memcpy(b, &v, 16); return b[0]; }
static inline uint64_t QH(long double v) { uint64_t b[2]; memcpy(b, &v, 16); return b[1]; }
static volatile float ONE_F = 1.0f;
static volatile double ONE_D = 1.0;
static volatile long double ONE_Q = 1.0L;
static void out(uint64_t hi, uint64_t lo) { printf(" %016llx%016llx", (unsigned long long)hi, (unsigned long long)lo); }
static void P_h(_Float16 v) { out(0, BH(v)); }
static void P_f(float v) { out(0, BF(v)); }
static void P_d(double v) { out(0, BD(v)); }
static void P_q(long double v) { out(QH(v), QL(v)); }
static void P_w(uint32_t v) { out(0, v); }
static void P_x(uint64_t v) { out(0, v); }
static void nl(void) { putchar('\n'); }
"#;

/// The module text of `funcs`, every function prefixed by `prefix`.
fn module_text(funcs: &[Func], prefix: &str, major: u32) -> String {
    let mut text = ll_header(major);
    let decls: BTreeSet<&String> = funcs.iter().flat_map(|f| &f.decls).collect();
    for d in decls {
        text.push_str(d);
        text.push('\n');
    }
    for f in funcs {
        if prefix != "l_" || f.llc_builds {
            text.push_str(&f.text_of(prefix));
        }
    }
    text
}

/// What [`run_native`] compared.
#[derive(Debug, Default)]
pub struct Report {
    pub calls: usize,
    pub interp: usize,
    pub c: usize,
    pub model: usize,
    pub llc: usize,
    /// Results where `llc`'s NaN differs from hod's.
    pub nan_only: usize,
}

/// Runs `funcs` natively (hod plain and stressed, `llc`) with `per`
/// random argument sets each (plus the specials or their pairs), and
/// compares as the module doc says. Panics with the first differences.
pub fn run_native(label: &str, funcs: &[Func], per: usize, seed: u64) -> Report {
    let dir = hod_testutil::Scratch::new(label);
    let mut report = Report::default();
    let Some(toolchain) = Toolchain::host(&dir) else { return report };
    let mut objects = Vec::new();
    let mut hod_text = String::new();
    for (prefix, stress) in [("h_", Stress::default()), ("s_", Stress::ALL)] {
        let text = module_text(funcs, prefix, 22);
        let (m, bodies) = parse(&text);
        let emitted = emit(&m, &bodies, &Options { stress, ..Options::default() });
        objects.push(write_object(&dir, &format!("{prefix}hod.o"), &emitted));
        if prefix == "h_" {
            hod_text = text;
        }
    }
    let mut prefixes = vec!["h_", "s_"];
    if let Some(object) =
        llc(&dir, "llc", &module_text(funcs, "l_", llc_major()), &["-relocation-model=pic"])
    {
        objects.push(object);
        prefixes.push("l_");
    }
    let mut rng = Rng(seed);
    let mut calls: Vec<(usize, Vec<u128>)> = Vec::new();
    let mut c = String::from(DRIVER_HEAD);
    for f in funcs {
        let params: Vec<&str> = f.params.iter().map(|t| t.c()).collect();
        for prefix in &prefixes {
            let _ = writeln!(c, "{} {prefix}{}({});", f.ret.c(), f.name, params.join(", "));
        }
        if prefixes.len() == 3 && !f.llc_builds {
            // Its column is hod's again, not compared.
            let _ = writeln!(c, "#define l_{0} h_{0}", f.name);
        }
        if let Some(expr) = &f.c {
            let params: Vec<String> =
                f.params.iter().enumerate().map(|(i, t)| format!("{} a{i}", t.c())).collect();
            // `noipa`: gcc folds a call of constants exactly (`powi`
            // by MPFR), not as the library computes it.
            let _ = writeln!(
                c,
                "__attribute__((noipa)) static {} c_{}({}) {{ return ({}) ({expr}); }}",
                f.ret.c(),
                f.name,
                params.join(", "),
                f.ret.c()
            );
        }
    }
    for (index, f) in funcs.iter().enumerate() {
        let _ = writeln!(c, "static void t{index}(void) {{");
        for args in f.arg_sets(&mut rng, per) {
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
    let driver = toolchain.compile_c(&dir, "driver", &c, &["-ffp-contract=off"]);
    let mut inputs: Vec<&std::path::Path> = vec![&driver];
    inputs.extend(objects.iter().map(std::path::PathBuf::as_path));
    let (m, bodies) = parse(&hod_text);
    let program = Program::with_bodies(&m, &bodies).unwrap();
    let config = Config { nan: NanPolicy::Host, ..Config::default() };
    let expected: Vec<Option<u128>> = calls
        .iter()
        .map(|(index, args)| {
            let f = &funcs[*index];
            if !f.interp {
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
    for (k, linker) in toolchain.linkers.iter().enumerate() {
        let program = dir.join(format!("fpintr-{}", linker.name));
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
            if f.c.is_some() {
                report.c += 1;
                let theirs = got[got.len() - 1];
                if theirs != hod && (f.exact_c || !same_but_nan(f.ret, theirs, hod)) {
                    why.push(format!("C {theirs:x}"));
                }
            }
            if prefixes.len() == 3 && f.llc {
                report.llc += 1;
                let theirs = got[2];
                if theirs != hod {
                    if same_but_nan(f.ret, theirs, hod) {
                        report.nan_only += 1;
                    } else {
                        why.push(format!("llc {theirs:x}"));
                    }
                }
            }
            if let Some(want) = want {
                report.interp += 1;
                if *want != hod {
                    why.push(format!("interp {want:x}"));
                }
            }
            if let Some(model) = &f.model {
                report.model += 1;
                let want = model(args) & mask;
                if want != hod {
                    why.push(format!("model {want:x}"));
                }
            }
            if !why.is_empty() {
                failures.push(format!("{}({args:x?}) = {hod:x}, but {}", f.name, why.join(", ")));
            }
        }
    }
    report.calls = calls.len();
    eprintln!("{label}: {} functions, {report:?}", funcs.len());
    assert!(
        failures.is_empty(),
        "{} of {} calls differ:\n{}",
        failures.len(),
        calls.len(),
        failures.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
    report
}

// ---- models ----

/// `fptosi.sat`/`fptoui.sat` of the `half`/`float`/`double` pattern
/// `bits` to `i<width>`.
pub fn saturate(from: Ty, bits: u128, width: u32, signed: bool) -> u128 {
    let x = to_f64(from, bits);
    if x.is_nan() {
        return 0;
    }
    let t = x.trunc();
    let m = I(width).mask();
    if signed {
        let max = 2f64.powi(width as i32 - 1);
        if t >= max {
            m >> 1
        } else if t < -max {
            (m >> 1) + 1
        } else {
            (t as i64 as u128) & m
        }
    } else if t <= 0.0 {
        0
    } else if t >= 2f64.powi(width as i32) {
        m
    } else {
        u128::from(t as u64)
    }
}

/// The `is.fpclass` test bit of the pattern `bits` of `ty`: 0 signalling
/// NaN, 1 quiet NaN, 2 -inf, 3 -normal, 4 -subnormal, 5 -0, then +0,
/// +subnormal, +normal, +inf.
pub fn fp_class(ty: Ty, bits: u128) -> u32 {
    let (e, f) = ty.fields();
    let exp = (bits >> f) & ((1 << e) - 1);
    let frac = bits & ((1u128 << f) - 1);
    let negative = bits >> (ty.bits() - 1) & 1 == 1;
    let max = (1 << e) - 1;
    let kind = match (exp, frac) {
        (x, 0) if x == max => 3, // inf
        (x, _) if x == max => {
            return u32::from(frac >> (f - 1) == 1);
        }
        (0, 0) => 0,
        (0, _) => 1,
        _ => 2,
    };
    if negative { 5 - kind } else { 6 + kind }
}
