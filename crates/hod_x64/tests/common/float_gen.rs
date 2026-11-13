//! The x86-64 floating-point groups' programs (WP-127): functions of
//! integer bit patterns that make floating-point values of them (a
//! `bitcast`, or for `fp128` a store of its two halves and a load), run
//! one operation, and return the result's bits (an `fp128` folded into 64
//! bits). The same `.ll` text goes to `hod_interp`, to `hod_x64` (plainly
//! and with every stress option) and to `llc -O0 -fast-isel=0` of LLVM 22
//! and 19, all run from one C driver under the emulator.
//!
//! What is compared ([`run`]):
//!
//! - hod stressed with hod plain, bit for bit;
//! - hod with `llc` 22 and 19, bit for bit (NaN payloads included: the
//!   two run the same SSE2 instructions and the same routines), for every
//!   function marked [`Func::llc`];
//! - hod with the interpreter (`NanPolicy::Host`) for the functions of
//!   `float`, `double` and integers ([`Check::Interp`]): bit for bit, but
//!   NaN for NaN where the result is a NaN (the host is AArch64, whose
//!   default NaN is positive; x86's is negative). A case the interpreter
//!   traps on (an out-of-range conversion: poison) is not run.
//!
//! `half`, `bfloat` and `fp128` have no interpreter ([`Check::Llc`]):
//! `llc` is their oracle, and their cases keep conversions in range.

#![allow(clippy::many_single_char_names, clippy::too_many_lines, clippy::cast_precision_loss)]
#![allow(dead_code, reason = "each test uses part of it")]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hod_cg::{Options, RelocModel, Stress};
use hod_interp::{Config, Interp, NanPolicy, Program, Val};
use hod_testutil::{Scratch, run_with, tool};
use hod_x64::compile_to_object;

use crate::linking::Toolchain;

/// The header of a `.ll` module for x86-64 Linux.
pub fn ll_header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// A module and its bodies from `.ll` text (target lines included).
pub fn parse(text: &str) -> (hod_ir::Module, Vec<hod_ir::Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        let body =
            parsed.body.unwrap_or_else(|| panic!("hod_ll: unsupported {:?}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, bodies)
}

/// Position-independent code, as every x86-64 test links it (PIE).
pub fn pic(stress: Stress) -> Options {
    Options { reloc_model: RelocModel::Pic, stress, ..Options::default() }
}

/// `llc` of LLVM `major`, if installed.
pub fn llc_of(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if installed.is_file() {
        return Some(installed);
    }
    hod_testutil::find_tool(&format!("llc-{major}"))
}

/// `llc -O0 -fast-isel=0 -relocation-model=pic` for x86-64 of `text` to
/// an object in `dir`; `None` without that `llc`.
pub fn llc(dir: &Scratch, major: u32, name: &str, text: &str) -> Option<PathBuf> {
    let llc = llc_of(major)?;
    let input = dir.write(&format!("{name}.ll"), text);
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args([
            "-O0",
            "-fast-isel=0",
            "-mtriple=x86_64-unknown-linux-gnu",
            "-relocation-model=pic",
            "-filetype=obj",
        ])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

/// The size of `.text` in `object`, by `llvm-objdump -h`.
pub fn text_size(object: &Path) -> usize {
    let Some(objdump) = tool("llvm-objdump") else { return 0 };
    let mut command = Command::new(objdump);
    command.arg("-h").arg(object);
    let out = run_with(&mut command, &[], Duration::from_mins(1));
    out.stdout_text()
        .lines()
        .find_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields.get(1) == Some(&".text"))
                .then(|| usize::from_str_radix(fields[2], 16).unwrap_or(0))
        })
        .unwrap_or(0)
}

// ---- types ----

/// A scalar type of the tests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T {
    /// `half`.
    H,
    /// `bfloat`.
    B,
    /// `float`.
    F,
    /// `double`.
    D,
    /// `fp128`.
    Q,
    /// `iN`.
    I(u32),
}

impl T {
    /// The type in `.ll` text.
    pub fn ll(self) -> String {
        match self {
            T::H => "half".into(),
            T::B => "bfloat".into(),
            T::F => "float".into(),
            T::D => "double".into(),
            T::Q => "fp128".into(),
            T::I(bits) => format!("i{bits}"),
        }
    }

