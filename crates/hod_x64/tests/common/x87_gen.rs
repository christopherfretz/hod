//! The `x86_fp80` group's programs (WP-131), on the model of
//! `float_gen.rs`: functions of `i64` words that build their operands
//! (an `x86_fp80` from two words, the significand and the sign and
//! exponent, through `i80`), run the operations under test, and return
//! the result's bits as an `i128` (an `x86_fp80` as its 80 bits). The
//! same `.ll` text goes to `hod_interp`, to `hod_x64` (plainly and with
//! every stress option) and to `llc -O0 -fast-isel=0` of LLVM 22 and 19,
//! all run from one C driver under the emulator.
//!
//! What is compared ([`run`]):
//!
//! - hod stressed with hod plain, bit for bit;
//! - hod with `llc` 22 and 19, bit for bit, NaN payloads included (the
//!   two run the same x87 instructions and call the same routines), for
//!   the functions marked [`Func::llc`] from [`Func::since`];
//! - hod with the interpreter (`NanPolicy::Host`, exact for `x86_fp80`)
//!   for the functions marked [`Check::Interp`]: bit for bit, but NaN for
//!   NaN where the result is a NaN. A case the interpreter traps on (an
//!   out-of-range conversion: poison) or refuses is not run.

#![allow(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::similar_names
)]
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
    let line = |e: &dyn std::fmt::Display| {
        let at = e.to_string();
        let n: usize = at.split(':').next().and_then(|n| n.parse().ok()).unwrap_or(0);
        text.lines().nth(n.saturating_sub(1)).unwrap_or("").to_string()
    };
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}: {}", line(&e)));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        let body =
            parsed.body.unwrap_or_else(|| panic!("hod_ll: unsupported {:?}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}: {}", line(&e)));
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

/// A type of the tests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T {
    /// `x86_fp80`: two words, the significand and the sign and exponent
    /// (the low 16 bits).
    X,
    /// `half`.
    H,
    /// `bfloat`.
    B,
    /// `float`.
    F,
    /// `double`.
    D,
    /// `fp128`: two words, low first.
    Q,
    /// `iN` (`N` up to 128; above 64 two words, low first).
    I(u32),
}

impl T {
    /// The type in `.ll` text.
    pub fn ll(self) -> String {
        match self {
            T::X => "x86_fp80".into(),
            T::H => "half".into(),
            T::B => "bfloat".into(),
            T::F => "float".into(),
            T::D => "double".into(),
            T::Q => "fp128".into(),
            T::I(bits) => format!("i{bits}"),
        }
    }

    /// The width of its bits.
    pub fn bits(self) -> u32 {
        match self {
            T::X => 80,
            T::H | T::B => 16,
            T::F => 32,
            T::D => 64,
            T::Q => 128,
            T::I(bits) => bits,
        }
    }

    /// The `i64` parameters one operand of it takes.
    pub fn params(self) -> usize {
        if self.bits() > 64 { 2 } else { 1 }
    }