    /// The width of its bits (`fp128`: 128).
    pub fn bits(self) -> u32 {
        match self {
            T::H | T::B => 16,
            T::F => 32,
            T::D => 64,
            T::Q => 128,
            T::I(bits) => bits,
        }
    }

    /// The `i64` parameters one operand of it takes (an `fp128` two).
    fn params(self) -> usize {
        if self == T::Q { 2 } else { 1 }
    }

    /// Whether `bits` (of this type) is a NaN.
    pub fn is_nan(self, bits: u64) -> bool {
        let (exp, frac) = match self {
            T::H => (0x7c00, 0x3ff),
            T::B => (0x7f80, 0x7f),
            T::F => (0x7f80_0000, 0x7f_ffff),
            T::D => (0x7ff0_0000_0000_0000, 0xf_ffff_ffff_ffff),
            _ => return false,
        };
        bits & exp == exp && bits & frac != 0
    }

    /// Whether this is a floating-point type.
    pub fn is_fp(self) -> bool {
        !matches!(self, T::I(_))
    }
}

/// The low `bits` bits.
pub fn mask(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1 << bits) - 1 }
}

/// Who checks a function's results besides hod itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    /// The interpreter (and `llc`, if [`Func::llc`]).
    Interp,
    /// `llc` only.
    Llc,
}

/// A function of a suite.
#[derive(Clone, Debug)]
pub struct Func {
    /// Its name.
    pub name: String,
    /// Its operand types (each one or two `i64` parameters).
    pub ops: Vec<T>,
    /// Its result type.
    pub ret: T,
    /// Who checks it.
    pub check: Check,
    /// Whether `llc`'s bits must equal hod's.
    pub llc: bool,
    /// The oldest LLVM whose `llc` knows it.
    pub since: u32,
    /// Whether a NaN for a NaN matches `llc`'s result: the IR leaves the
    /// NaN open (the LLVM language reference, "Behavior of Floating-Point
    /// NaN values": a quieted operand NaN or the preferred NaN) and
    /// `llc`'s comes from code the two do not share (a C library routine
    /// where hod computes inline), so the hardware may pick another
    /// operand's.
    pub nan_open: bool,
    /// An exact model of the result from the arguments, compared with
    /// hod plain where `llc` is no oracle.
    pub model: Option<fn(&[u64]) -> u64>,
    /// Where its text is in [`Suite::text`].
    pub text: std::ops::Range<usize>,
}

/// A function called with arguments: the bits of its operands (an
/// `fp128` as two words, low first).
#[derive(Clone, Debug)]
pub struct Case {
    /// Index into [`Suite::funcs`].
    pub func: usize,
    /// The `i64` arguments.
    pub args: [u64; 6],
}

/// Functions as `.ll` text and the cases to run them with.
#[derive(Default, Debug)]
pub struct Suite {
    /// The declarations the functions call (module level).
    pub decls: String,
    /// The functions' text (without the module header).
    pub text: String,
    /// The functions, in text order.
    pub funcs: Vec<Func>,
    /// The cases.
    pub cases: Vec<Case>,
}

impl Suite {
    /// Adds the declaration `line` (once).
    pub fn declare(&mut self, line: &str) {
        if !self.decls.contains(line) {
            self.decls.push_str(line);
            self.decls.push('\n');
        }
    }

    /// Adds a function of operands `ops` whose `body` computes `%r` of
    /// type `ret` from `%a0`, `%a1`, ... (it may add blocks; it ends in
    /// the block that has `%r`), checked by `check`. Returns its index.
    pub fn add(&mut self, name: &str, ops: &[T], ret: T, body: &str, check: Check) -> usize {
        let mut params = Vec::new();
        let mut entry = String::new();
        let mut k = 0;
        for (i, &ty) in ops.iter().enumerate() {
            let p = format!("%p{k}");
            match ty {
                T::Q => {
                    params.push(format!("i64 {p}"));
                    params.push(format!("i64 %p{}", k + 1));
                    write!(
                        entry,
                        "  %s{i} = alloca [2 x i64], align 16\n  store i64 {p}, ptr %s{i}\n  \
                         %s{i}h = getelementptr i8, ptr %s{i}, i64 8\n  \
                         store i64 %p{}, ptr %s{i}h\n  %a{i} = load fp128, ptr %s{i}\n",
                        k + 1
                    )
                    .unwrap();
                }
                T::D => {
                    params.push(format!("i64 {p}"));
                    writeln!(entry, "  %a{i} = bitcast i64 {p} to double").unwrap();
                }
                T::I(64) => {
                    params.push(format!("i64 {p}"));
                    writeln!(entry, "  %a{i} = add i64 {p}, 0").unwrap();
                }
                T::I(bits) => {
                    params.push(format!("i64 {p}"));
                    writeln!(entry, "  %a{i} = trunc i64 {p} to i{bits}").unwrap();
                }
                fp => {
                    params.push(format!("i64 {p}"));
                    let w = fp.bits();
                    writeln!(
                        entry,
                        "  %t{i} = trunc i64 {p} to i{w}\n  %a{i} = bitcast i{w} %t{i} to {}",
                        fp.ll()
                    )
                    .unwrap();
                }
            }
            k += ty.params();
        }
        assert!(k <= 6, "{name}: too many parameters");
        let tail = match ret {
            T::Q => {
                entry.push_str("  %rs = alloca fp128, align 16\n");
                "  store fp128 %r, ptr %rs\n  %lo = load i64, ptr %rs\n  \
                 %hp = getelementptr i8, ptr %rs, i64 8\n  %hi = load i64, ptr %hp\n  \
                 %hm = mul i64 %hi, -7046029254386353131\n  %out = xor i64 %hm, %lo\n  \
                 ret i64 %out\n"
                    .to_string()
            }
            T::D => "  %out = bitcast double %r to i64\n  ret i64 %out\n".to_string(),
            T::I(64) => "  ret i64 %r\n".to_string(),
            T::I(bits) => format!("  %out = zext i{bits} %r to i64\n  ret i64 %out\n"),
            fp => {
                let w = fp.bits();
                format!(
                    "  %rb = bitcast {} %r to i{w}\n  %out = zext i{w} %rb to i64\n  ret i64 %out\n",
                    fp.ll()
                )
            }
        };
        let start = self.text.len();
        writeln!(
            self.text,
            "define i64 @{name}({}) {{\n{entry}{body}{tail}}}\n",
            params.join(", ")
        )
        .unwrap();
        let text = start..self.text.len();
        self.funcs.push(Func {
            name: name.to_string(),
            ops: ops.to_vec(),
            ret,
            check,
            llc: true,
            since: 19,
            nan_open: false,
            model: None,
            text,
        });
        self.funcs.len() - 1
    }

    /// Adds a case of function `func` with one value per operand (an
    /// `fp128` as `(low, high)` in two consecutive words of `values`).
    pub fn case(&mut self, func: usize, values: &[u64]) {
        let mut args = [0; 6];
        args[..values.len()].copy_from_slice(values);
        self.cases.push(Case { func, args });
    }
}