    /// Whether `bits` (of this type) is a NaN.
    pub fn is_nan(self, bits: u128) -> bool {
        let (exp, frac): (u128, u128) = match self {
            T::H => (0x7c00, 0x3ff),
            T::B => (0x7f80, 0x7f),
            T::F => (0x7f80_0000, 0x7f_ffff),
            T::D => (0x7ff0_0000_0000_0000, 0xf_ffff_ffff_ffff),
            T::X => (0x7fff << 64, 0x7fff_ffff_ffff_ffff),
            T::Q => (0x7fff << 112, (1 << 112) - 1),
            T::I(_) => return false,
        };
        bits & exp == exp && bits & frac != 0
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
    /// Its operand types.
    pub ops: Vec<T>,
    /// Its result type.
    pub ret: T,
    /// Who checks it.
    pub check: Check,
    /// Whether `llc`'s bits must equal hod's.
    pub llc: bool,
    /// The oldest LLVM whose `llc` compiles it.
    pub since: u32,
    /// The function calls the C library, which computes on the invalid
    /// encodings (unnormals, pseudo-infinities, pseudo-NaNs) and on
    /// signalling NaNs as integers, not as the x87 does: the
    /// interpreter, which models the x87, is not compared on those.
    pub libm: bool,
    /// `llc` may rewrite the function so that a NaN result has another
    /// sign or payload (`fadd (fneg x), y` to `fsub y, x`): a NaN for a
    /// NaN is enough.
    pub llc_nan: bool,
    /// Where its text is in [`Suite::text`].
    pub text: std::ops::Range<usize>,
}

impl Func {
    /// The `i64` parameters of the function.
    pub fn words(&self) -> usize {
        self.ops.iter().map(|t| t.params()).sum()
    }
}

/// A function called with arguments.
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
    /// The declarations and globals the functions use (module level).
    pub decls: String,
    /// The functions' text (without the module header).
    pub text: String,
    /// The functions, in text order.
    pub funcs: Vec<Func>,
    /// The cases.
    pub cases: Vec<Case>,
    /// C the driver also compiles: functions the IR calls.
    pub c: String,
    /// Functions the suite's functions call, at module level; [`run`]
    /// also compiles them apart from their callers, by hod and by `llc`
    /// 22, and links each way round (a mixed object).
    pub callees: String,
    /// The declarations of [`Suite::callees`], for the callers' module
    /// when they are apart.
    pub externs: String,
}

impl Suite {
    /// Adds the module-level line `line` (once).
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
            params.push(format!("i64 {p}"));
            if ty.params() == 2 {
                let q = format!("%p{}", k + 1);
                params.push(format!("i64 {q}"));
                let w = ty.bits();
                write!(
                    entry,
                    "  %l{i} = zext i64 {p} to i128\n  %h{i} = zext i64 {q} to i128\n  \
                     %hs{i} = shl i128 %h{i}, 64\n  %w{i} = or i128 %l{i}, %hs{i}\n"
                )
                .unwrap();
                match ty {
                    T::I(128) => writeln!(entry, "  %a{i} = add i128 %w{i}, 0").unwrap(),
                    T::I(_) => writeln!(entry, "  %a{i} = trunc i128 %w{i} to i{w}").unwrap(),
                    T::Q => writeln!(entry, "  %a{i} = bitcast i128 %w{i} to fp128").unwrap(),
                    _ => writeln!(
                        entry,
                        "  %n{i} = trunc i128 %w{i} to i80\n  %a{i} = bitcast i80 %n{i} to x86_fp80"
                    )
                    .unwrap(),
                }
            } else {
                match ty {
                    T::D => writeln!(entry, "  %a{i} = bitcast i64 {p} to double").unwrap(),
                    T::I(64) => writeln!(entry, "  %a{i} = add i64 {p}, 0").unwrap(),
                    T::I(bits) => writeln!(entry, "  %a{i} = trunc i64 {p} to i{bits}").unwrap(),
                    fp => {
                        let w = fp.bits();
                        writeln!(
                            entry,
                            "  %t{i} = trunc i64 {p} to i{w}\n  %a{i} = bitcast i{w} %t{i} to {}",
                            fp.ll()
                        )
                        .unwrap();
                    }
                }
            }
            k += ty.params();
        }
        assert!(k <= 6, "{name}: too many parameters");
        let tail = match ret {
            T::I(128) => "  ret i128 %r\n".to_string(),
            T::I(bits) => format!("  %out = zext i{bits} %r to i128\n  ret i128 %out\n"),
            T::Q => "  %out = bitcast fp128 %r to i128\n  ret i128 %out\n".to_string(),
            fp => {
                let w = fp.bits();
                format!(
                    "  %rb = bitcast {} %r to i{w}\n  %out = zext i{w} %rb to i128\n  \
                     ret i128 %out\n",
                    fp.ll()
                )
            }
        };
        let start = self.text.len();
        writeln!(
            self.text,
            "define i128 @{name}({}) {{\n{entry}{body}{tail}}}\n",
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
            libm: false,
            llc_nan: false,
            text,
        });
        self.funcs.len() - 1
    }

    /// Adds a case of function `func` with the words `values`.
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