/// `SplitMix64`.
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Rng {
    /// The next 64 random bits.
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform below `n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// One element of `items`.
    pub fn pick<'t, X>(&mut self, items: &'t [X]) -> &'t X {
        &items[self.below(items.len() as u64) as usize]
    }
}

// ---- values ----

/// The `fp128` bits (low, high) of a `double` (exact).
pub fn quad_of(d: f64) -> (u64, u64) {
    let bits = d.to_bits();
    let sign = bits >> 63;
    let exp = (bits >> 52) & 0x7ff;
    let frac = bits & 0xf_ffff_ffff_ffff;
    let (qexp, qfrac) = if exp == 0x7ff {
        (0x7fff, u128::from(frac) << 60)
    } else if exp == 0 {
        if frac == 0 {
            (0, 0)
        } else {
            // A subnormal double: normalized in fp128.
            let shift = frac.leading_zeros() - 11;
            let m = (frac << shift) & 0xf_ffff_ffff_ffff;
            (15361 - u64::from(shift), u128::from(m) << 60)
        }
    } else {
        (exp + 15360, u128::from(frac) << 60)
    };
    let q = (u128::from(sign) << 127) | (u128::from(qexp) << 112) | qfrac;
    (q as u64, (q >> 64) as u64)
}

/// The edge values of a floating-point type: zeros, ones, halves and the
/// ties of the roundings, the subnormal and normal boundaries, the
/// largest finite values, infinities, quiet and signalling NaNs with
/// payloads and both signs, and the conversion boundaries `±2^31`,
/// `2^32`, `±2^63`, `2^64` with their neighbours. `fp128` as pairs.
pub fn edges(ty: T) -> Vec<u64> {
    let f = |x: f32| u64::from(x.to_bits());
    let d = |x: f64| x.to_bits();
    let mut out: Vec<u64> = match ty {
        T::H => vec![
            0, 0x8000, 0x3c00, 0xbc00, 0x3800, 0x3e00, 0x4100, 0xc100, 0x0001, 0x8001, 0x03ff,
            0x0400, 0x7bff, 0xfbff, 0x7c00, 0xfc00, 0x7e00, 0x7e01, 0xfe00, 0x7c01, 0x7d55, 0x5800,
            0xd800, 0x57ff, 0x7800, 0x77ff,
        ],
        T::B => vec![
            0, 0x8000, 0x3f80, 0xbf80, 0x3f00, 0x3fc0, 0x4020, 0xc020, 0x0001, 0x8001, 0x007f,
            0x0080, 0x7f7f, 0xff7f, 0x7f80, 0xff80, 0x7fc0, 0x7fc1, 0xffc0, 0x7f81, 0x7fa5, 0x4f00,
            0xcf00, 0x4eff, 0x5f00, 0x5eff,
        ],
        T::F => {
            let mut v = vec![
                f(0.0),
                f(-0.0),
                f(1.0),
                f(-1.0),
                f(0.5),
                f(1.5),
                f(2.5),
                f(-2.5),
                f(-0.5),
                f(127.5),
                f(-128.5),
                f(255.75),
                f(65535.5),
                1,
                0x8000_0001,
                0x007f_ffff,
                0x0080_0000,
                0x7f7f_ffff,
                0xff7f_ffff,
                0x7f80_0000,
                0xff80_0000,
                0x7fc0_0000,
                0x7fc1_2345,
                0xffc0_0001,
                0x7f80_0001,
                0x7fa0_0000,
                0xffa0_0003,
                f(16_777_217.0),
                f(-1e-30),
            ];
            for p in [31, 32, 63, 64] {
                let base = (127 + p) << 23;
                v.extend([base - 1, base, base + 1, base | 1 << 31, (base - 1) | 1 << 31]);
            }
            v
        }
        T::D => {
            let mut v = vec![
                d(0.0),
                d(-0.0),
                d(1.0),
                d(-1.0),
                d(0.5),
                d(1.5),
                d(2.5),
                d(-2.5),
                d(-0.5),
                d(127.5),
                d(-128.5),
                d(255.75),
                d(65535.5),
                d(4_294_967_295.5),
                d(-2_147_483_648.5),
                1,
                0x8000_0000_0000_0001,
                0x000f_ffff_ffff_ffff,
                0x0010_0000_0000_0000,
                0x7fef_ffff_ffff_ffff,
                0xffef_ffff_ffff_ffff,
                0x7ff0_0000_0000_0000,
                0xfff0_0000_0000_0000,
                0x7ff8_0000_0000_0000,
                0x7ff8_1234_5678_9abc,
                0xfff8_0000_0000_0001,
                0x7ff0_0000_0000_0001,
                0x7ff4_0000_0000_0000,
                0x47ef_ffff_e000_0000,
                0x47ef_ffff_f000_0000,
                0x3690_0000_0000_0000,
                0x36a0_0000_0000_0000,
                d(1.000_000_059_604_644_8),
                d(16_777_217.0),
                d(9_007_199_254_740_993.0),
            ];
            for p in [31, 32, 63, 64] {
                let base = (1023 + p) << 52;
                v.extend([base - 1, base, base + 1, base | 1 << 63, (base - 1) | 1 << 63]);
            }
            v
        }
        T::Q => {
            let mut v = Vec::new();
            for x in [
                0.0,
                -0.0,
                1.0,
                -1.0,
                0.5,
                2.5,
                -2.5,
                1e300,
                -1e-300,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::MIN_POSITIVE,
                5e-324,
                2_147_483_648.0,
                -2_147_483_648.0,
                4_294_967_295.0,
                9_223_372_036_854_775_808.0,
                18_446_744_073_709_551_615.0,
            ] {
                let (lo, hi) = quad_of(x);
                v.extend([lo, hi]);
            }
            v.extend([
                0,
                0x7fff_8000_0000_0000,
                5,
                0x7fff_8000_0000_1234,
                1,
                0x7fff_0000_0000_0000,
            ]);
            v.extend([0, 0xffff_8000_0000_0000, 1, 0x0000_0000_0000_0000]);
            v.extend([0x1234_5678_9abc_def1, 0x3fff_8000_0000_0001]);
            v.extend([u64::MAX, 0x0000_ffff_ffff_ffff, 0, 0x0001_0000_0000_0000]);
            v
        }
        T::I(bits) => {
            let m = mask(bits);
            let mut v = vec![0, 1, 2, m, m - 1, m >> 1, (m >> 1) + 1, (m >> 1).wrapping_sub(1)];
            for k in [11, 12, 24, 25, 31, 32, 52, 53, 54, 62, 63] {
                if k < bits {
                    v.extend([
                        1 << k,
                        (1 << k) + 1,
                        (1 << k) - 1,
                        ((1u64 << k) + 1).wrapping_neg(),
                    ]);
                }
            }
            v.iter().map(|x| x & m).collect()
        }
    };
    if ty != T::Q {
        let mut seen = std::collections::HashSet::new();
        out.retain(|x| seen.insert(*x));
    }
    out
}

/// A random value of `ty` (an `fp128` as two words): random bits, with a
/// bias to small exponents.
pub fn random(rng: &mut Rng, ty: T) -> Vec<u64> {
    match ty {
        T::Q => {
            let hi = rng.next();
            let near =
                0x3fff_0000_0000_0000 + ((rng.below(256) << 48) ^ (hi & 0x8000_ffff_ffff_ffff));
            vec![rng.next(), if rng.below(4) == 0 { hi } else { near }]
        }
        T::I(bits) => vec![rng.next() & mask(bits)],
        T::H | T::B => vec![rng.next() & 0xffff],
        T::F => {
            let x = rng.next() as u32;
            let x = if rng.below(2) == 0 {
                x
            } else {
                (x & 0x807f_ffff) | (rng.below(64) as u32 + 100) << 23
            };
            vec![u64::from(x)]
        }
        T::D => {
            let x = rng.next();
            vec![if rng.below(2) == 0 {
                x
            } else {
                (x & 0x800f_ffff_ffff_ffff) | (rng.below(128) + 990) << 52
            }]
        }
    }
}

/// The edge values of `ty` as operands (one or two words each).
pub fn operands(ty: T) -> Vec<Vec<u64>> {
    let e = edges(ty);
    if ty == T::Q {
        e.chunks(2).map(<[u64]>::to_vec).collect()
    } else {
        e.into_iter().map(|x| vec![x]).collect()
    }
}

/// Cases of `func` (operands `ops`): every pair (or single) of edge
/// values when there are at most `limit` combinations, a random sample
/// of them otherwise, then `extra` random sets.
pub fn cases(suite: &mut Suite, rng: &mut Rng, func: usize, ops: &[T], limit: usize, extra: usize) {
    let lists: Vec<Vec<Vec<u64>>> = ops.iter().map(|&t| operands(t)).collect();
    let total: usize = lists.iter().map(Vec::len).product();
    let add = |suite: &mut Suite, choice: &[&Vec<u64>]| {
        let values: Vec<u64> = choice.iter().flat_map(|v| v.iter().copied()).collect();
        suite.case(func, &values);
    };
    if total <= limit {
        let mut index = vec![0; lists.len()];
        'all: loop {
            let choice: Vec<&Vec<u64>> = index.iter().zip(&lists).map(|(&i, l)| &l[i]).collect();
            add(suite, &choice);
            for k in (0..index.len()).rev() {
                index[k] += 1;
                if index[k] < lists[k].len() {
                    continue 'all;
                }
                index[k] = 0;
            }
            break;
        }
    } else {
        for _ in 0..limit {
            let choice: Vec<&Vec<u64>> = lists.iter().map(|l| rng.pick(l)).collect();
            add(suite, &choice);
        }
    }
    for _ in 0..extra {
        let values: Vec<u64> = ops.iter().flat_map(|&t| random(rng, t)).collect();
        suite.case(func, &values);
    }
}

// ---- running a suite ----

/// The C driver: every case that is defined, run in order, each result
/// printed in hex.
fn driver(suite: &Suite, defined: &[bool]) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         typedef uint64_t u;\ntypedef u (*fn_t)(u, u, u, u, u, u);\n",
    );
    for f in &suite.funcs {
        writeln!(c, "u {}(u, u, u, u, u, u);", f.name).unwrap();
    }
    c.push_str("static const struct { fn_t f; u a[6]; } cases[] = {\n");
    for (case, _) in suite.cases.iter().zip(defined).filter(|(_, d)| **d) {
        let a = case.args;
        writeln!(
            c,
            "  {{ {}, {{ {:#x}ULL, {:#x}ULL, {:#x}ULL, {:#x}ULL, {:#x}ULL, {:#x}ULL }} }},",
            suite.funcs[case.func].name, a[0], a[1], a[2], a[3], a[4], a[5]
        )
        .unwrap();
    }
    c.push_str(
        "  { 0, { 0 } }\n};\nint main(void) {\n  for (unsigned long i = 0; cases[i].f; i++) {\n    \
         const u *a = cases[i].a;\n    \
         printf(\"%llx\\n\", (unsigned long long)cases[i].f(a[0], a[1], a[2], a[3], a[4], a[5]));\n  \
         }\n  return 0;\n}\n",
    );
    c
}

/// What [`run`] found.
#[derive(Debug, Default)]
pub struct Report {
    /// Cases run.
    pub compared: usize,
    /// Cases compared with the interpreter.
    pub interpreted: usize,
    /// Cases the interpreter trapped on (poison; not run).
    pub undefined: usize,
    /// Disagreements.
    pub wrong: Vec<String>,
    /// `.text` bytes of hod (plain) and `llc` 22.
    pub hod_bytes: usize,
    /// `llc` 22's `.text` bytes (0 without it).
    pub llc_bytes: usize,
    /// IR instructions of the suite.
    pub insts: usize,
    /// The legs that ran.
    pub legs: Vec<String>,
    /// Time the whole run took.
    pub time: Duration,
}

/// The interpreter's result of each case (`None` where it traps, or the
/// function is not [`Check::Interp`]), and whether the case is defined.
fn interpret(suite: &Suite) -> (Vec<Option<u64>>, Vec<bool>) {
    let mut expected = vec![None; suite.cases.len()];
    let mut defined = vec![true; suite.cases.len()];
    let mut by_func: Vec<Vec<usize>> = vec![Vec::new(); suite.funcs.len()];
    for (index, case) in suite.cases.iter().enumerate() {
        by_func[case.func].push(index);
    }
    let config = Config { nan: NanPolicy::Host, ..Config::default() };
    for (chunk, funcs) in suite.funcs.chunks(64).enumerate() {
        if funcs.iter().all(|f| f.check != Check::Interp) {
            continue;
        }
        let mut text = ll_header();
        text.push_str(&suite.decls);
        for f in funcs.iter().filter(|f| f.check == Check::Interp) {
            text.push_str(&suite.text[f.text.clone()]);
        }
        let (module, bodies) = parse(&text);
        let program = Program::with_bodies(&module, &bodies).expect("a valid program");
        for (offset, f) in funcs.iter().enumerate() {
            if f.check != Check::Interp {
                continue;
            }
            let sym = module.find_symbol(f.name.as_bytes()).expect("a defined function");
            let words: usize = f.ops.iter().map(|t| t.params()).sum();
            for &index in &by_func[chunk * 64 + offset] {
                let args: Vec<Val> = suite.cases[index].args[..words]
                    .iter()
                    .map(|&v| Val::int(64, u128::from(v)))
                    .collect();
                match Interp::new(&program).with_config(config).run(sym, &args).result {
                    Ok(Some(Val::Int { value, .. })) => expected[index] = Some(value as u64),
                    Ok(other) => panic!("{}: {other:?}", f.name),
                    Err(hod_interp::Stop::Trap(_)) => defined[index] = false,
                    Err(e) => panic!("{}: the interpreter cannot run it: {e:?}", f.name),
                }
            }
        }
    }
    (expected, defined)
}

/// `llvm-objdump -d -M intel` of `functions` (the header added),
/// compiled with `stress`: one line per instruction, `mnemonic
/// operands`. `None` without the tool.
pub fn disassembly(functions: &str, stress: Stress) -> Option<Vec<String>> {
    let objdump = tool("llvm-objdump")?;
    let (m, bodies) = parse(&format!("{}{functions}", ll_header()));
    let object = compile_to_object(&m, &bodies, &pic(stress)).unwrap_or_else(|e| panic!("{e}"));
    let scratch = Scratch::new("x64-float-disasm");
    let path = scratch.write("f.o", object);
    let mut command = Command::new(objdump);
    command.args(["-d", "-M", "intel", "--no-show-raw-insn", "--no-leading-addr"]).arg(&path);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    Some(
        outcome
            .stdout_text()
            .lines()
            .filter(|line| line.starts_with(char::is_whitespace) && !line.trim().is_empty())
            .map(|line| {
                let line = line.split('#').next().unwrap_or(line);
                line.split_whitespace().collect::<Vec<_>>().join(" ")
            })
            .collect(),
    )
}

pub fn mnemonic(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or("")
}

/// Whether mnemonic `m` needs more than SSE2 (SSE4.1's `round*`,
/// `blend*`, `ptest`, `pextrd`/`pinsrd`; AVX's `v*`) or is x87.
pub fn above_sse2(m: &str) -> bool {
    m.starts_with("round")
        || m.starts_with('v')
        || m.contains("blend")
        || m.starts_with("ptest")
        || m.starts_with('f') && !m.starts_with("fn")
        || m == "pextrd"
        || m == "pinsrd"
}

/// Panics if hod's plain code for `suite` has an instruction above SSE2.
pub fn assert_sse2(suite: &Suite) {
    let text = format!("{}{}", suite.decls, suite.text);
    let Some(lines) = disassembly(&text, Stress::default()) else { return };
    for line in &lines {
        assert!(!above_sse2(mnemonic(line)), "{line}");
    }
}

/// Whether `got` matches the interpreter's `want` for a result of `ret`:
/// equal, or both NaNs.
fn matches_interp(ret: T, want: u64, got: u64) -> bool {
    want == got || (ret.is_nan(want) && ret.is_nan(got))
}

/// Checks `suite` (see the module documentation). `name` labels the
/// scratch directory; `majors` are the `llc`s to compare with. Without
/// the cross toolchain only the compile (and the interpreter) runs.
pub fn run(name: &str, suite: &Suite, majors: &[u32]) -> Report {
    let start = Instant::now();
    let text = format!("{}{}{}", ll_header(), suite.decls, suite.text);
    let (module, bodies) = parse(&text);
    let mut report =
        Report { insts: bodies.iter().map(hod_ir::Body::inst_count).sum(), ..Report::default() };
    let (expected, defined) = interpret(suite);
    report.undefined = defined.iter().filter(|d| !**d).count();
    report.compared = defined.len() - report.undefined;
    report.interpreted = expected.iter().filter(|e| e.is_some()).count();

    let scratch = Scratch::new(&format!("x64-float-{name}"));
    let mut legs: Vec<(String, PathBuf)> = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{mode}: {e}"));
        let path = scratch.write(&format!("hod-{mode}.o"), object);
        if mode == "plain" {
            report.hod_bytes = text_size(&path);
        }
        legs.push((format!("hod {mode}"), path));
    }
    for &major in majors {
        if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &llc_text(suite, major))
        {
            if major == 22 {
                report.llc_bytes = text_size(&object);
            }
            legs.push((format!("llc {major}"), object));
        }
    }
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return report };
    let Some(linker) = toolchain.linkers.first() else { return report };
    let driver = toolchain.compile_c(&scratch, "driver", &driver(suite, &defined), &[]);
    let index: Vec<usize> = (0..suite.cases.len()).filter(|&i| defined[i]).collect();
    let mut plain: Vec<u64> = Vec::new();
    for (leg, object) in &legs {
        let program = scratch.join(leg.replace(' ', "-"));
        let lm = Path::new("-lm");
        let ran = toolchain.link_and_run(linker, &[], &[&driver, object, lm], &program);
        assert!(ran.success(), "{leg}: {:?}\n{}", ran.status, ran.stderr_text());
        let got: Vec<u64> = ran
            .stdout_text()
            .lines()
            .map(|line| u64::from_str_radix(line, 16).unwrap_or_else(|_| panic!("{leg}: {line}")))
            .collect();
        assert_eq!(got.len(), index.len(), "{leg}: one line per defined case");
        for (&i, &got) in index.iter().zip(&got) {
            let case = &suite.cases[i];
            let f = &suite.funcs[case.func];
            let args = &case.args[..f.ops.iter().map(|t| t.params()).sum::<usize>()];
            let mut wrong = |what: &str, want: u64| {
                report
                    .wrong
                    .push(format!("{leg}: {}({args:#x?}): {what} {want:#x}, got {got:#x}", f.name));
            };
            if leg == "hod plain" {
                if let Some(want) = expected[i]
                    && !matches_interp(f.ret, want, got)
                {
                    wrong("interpreter", want);
                }
                if let Some(model) = f.model
                    && model(args) != got
                {
                    wrong("model", model(args));
                }
            } else if (leg == "hod stress" || compared_by_llc(f, leg))
                && plain[report_index(&index, i)] != got
                && !(leg != "hod stress"
                    && f.nan_open
                    && f.ret.is_nan(plain[report_index(&index, i)])
                    && f.ret.is_nan(got))
            {
                wrong("hod plain", plain[report_index(&index, i)]);
            }
        }
        if leg == "hod plain" {
            plain = got;
        }
        report.legs.push(leg.clone());
    }
    report.time = start.elapsed();
    report
}