/// The integer bit of an `x86_fp80` significand.
pub const J: u64 = 1 << 63;

/// The `x86_fp80` of `2^k` (`k` from -16382 up), negative if asked.
pub fn pow2(k: i32, negative: bool) -> [u64; 2] {
    [J, ((16383 + k) as u64) | (u64::from(negative) << 15)]
}

/// The `x86_fp80` words of a finite `double` (exact).
pub fn x_of(d: f64) -> [u64; 2] {
    let bits = d.to_bits();
    let sign = (bits >> 63) << 15;
    let exp = (bits >> 52) & 0x7ff;
    let frac = bits & 0xf_ffff_ffff_ffff;
    if exp == 0x7ff {
        return [J | frac << 11, sign | 0x7fff];
    }
    if exp == 0 {
        if frac == 0 {
            return [0, sign];
        }
        let shift = frac.leading_zeros();
        return [frac << shift, sign | (15372 - u64::from(shift))];
    }
    [J | frac << 11, sign | (exp + 15360)]
}

/// The edge values of `x86_fp80`, as word pairs: zeros, ones, halves and
/// ties, the denormal and normal boundaries, a pseudo-denormal, an
/// unnormal, the largest finite values, infinities, quiet and signalling
/// NaNs with payloads and both signs, a pseudo-infinity and a pseudo-NaN,
/// the integer boundaries `±2^7`, `±2^15`, `±2^31`, `2^32`, `±2^63`,
/// `2^64` with their neighbours, and values whose rounding to `double`
/// and `float` is a tie.
pub fn x_edges() -> Vec<[u64; 2]> {
    let mut v = vec![
        [0, 0],
        [0, 0x8000],
        x_of(1.0),
        x_of(-1.0),
        x_of(0.5),
        x_of(1.5),
        x_of(2.5),
        x_of(-2.5),
        x_of(-0.5),
        x_of(3.75),
        x_of(-1e-30),
        x_of(1e300),
        [1, 0],
        [1, 0x8000],
        [0x7fff_ffff_ffff_ffff, 0],
        [J, 1],
        [J | 1, 0],
        [0x4000_0000_0000_0000, 0x3fff],
        [u64::MAX, 0x7ffe],
        [u64::MAX, 0xfffe],
        [J, 0x7fff],
        [J, 0xffff],
        [0xc000_0000_0000_0000, 0x7fff],
        [0xc000_0000_0000_0000, 0xffff],
        [0xc000_0000_1234_5678, 0x7fff],
        [J | 1, 0x7fff],
        [0xa000_0000_0000_0042, 0xffff],
        [0, 0x7fff],
        [0x4000_0000_0000_0000, 0x7fff],
        [J | 1 << 10, 0x3fff],
        [J | 3 << 10, 0x3fff],
        [J | 1 << 39, 0x3fff],
        [J | 0x80_0000_0001, 0xbfff],
        [0x9234_5678_9abc_def1, 0x3ff0],
        [0xffff_ffff_ffff_fffe, 0x403d],
    ];
    for k in [7, 15, 31, 32, 63, 64] {
        let [m, e] = pow2(k, false);
        v.push([m, e]);
        v.push([m, e | 0x8000]);
        // The value just below: all ones at the exponent below.
        v.push([u64::MAX, e - 1]);
        v.push([u64::MAX, (e - 1) | 0x8000]);
        // And just above.
        v.push([m | 1, e]);
    }
    v
}

/// A random `x86_fp80`: a significand with its integer bit set (rarely
/// clear), an exponent near 1 (often), anywhere (sometimes).
pub fn x_random(rng: &mut Rng) -> [u64; 2] {
    let m = rng.next();
    let m = if rng.below(32) == 0 { m } else { m | J };
    let sign = rng.below(2) << 15;
    let e = match rng.below(4) {
        0 => rng.below(0x8000),
        1 => 16383 + rng.below(70) - 4,
        _ => 16383 + rng.below(128) - 64,
    };
    [m, sign | e]
}

/// The edge values of another type: as `float_gen`'s, fewer.
pub fn edges(ty: T) -> Vec<Vec<u64>> {
    let f = |x: f32| u64::from(x.to_bits());
    let d = |x: f64| x.to_bits();
    match ty {
        T::X => x_edges().iter().map(|w| w.to_vec()).collect(),
        T::H => [0u64, 0x8000, 0x3c00, 0xbc00, 0x3800, 0x0001, 0x8001, 0x03ff, 0x0400, 0x7bff]
            .iter()
            .chain(&[0x7c00, 0xfc00, 0x7e00, 0x7e01, 0xfe00, 0x7c01, 0x5800])
            .map(|&x| vec![x])
            .collect(),
        T::B => [0u64, 0x8000, 0x3f80, 0xbf80, 0x3f00, 0x0001, 0x8001, 0x007f, 0x7f7f, 0x7f80]
            .iter()
            .chain(&[0xff80, 0x7fc0, 0x7fc1, 0x7f81, 0x4f00])
            .map(|&x| vec![x])
            .collect(),
        T::F => [
            f(0.0),
            f(-0.0),
            f(1.0),
            f(-2.5),
            f(0.5),
            1,
            0x8000_0001,
            0x007f_ffff,
            0x0080_0000,
            0x7f7f_ffff,
            0x7f80_0000,
            0xff80_0000,
            0x7fc0_0000,
            0x7fc1_2345,
            0x7f80_0001,
            0xffa0_0003,
            0x5f00_0000,
            0xdf00_0000,
        ]
        .iter()
        .map(|&x| vec![x])
        .collect(),
        T::D => [
            d(0.0),
            d(-0.0),
            d(1.0),
            d(-2.5),
            d(0.1),
            1,
            0x8000_0000_0000_0001,
            0x000f_ffff_ffff_ffff,
            0x0010_0000_0000_0000,
            0x7fef_ffff_ffff_ffff,
            0x7ff0_0000_0000_0000,
            0xfff0_0000_0000_0000,
            0x7ff8_0000_0000_0000,
            0x7ff8_1234_5678_9abc,
            0x7ff0_0000_0000_0001,
            0xfff4_0000_0000_0003,
            0x43e0_0000_0000_0000,
        ]
        .iter()
        .map(|&x| vec![x])
        .collect(),
        T::Q => {
            let q = |hi: u64, lo: u64| vec![lo, hi];
            vec![
                q(0, 0),
                q(0x8000_0000_0000_0000, 0),
                q(0x3fff_0000_0000_0000, 0),
                q(0xbfff_8000_0000_0000, 0),
                q(0x3fff_0000_0000_0000, 1),
                q(0x3fff_0000_0000_0000, 0x4000),
                q(0x3fff_0000_0000_0000, 0x8000),
                q(0x3fff_0000_0000_0000, 0x1_8000),
                q(0x7ffe_ffff_ffff_ffff, u64::MAX),
                q(0x0000_0000_0000_0000, 1),
                q(0x0001_0000_0000_0000, 0),
                q(0x3c01_0000_0000_0000, 0),
                q(0x3bff_8000_0000_0000, 0),
                q(0x7fff_0000_0000_0000, 0),
                q(0xffff_0000_0000_0000, 0),
                q(0x7fff_8000_0000_0000, 0),
                q(0x7fff_0000_0000_0001, 0),
                q(0x43fe_0000_0000_0000, 0),
            ]
        }
        T::I(bits) => {
            let m = mask(bits.min(64));
            let mut v = vec![0, 1, 2, m, m - 1, m >> 1, (m >> 1) + 1, (m >> 1).wrapping_sub(1)];
            for k in [11, 24, 31, 32, 52, 53, 54, 62, 63, 64] {
                if k < bits.min(64) {
                    v.extend([
                        1 << k,
                        (1 << k) + 1,
                        (1 << k) - 1,
                        ((1u64 << k) + 1).wrapping_neg(),
                    ]);
                }
            }
            let mut seen = std::collections::HashSet::new();
            v.retain(|x| seen.insert(*x & m));
            if bits > 64 {
                let hm = mask(bits - 64);
                let mut out: Vec<Vec<u64>> = v.iter().map(|&x| vec![x, 0]).collect();
                out.extend(v.iter().map(|&x| vec![x, hm]));
                out.extend([vec![0, 1], vec![0, hm >> 1], vec![u64::MAX, hm >> 1]]);
                out.extend([vec![0, (hm >> 1) + 1], vec![0x8000, 0x10]]);
                out
            } else {
                v.iter().map(|x| vec![x & m]).collect()
            }
        }
    }
}