/// Whether leg `leg` (`llc <major>`) compiles `f` and compares it.
fn compared_by_llc(f: &Func, leg: &str) -> bool {
    let major = leg.strip_prefix("llc ").and_then(|m| m.parse::<u32>().ok());
    f.llc && major.is_some_and(|m| f.since <= m)
}

/// The module `llc` `major` compiles: the functions it compares, a stub
/// returning 0 for each other one (so that the driver links), the
/// declarations they call.
fn llc_text(suite: &Suite, major: u32) -> String {
    let mut body = String::new();
    for f in &suite.funcs {
        if f.llc && f.since <= major {
            body.push_str(&suite.text[f.text.clone()]);
        } else {
            let words: usize = f.ops.iter().map(|t| t.params()).sum();
            let params: Vec<String> = (0..words).map(|k| format!("i64 %p{k}")).collect();
            let _ = write!(
                body,
                "define i64 @{}({}) {{\n  ret i64 0\n}}\n\n",
                f.name,
                params.join(", ")
            );
        }
    }
    let decls: String = suite
        .decls
        .lines()
        .filter(|line| {
            line.split_once('@')
                .and_then(|(_, rest)| rest.split_once('('))
                .is_none_or(|(callee, _)| body.contains(&format!("@{callee}(")))
        })
        .fold(String::new(), |mut decls, line| {
            let _ = writeln!(decls, "{line}");
            decls
        });
    format!("{}{decls}{body}", ll_header())
}