/// A random value of `ty`.
pub fn random(rng: &mut Rng, ty: T) -> Vec<u64> {
    match ty {
        T::X => x_random(rng).to_vec(),
        T::Q => {
            let hi = rng.next();
            let near =
                0x3fff_0000_0000_0000 + ((rng.below(256) << 48) ^ (hi & 0x8000_ffff_ffff_ffff));
            vec![rng.next(), if rng.below(4) == 0 { hi } else { near }]
        }
        T::I(bits) if bits > 64 => vec![rng.next(), rng.next() & mask(bits - 64)],
        T::I(bits) => {
            let x = rng.next() >> rng.below(64);
            vec![if rng.below(2) == 0 { x } else { x.wrapping_neg() } & mask(bits)]
        }
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

/// Cases of `func` (operands `ops`): every combination of edge values
/// when there are at most `limit`, a random sample of them otherwise,
/// then `extra` random sets.
pub fn cases(suite: &mut Suite, rng: &mut Rng, func: usize, ops: &[T], limit: usize, extra: usize) {
    let lists: Vec<Vec<Vec<u64>>> = ops.iter().map(|&t| edges(t)).collect();
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
/// printed as 32 hex digits.
pub fn driver(suite: &Suite, defined: &[bool]) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         typedef uint64_t u;\ntypedef unsigned __int128 w;\n\
         typedef w (*fn_t)(u, u, u, u, u, u);\n",
    );
    c.push_str(&suite.c);
    for f in &suite.funcs {
        writeln!(c, "w {}(u, u, u, u, u, u);", f.name).unwrap();
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
         const u *a = cases[i].a;\n    w r = cases[i].f(a[0], a[1], a[2], a[3], a[4], a[5]);\n    \
         printf(\"%016llx%016llx\\n\", (unsigned long long)(r >> 64), (unsigned long long)r);\n  \
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
    /// `.text` bytes of hod (plain).
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

/// The interpreter's result of each case (`None` where it traps or
/// refuses, or the function is not [`Check::Interp`]), and whether the
/// case is defined (not a trap).
fn interpret(suite: &Suite) -> (Vec<Option<u128>>, Vec<bool>) {
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
        text.push_str(&suite.callees);
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
            for &index in &by_func[chunk * 64 + offset] {
                let args: Vec<Val> = suite.cases[index].args[..f.words()]
                    .iter()
                    .map(|&v| Val::int(64, u128::from(v)))
                    .collect();
                match Interp::new(&program).with_config(config).run(sym, &args).result {
                    Ok(Some(Val::Int { value, .. })) => expected[index] = Some(value),
                    Ok(other) => panic!("{}: {other:?}", f.name),
                    Err(hod_interp::Stop::Trap(_)) => defined[index] = false,
                    // An encoding the interpreter does not classify
                    // (`is.fpclass` of an x87-only one): not compared.
                    Err(hod_interp::Stop::Unsupported(_)) => {}
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
    let scratch = Scratch::new("x64-x87-disasm");
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

/// The mnemonic of a disassembled line.
pub fn mnemonic(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or("")
}

/// Whether `got` matches the interpreter's `want` for a result of `ret`:
/// equal, or both NaNs.
/// Whether an `x86_fp80` operand of `f` among `args` is an invalid
/// encoding or a signalling NaN.
fn unusual(f: &Func, args: &[u64]) -> bool {
    let mut k = 0;
    for &t in &f.ops {
        if t == T::X {
            let (m, e) = (args[k], args[k + 1] & 0x7fff);
            let invalid = e != 0 && m & J == 0;
            let signalling = e == 0x7fff && m & (1 << 62) == 0 && m << 1 != 0;
            if invalid || signalling {
                return true;
            }
        }
        k += t.params();
    }
    false
}

fn matches_interp(ret: T, want: u128, got: u128) -> bool {
    want == got || (ret.is_nan(want) && ret.is_nan(got))
}

/// Checks `suite` (see the module documentation). `name` labels the
/// scratch directory; `majors` are the `llc`s to compare with. Without
/// the cross toolchain only the compile (and the interpreter) runs.
pub fn run(name: &str, suite: &Suite, majors: &[u32]) -> Report {
    let start = Instant::now();
    let text = format!("{}{}{}{}", ll_header(), suite.decls, suite.callees, suite.text);
    let (module, bodies) = parse(&text);
    let mut report =
        Report { insts: bodies.iter().map(hod_ir::Body::inst_count).sum(), ..Report::default() };
    let (expected, defined) = interpret(suite);
    report.undefined = defined.iter().filter(|d| !**d).count();
    report.compared = defined.len() - report.undefined;
    report.interpreted = expected.iter().filter(|e| e.is_some()).count();

    let scratch = Scratch::new(&format!("x64-x87-{name}"));
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
    let mut objects: Vec<(String, Vec<PathBuf>)> =
        legs.into_iter().map(|(leg, object)| (leg, vec![object])).collect();
    if !suite.callees.is_empty() {
        objects.extend(mixed(&scratch, suite));
    }
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return report };
    let Some(linker) = toolchain.linkers.first() else { return report };
    let source = driver(suite, &defined);
    // A place to keep the module and the driver, to look into a failure.
    if let Some(dir) = std::env::var_os("HOD_X87_KEEP") {
        let dir = PathBuf::from(dir);
        let _ = std::fs::write(dir.join(format!("{name}.ll")), &text);
        let _ = std::fs::write(dir.join(format!("{name}.c")), &source);
    }
    let driver = toolchain.compile_c(&scratch, "driver", &source, &[]);
    let index: Vec<usize> = (0..suite.cases.len()).filter(|&i| defined[i]).collect();
    let mut plain: Vec<u128> = Vec::new();
    for (leg, parts) in &objects {
        let program = scratch.join(leg.replace([' ', '/'], "-"));
        let mut inputs: Vec<&Path> = vec![&driver];
        inputs.extend(parts.iter().map(PathBuf::as_path));
        inputs.push(Path::new("-lm"));
        let ran = toolchain.link_and_run(linker, &[], &inputs, &program);
        assert!(ran.success(), "{leg}: {:?}\n{}", ran.status, ran.stderr_text());
        let got: Vec<u128> = ran
            .stdout_text()
            .lines()
            .map(|line| u128::from_str_radix(line, 16).unwrap_or_else(|_| panic!("{leg}: {line}")))
            .collect();
        assert_eq!(got.len(), index.len(), "{leg}: one line per defined case");
        for (&i, &got) in index.iter().zip(&got) {
            let case = &suite.cases[i];
            let f = &suite.funcs[case.func];
            let args = &case.args[..f.words()];
            let mut wrong = |what: &str, want: u128| {
                report
                    .wrong
                    .push(format!("{leg}: {}({args:#x?}): {what} {want:#x}, got {got:#x}", f.name));
            };
            if leg == "hod plain" {
                if let Some(want) = expected[i]
                    && !(f.libm && unusual(f, args))
                    && !matches_interp(f.ret, want, got)
                {
                    wrong("interpreter", want);
                }
            } else if leg == "hod stress" || leg.starts_with("mix") || compared_by_llc(f, leg) {
                let want = plain[report_index(&index, i)];
                let nan = f.llc_nan && leg.starts_with("llc");
                if want != got && !(nan && matches_interp(f.ret, want, got)) {
                    wrong("hod plain", want);
                }
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

/// The mixed legs: the callers by hod with the callees by `llc` 22
/// (`mix hod/llc`) and the reverse (`mix llc/hod`); none without `llc`
/// 22.
fn mixed(scratch: &Scratch, suite: &Suite) -> Vec<(String, Vec<PathBuf>)> {
    let callers = format!("{}{}{}{}", ll_header(), suite.decls, suite.externs, suite.text);
    let callees = format!("{}{}{}", ll_header(), suite.decls, suite.callees);
    let hod = |name: &str, text: &str| {
        let (module, bodies) = parse(text);
        let object = compile_to_object(&module, &bodies, &pic(Stress::default()))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        scratch.write(&format!("{name}.o"), object)
    };
    let (Some(llc_callers), Some(llc_callees)) =
        (llc(scratch, 22, "mix-callers", &callers), llc(scratch, 22, "mix-callees", &callees))
    else {
        return Vec::new();
    };
    vec![
        ("mix hod/llc".into(), vec![hod("mix-hod-callers", &callers), llc_callees]),
        ("mix llc/hod".into(), vec![llc_callers, hod("mix-hod-callees", &callees)]),
    ]
}

/// Whether leg `leg` (`llc <major>`) compiles `f` and compares it.
fn compared_by_llc(f: &Func, leg: &str) -> bool {
    let major = leg.strip_prefix("llc ").and_then(|m| m.parse::<u32>().ok());
    f.llc && major.is_some_and(|m| f.since <= m)
}

/// The module `llc` `major` compiles: the functions it compares, a stub
/// returning 0 for each other one (so that the driver links), the
/// module-level lines they use.
fn llc_text(suite: &Suite, major: u32) -> String {
    let mut body = String::new();
    for f in &suite.funcs {
        if f.llc && f.since <= major {
            body.push_str(&suite.text[f.text.clone()]);
        } else {
            let params: Vec<String> = (0..f.words()).map(|k| format!("i64 %p{k}")).collect();
            let _ = write!(
                body,
                "define i128 @{}({}) {{\n  ret i128 0\n}}\n\n",
                f.name,
                params.join(", ")
            );
        }
    }
    format!("{}{}{}{body}", ll_header(), suite.decls, suite.callees)
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
        "x64 x87 {name}: {} functions, {} cases run on {:?} ({} against the interpreter, \
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
        let mut counts = std::collections::BTreeMap::new();
        for line in &report.wrong {
            let key = line.split('(').next().unwrap_or(line);
            *counts.entry(key).or_insert(0) += 1;
        }
        let mut message = format!("{} disagreements: {counts:?}; the first:\n", report.wrong.len());
        for line in report.wrong.iter().take(12) {
            let name = line.split(": ").nth(1).and_then(|s| s.split('(').next()).unwrap_or("");
            writeln!(message, "{line}\n{}", body_of(suite, name)).unwrap();
        }
        panic!("{message}");
    }
    report
}