/// The position of case `i` among the defined cases.
fn report_index(index: &[usize], i: usize) -> usize {
    index.binary_search(&i).unwrap_or(0)
}

/// The body of function `name` in `suite`'s text, for a failure message.
pub fn body_of<'s>(suite: &'s Suite, name: &str) -> &'s str {
    let start = suite.text.find(&format!("@{name}(")).unwrap_or(0);
    let end = suite.text[start..].find("\n}\n").map_or(suite.text.len(), |e| start + e + 3);
    &suite.text[start..end]
}

/// Prints what a suite's run measured, then panics with the first
/// disagreements, with their functions' text.
pub fn check(name: &str, suite: &Suite, majors: &[u32]) -> Report {
    let report = run(name, suite, majors);
    eprintln!(
        "x64 float {name}: {} functions, {} cases run on {:?} ({} against the interpreter, \
         {} poison not run), {} IR instructions; .text hod {} bytes, llc 22 {} (hod/llc {:.2}); {:?}",
        suite.funcs.len(),
        report.compared,
        report.legs,
        report.interpreted,
        report.undefined,
        report.insts,
        report.hod_bytes,
        report.llc_bytes,
        report.hod_bytes as f64 / report.llc_bytes.max(1) as f64,
        report.time,
    );
    if !report.wrong.is_empty() {
        let mut message = format!("{} disagreements; the first:\n", report.wrong.len());
        for line in report.wrong.iter().take(12) {
            let name = line.split(": ").nth(1).and_then(|s| s.split('(').next()).unwrap_or("");
            writeln!(message, "{line}\n{}", body_of(suite, name)).unwrap();
        }
        panic!("{message}");
    }
    report
}
