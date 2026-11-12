//! The `Mem`-class group's own programs (WP-040): wide integers,
//! aggregates, `Mem` vectors and the overflow intrinsics, as `.ll` text
//! checked against `hod_interp` and `llc -O0 -global-isel=0
//! -fast-isel=0` natively.
//!
//! A function of a [`Suite`] is a *core*: instructions over `%a`, `%b`,
//! `%c` (its parameters) that define `%r` (its result), starting in a
//! block named `entry`. Each core is emitted twice:
//!
//! - `@<name>(ptr %out, ptr %in)`: loads the parameters from `%in`
//!   (at offsets 0, 128, 256), runs the core and stores `%r` to `%out`.
//!   The driver surrounds `%out` with 16-byte sentinels on each side, so
//!   a store that writes outside the store size of `%r` is caught, and
//!   fills the bytes after each parameter's store size in `%in` with
//!   garbage.
//! - `@<name>_v(<params>)` returning `%r`, when every parameter is an
//!   integer of at most 128 bits and the result has no vector and no
//!   integer wider than 128 bits: the interpreter runs it (the expected
//!   result), and when the result is such an integer too it is called
//!   natively from C with `unsigned __int128` / `uint64_t` arguments
//!   whose bits above the width are random (AAPCS64 leaves them
//!   unspecified), so the lowering's treatment of the bits above the
//!   width is checked.
//!
//! The expected image is the interpreter's; where there is none (wider
//! than 128 bits, vectors, aggregate parameters) it is `llc`'s. Images
//! are compared under a significance mask computed here from the
//! layouts (struct padding never counts; the bits above an integer's
//! width must be zero in hod's stores, `spec/codegen.md` §5.5.1, and do
//! not count for `llc`). A case the interpreter traps on (undefined) is
//! run but not compared.

#![allow(clippy::many_single_char_names, clippy::too_many_lines, clippy::cast_possible_truncation)]
#![allow(dead_code, reason = "each test uses part of it")]
#![allow(
    clippy::similar_names,
    clippy::cloned_ref_to_slice_refs,
    clippy::comparison_chain,
    clippy::format_collect,
    reason = "a test generator: `.ll` text built by hand"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::process::Command;
use std::time::{Duration, Instant};

use hod_a64::compile_module;
use hod_cg::{Options, Stress};
use hod_interp::{Interp, Program, Stop, Val};
use hod_testutil::{Scratch, run_with};

use crate::common::linking::Toolchain;
use crate::common::{ll_header, llc, llc_major, parse, write_object};

/// Where the parameters are in the input buffer.
const STRIDE: usize = 128;

/// A type of a function of a suite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum T {
    /// `iN`.
    I(u32),
    /// `float`.
    F,
    /// `double`.
    D,
    /// `ptr`.
    P,
    /// A struct; `true` when packed.
    S(Vec<T>, bool),
    /// An array.
    A(u64, Box<T>),
    /// A vector (byte-sized elements, or `i1`).
    V(u64, Box<T>),
}

fn round(x: u64, a: u64) -> u64 {
    x.div_ceil(a) * a
}

impl T {
    /// Its `.ll` spelling.
    pub fn ll(&self) -> String {
        match self {
            T::I(b) => format!("i{b}"),
            T::F => "float".into(),
            T::D => "double".into(),
            T::P => "ptr".into(),
            T::S(fields, packed) => {
                let list: Vec<String> = fields.iter().map(T::ll).collect();
                let inner =
                    if list.is_empty() { String::new() } else { format!(" {} ", list.join(", ")) };
                if *packed { format!("<{{{inner}}}>") } else { format!("{{{inner}}}") }
            }
            T::A(n, e) => format!("[{n} x {}]", e.ll()),
            T::V(n, e) => format!("<{n} x {}>", e.ll()),
        }
    }

    fn bits(&self) -> u64 {
        match self {
            T::I(b) => u64::from(*b),
            T::F => 32,
            T::D | T::P => 64,
            _ => self.store() * 8,
        }
    }

    /// Its ABI alignment (the AArch64 Linux data layout).
    pub fn align(&self) -> u64 {
        match self {
            T::I(b) => match b {
                0..=8 => 1,
                9..=16 => 2,
                17..=32 => 4,
                33..=64 => 8,
                _ => 16,
            },
            T::F => 4,
            T::D | T::P => 8,
            T::S(_, true) => 1,
            T::S(fields, false) => fields.iter().map(T::align).max().unwrap_or(1),
            T::A(_, e) => e.align(),
            T::V(..) => self.store().next_power_of_two(),
        }
    }

    /// The offsets of a struct's fields.
    pub fn offsets(&self) -> Vec<u64> {
        let T::S(fields, packed) = self else { return Vec::new() };
        let mut at = 0;
        fields
            .iter()
            .map(|f| {
                if !packed {
                    at = round(at, f.align());
                }
                let here = at;
                at += f.alloc();
                here
            })
            .collect()
    }

    /// Its store size.
    pub fn store(&self) -> u64 {
        match self {
            T::I(b) => u64::from(*b).div_ceil(8),
            T::F => 4,
            T::D | T::P => 8,
            T::S(fields, _) => {
                let end = self.offsets().last().map_or(0, |o| o + fields.last().unwrap().alloc());
                round(end, self.align())
            }
            T::A(n, e) => n * e.alloc(),
            T::V(n, e) => (n * e.bits()).div_ceil(8),
        }
    }

    /// Its alloc size.
    pub fn alloc(&self) -> u64 {
        round(self.store(), self.align())
    }

    /// Writes into `out` (at `off`) which bits are significant: with
    /// `strict`, the bits above an integer's width count too (they must
    /// be zero).
    pub fn mask(&self, strict: bool, out: &mut [u8], off: u64) {
        let off = off as usize;
        match self {
            T::I(b) => {
                for i in 0..self.store() as usize {
                    let lo = i as u32 * 8;
                    out[off + i] |=
                        if strict || lo + 8 <= *b { 0xff } else { (1u8 << (b - lo)) - 1 };
                }
            }
            T::F | T::D | T::P => out[off..off + self.store() as usize].fill(0xff),
            T::S(fields, _) => {
                for (f, o) in fields.iter().zip(self.offsets()) {
                    f.mask(strict, out, off as u64 + o);
                }
            }
            T::A(n, e) => {
                for i in 0..*n {
                    e.mask(strict, out, off as u64 + i * e.alloc());
                }
            }
            T::V(n, e) => {
                if **e == T::I(1) {
                    for i in 0..*n as usize {
                        out[off + i / 8] |= 1 << (i % 8);
                    }
                } else {
                    let step = e.bits() / 8;
                    for i in 0..*n {
                        e.mask(strict, out, off as u64 + i * step);
                    }
                }
            }
        }
    }

    /// Writes a random canonical image (bits above integer widths clear)
    /// into `out` at `off`.
    pub fn image(&self, rng: &mut Rng, out: &mut [u8], off: u64) {
        let o = off as usize;
        match self {
            T::I(b) => {
                let w = value(rng, *b);
                out[o..o + self.store() as usize].copy_from_slice(&bytes_of(&w, *b));
            }
            T::F | T::D | T::P => {
                let n = self.store() as usize;
                for byte in &mut out[o..o + n] {
                    *byte = rng.next() as u8;
                }
                // Finite floats: a NaN's payload need not survive.
                out[o + n - 1] &= 0xbf;
            }
            T::S(fields, _) => {
                for (f, at) in fields.iter().zip(self.offsets()) {
                    f.image(rng, out, off + at);
                }
            }
            T::A(n, e) => {
                for i in 0..*n {
                    e.image(rng, out, off + i * e.alloc());
                }
            }
            T::V(n, e) => {
                if **e == T::I(1) {
                    for i in 0..*n as usize {
                        out[o + i / 8] |= u8::from(rng.chance(50)) << (i % 8);
                    }
                } else {
                    let step = e.bits() / 8;
                    for i in 0..*n {
                        e.image(rng, out, off + i * step);
                    }
                }
            }
        }
    }

    /// Whether the interpreter can return it (no vector, no integer wider
    /// than 128 bits).
    pub fn interp(&self) -> bool {
        match self {
            T::I(b) => *b <= 128,
            T::F | T::D | T::P => true,
            T::S(fields, _) => fields.iter().all(T::interp),
            T::A(_, e) => e.interp(),
            T::V(..) => false,
        }
    }

    fn small_int(&self) -> Option<u32> {
        match self {
            T::I(b) if *b <= 128 => Some(*b),
            _ => None,
        }
    }
}

/// `{ ty, i1 }`, the result of an overflow intrinsic.
pub fn with_flag(bits: u32) -> T {
    T::S(vec![T::I(bits), T::I(1)], false)
}

// ---- wide values: little-endian limbs, the bits above the width clear ----

/// The limbs of an `iN`.
pub fn limbs(bits: u32) -> usize {
    bits.div_ceil(64) as usize
}

fn canon(mut w: Vec<u64>, bits: u32) -> Vec<u64> {
    w.resize(limbs(bits), 0);
    let top = bits % 64;
    if top != 0 {
        *w.last_mut().unwrap() &= (1u64 << top) - 1;
    }
    w
}

/// The value `v` (zero-extended) at `bits`.
pub fn small(bits: u32, v: u128) -> Vec<u64> {
    canon(vec![v as u64, (v >> 64) as u64], bits)
}

fn ones(bits: u32) -> Vec<u64> {
    canon(vec![u64::MAX; limbs(bits)], bits)
}

fn bit(bits: u32, k: u32) -> Vec<u64> {
    let mut w = vec![0; limbs(bits)];
    if k < bits {
        w[(k / 64) as usize] = 1 << (k % 64);
    }
    w
}

fn sub1(w: &[u64], bits: u32) -> Vec<u64> {
    let mut w = w.to_vec();
    for limb in &mut w {
        let (v, borrow) = limb.overflowing_sub(1);
        *limb = v;
        if !borrow {
            break;
        }
    }
    canon(w, bits)
}

fn add1(w: &[u64], bits: u32) -> Vec<u64> {
    let mut w = w.to_vec();
    for limb in &mut w {
        let (v, carry) = limb.overflowing_add(1);
        *limb = v;
        if !carry {
            break;
        }
    }
    canon(w, bits)
}

/// The values at the edges of `bits`: 0, 1, 2, -1, -2, the smallest and
/// largest signed values and their neighbours, the shift amounts around
/// 64 and the width, the limb boundaries, patterns.
pub fn edges(bits: u32) -> Vec<Vec<u64>> {
    let min = bit(bits, bits - 1);
    let mut out: Vec<Vec<u64>> = Vec::new();
    let mut add = |w: Vec<u64>| {
        let w = canon(w, bits);
        if !out.contains(&w) {
            out.push(w);
        }
    };
    add(small(bits, 0));
    add(small(bits, 1));
    add(small(bits, 2));
    add(ones(bits));
    add(sub1(&ones(bits), bits));
    add(min.clone());
    add(sub1(&min, bits));
    add(add1(&min, bits));
    for k in [1u32, 63, 64, 65, 127, 128, 129] {
        add(small(bits, u128::from(k)));
    }
    for k in [bits - 1, bits, bits + 1, bits.saturating_sub(64)] {
        add(small(bits, u128::from(k)));
    }
    add(small(bits, u128::from(u64::MAX)));
    add(bit(bits, 64));
    add(add1(&bit(bits, 64), bits));
    add(bit(bits, 63));
    add(bit(bits, 127));
    add(bit(bits, 128));
    add(vec![0x5555_5555_5555_5555; limbs(bits)]);
    add(vec![0x00ff_00ff_00ff_00ff; limbs(bits)]);
    add(canon(vec![0, u64::MAX, 0, u64::MAX], bits));
    out
}

/// A value of `iN`: an edge (50%) or random bits.
pub fn value(rng: &mut Rng, bits: u32) -> Vec<u64> {
    if rng.chance(50) {
        rng.pick(&edges(bits)).clone()
    } else {
        canon((0..limbs(bits)).map(|_| rng.next()).collect(), bits)
    }
}

/// A shift amount at `bits`: around 0, 64, 128 and the width.
pub fn amount(rng: &mut Rng, bits: u32) -> Vec<u64> {
    let picks =
        [0, 1, 7, 31, 63, 64, 65, 100, 127, 128, 129, bits - 1, bits - 2, bits / 2, bits - 63];
    let k = u128::from(*rng.pick(&picks) % bits);
    small(bits, k)
}

/// The store image of `w` at `bits`.
pub fn bytes_of(w: &[u64], bits: u32) -> Vec<u8> {
    let mut out: Vec<u8> = w.iter().flat_map(|l| l.to_le_bytes()).collect();
    out.truncate(bits.div_ceil(8) as usize);
    out
}

/// `w` as an `.ll` literal of `iN`.
pub fn lit(w: &[u64], bits: u32) -> String {
    if bits == 1 {
        return if w[0] & 1 == 1 { "true" } else { "false" }.into();
    }
    let mut hex = String::new();
    for limb in w.iter().rev() {
        write!(hex, "{limb:016x}").unwrap();
    }
    let hex = hex.trim_start_matches('0');
    if hex.is_empty() { "0".into() } else { format!("u0x{hex}") }
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

    /// True with probability `percent` / 100.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// One element of `items`.
    pub fn pick<'t, X>(&mut self, items: &'t [X]) -> &'t X {
        &items[self.below(items.len() as u64) as usize]
    }
}

// ---- suites ----

/// A function of a suite.
#[derive(Clone, Debug)]
pub struct Func {
    /// Its name.
    pub name: String,
    /// Its parameters (at most three).
    pub params: Vec<T>,
    /// Its result.
    pub ret: T,
    /// Whether `@<name>_v` exists (interpreted).
    pub twin: bool,
    /// Whether `@<name>_v` is also called natively with garbage above
    /// the widths.
    pub regs: bool,
    /// Whether `llc`'s result must agree (not where `freeze` of `poison`
    /// decides the result: LLVM leaves it arbitrary).
    pub llc: bool,
    /// Its core, for failure messages.
    pub core: String,
}

/// One call of a function: the parameters' store images.
#[derive(Clone, Debug)]
pub struct Case {
    /// Index into [`Suite::funcs`].
    pub func: usize,
    /// The arguments' images.
    pub args: Vec<Vec<u8>>,
}

/// Functions and the cases to run them with.
#[derive(Default, Debug)]
pub struct Suite {
    /// The wrappers' text and the declarations.
    pub text: String,
    /// The twins' text (all of them, for the interpreter).
    pub twins: String,
    /// The twins called natively.
    pub reg_twins: String,
    /// Declarations (intrinsics).
    pub decls: BTreeSet<String>,
    /// The functions.
    pub funcs: Vec<Func>,
    /// The cases.
    pub cases: Vec<Case>,
}

const NAMES: [&str; 3] = ["%a", "%b", "%c"];

impl Suite {
    /// Adds a function of `core`; returns its index.
    pub fn add(&mut self, params: &[T], ret: T, core: &str) -> usize {
        let name = format!("f{}", self.funcs.len());
        let twin = params.iter().all(|p| p.small_int().is_some()) && ret.interp();
        let regs = twin && ret.small_int().is_some();
        let mut w = format!("define void @{name}(ptr %out, ptr %in) {{\nentry:\n");
        for (i, p) in params.iter().enumerate() {
            writeln!(w, "  %in{i} = getelementptr i8, ptr %in, i64 {}", i * STRIDE).unwrap();
            writeln!(w, "  {} = load {}, ptr %in{i}", NAMES[i], p.ll()).unwrap();
        }
        writeln!(w, "{core}  store {} %r, ptr %out\n  ret void\n}}\n", ret.ll()).unwrap();
        self.text.push_str(&w);
        if twin {
            let list: Vec<String> = params
                .iter()
                .enumerate()
                .map(|(i, p)| format!("{} {}", p.ll(), NAMES[i]))
                .collect();
            let text = format!(
                "define {} @{name}_v({}) {{\nentry:\n{core}  ret {} %r\n}}\n\n",
                ret.ll(),
                list.join(", "),
                ret.ll()
            );
            self.twins.push_str(&text);
            if regs {
                self.reg_twins.push_str(&text);
            }
        }
        let core = core.to_string();
        self.funcs.push(Func { name, params: params.to_vec(), ret, twin, regs, llc: true, core });
        self.funcs.len() - 1
    }

    /// Adds a case of `func` with these images.
    pub fn case(&mut self, func: usize, args: Vec<Vec<u8>>) {
        self.cases.push(Case { func, args });
    }

    /// Adds `count` cases of `func` with random canonical arguments.
    pub fn random_cases(&mut self, rng: &mut Rng, func: usize, count: usize) {
        for _ in 0..count {
            let args = self.funcs[func]
                .params
                .iter()
                .map(|p| {
                    let mut out = vec![0; p.store() as usize];
                    p.image(rng, &mut out, 0);
                    out
                })
                .collect();
            self.case(func, args);
        }
    }

    /// The whole module text after `header`.
    pub fn module(&self, header: &str) -> String {
        let mut text = header.to_string();
        for d in &self.decls {
            text.push_str(d);
            text.push('\n');
        }
        text.push_str(&self.text);
        text.push_str(&self.reg_twins);
        text
    }
}

/// The wide widths every operation is checked at.
pub const WIDTHS: [u32; 10] = [65, 72, 96, 127, 128, 129, 192, 200, 255, 256];

/// The binary operations.
pub const BINARY: [&str; 13] = [
    "add", "sub", "mul", "and", "or", "xor", "shl", "lshr", "ashr", "udiv", "sdiv", "urem", "srem",
];

/// The predicates.
pub const PREDICATES: [&str; 10] =
    ["eq", "ne", "ugt", "uge", "ult", "ule", "sgt", "sge", "slt", "sle"];

fn ints(ws: &[Vec<u64>], bits: u32) -> Vec<Vec<u8>> {
    ws.iter().map(|w| bytes_of(w, bits)).collect()
}

/// Every binary operation at every wide width (division up to 128
/// bits): two register operands over edge pairs and random values, a
/// constant on either side.
pub fn binary_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for bits in WIDTHS {
        let ty = T::I(bits);
        let e = edges(bits);
        for op in BINARY {
            if (op.ends_with("div") || op.ends_with("rem")) && bits > 128 {
                continue;
            }
            let shift = matches!(op, "shl" | "lshr" | "ashr");
            let f = s.add(
                &[ty.clone(), ty.clone()],
                ty.clone(),
                &format!("  %r = {op} i{bits} %a, %b\n"),
            );
            for _ in 0..40 {
                let a = if rng.chance(50) { rng.pick(&e).clone() } else { value(rng, bits) };
                let b = if shift {
                    amount(rng, bits)
                } else if rng.chance(50) {
                    rng.pick(&e).clone()
                } else {
                    value(rng, bits)
                };
                s.case(f, ints(&[a, b], bits));
            }
            // Constants on the right and on the left.
            for _ in 0..3 {
                let k = if shift { amount(rng, bits) } else { value(rng, bits) };
                let f = s.add(
                    &[ty.clone()],
                    ty.clone(),
                    &format!("  %r = {op} i{bits} %a, {}\n", lit(&k, bits)),
                );
                for _ in 0..6 {
                    let a = value(rng, bits);
                    s.case(f, ints(&[a], bits));
                }
            }
            if !shift {
                let k = value(rng, bits);
                let f = s.add(
                    &[ty.clone()],
                    ty.clone(),
                    &format!("  %r = {op} i{bits} {}, %a\n", lit(&k, bits)),
                );
                for _ in 0..6 {
                    let a = value(rng, bits);
                    s.case(f, ints(&[a], bits));
                }
            }
        }
    }
    s
}

/// The producers whose results carry bits above the width in hod's
/// representation (`spec/codegen.md` §5.5.1), followed by each consumer
/// that depends on them: compares, right shifts, division, extensions,
/// truncation and stores.
pub fn chain_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for bits in [65, 96, 127, 128, 129, 200] {
        let ty = T::I(bits);
        for producer in ["add", "sub", "mul", "shl"] {
            let mut consumers: Vec<(String, T)> = vec![
                (format!("  %r = add i{bits} %t, 0\n"), ty.clone()),
                (format!("  %r = lshr i{bits} %t, %c\n"), ty.clone()),
                (format!("  %r = ashr i{bits} %t, %c\n"), ty.clone()),
                (format!("  %r = lshr i{bits} %t, 3\n"), ty.clone()),
                (format!("  %r = ashr i{bits} %t, {}\n", bits - 1), ty.clone()),
                (
                    format!("  %x = zext i{bits} %t to i300\n  %r = trunc i300 %x to i260\n"),
                    T::I(260),
                ),
                (format!("  %r = sext i{bits} %t to i264\n"), T::I(264)),
                (format!("  %r = trunc i{bits} %t to i64\n"), T::I(64)),
                (format!("  %r = trunc i{bits} %t to i{}\n", bits - 1), T::I(bits - 1)),
                (
                    format!(
                        "  %r = insertvalue {{ i8, i{bits} }} zeroinitializer, i{bits} %t, 1\n"
                    ),
                    T::S(vec![T::I(8), ty.clone()], false),
                ),
            ];
            for p in PREDICATES {
                consumers.push((format!("  %r = icmp {p} i{bits} %t, %c\n"), T::I(1)));
            }
            if bits <= 128 {
                for d in ["udiv", "sdiv", "urem", "srem"] {
                    consumers.push((format!("  %r = {d} i{bits} %t, %c\n"), ty.clone()));
                }
            }
            for (consumer, ret) in consumers {
                let shiftish = consumer.contains("shr");
                let core = format!("  %t = {producer} i{bits} %a, %b\n{consumer}");
                let f = s.add(&[ty.clone(), ty.clone(), ty.clone()], ret, &core);
                for _ in 0..12 {
                    let a = value(rng, bits);
                    let b = if producer == "shl" { amount(rng, bits) } else { value(rng, bits) };
                    let c = if shiftish { amount(rng, bits) } else { value(rng, bits) };
                    s.case(f, ints(&[a, b, c], bits));
                }
            }
        }
    }
    s
}

/// Every predicate at every width, as a value and feeding `select`;
/// `select` with a parameter condition; `freeze`; a `phi` of wide values.
pub fn compare_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for bits in WIDTHS {
        let ty = T::I(bits);
        let e = edges(bits);
        for p in PREDICATES {
            let f = s.add(
                &[ty.clone(), ty.clone()],
                T::I(1),
                &format!("  %r = icmp {p} i{bits} %a, %b\n"),
            );
            for _ in 0..30 {
                let a = rng.pick(&e).clone();
                let b = if rng.chance(30) {
                    a.clone()
                } else if rng.chance(50) {
                    rng.pick(&e).clone()
                } else {
                    value(rng, bits)
                };
                s.case(f, ints(&[a, b], bits));
            }
            let k = rng.pick(&e).clone();
            let core = format!(
                "  %c1 = icmp {p} i{bits} %a, {}\n  %r = select i1 %c1, i{bits} %a, i{bits} %b\n",
                lit(&k, bits)
            );
            let f = s.add(&[ty.clone(), ty.clone()], ty.clone(), &core);
            for _ in 0..8 {
                let a = if rng.chance(30) { k.clone() } else { value(rng, bits) };
                let b = value(rng, bits);
                s.case(f, ints(&[a, b], bits));
            }
        }
        // select on a parameter, with constants and registers.
        let k = value(rng, bits);
        for core in [
            format!("  %r = select i1 %c, i{bits} %a, i{bits} %b\n"),
            format!("  %r = select i1 %c, i{bits} {}, i{bits} %a\n", lit(&k, bits)),
            format!("  %r = select i1 %c, i{bits} %a, i{bits} 0\n"),
            format!("  %r = select i1 true, i{bits} %a, i{bits} %b\n"),
            format!("  %r = freeze i{bits} %a\n"),
            format!("  %f = freeze i{bits} {}\n  %r = xor i{bits} %f, %a\n", lit(&k, bits)),
            format!(
                "  br i1 %c, label %l, label %m\nl:\n  %x = add i{bits} %a, 1\n  br label %j\nm:\n  br label %j\nj:\n  %r = phi i{bits} [ %x, %l ], [ %b, %m ]\n"
            ),
            format!(
                "  br label %h\nh:\n  %i = phi i{bits} [ 0, %entry ], [ %i1, %h ]\n  %v = phi i{bits} [ %a, %entry ], [ %b, %h ], \n  %i1 = add i{bits} %i, 1\n  %d = icmp ult i{bits} %i1, 3\n  br i1 %d, label %h, label %x\nx:\n  %r = add i{bits} %v, %i\n"
            ),
        ] {
            let core = core.replace(", \n", "\n");
            let f = s.add(&[ty.clone(), ty.clone(), T::I(1)], ty.clone(), &core);
            for _ in 0..8 {
                let (a, b) = (value(rng, bits), value(rng, bits));
                let c = small(1, u128::from(rng.chance(50)));
                let mut args = ints(&[a, b], bits);
                args.push(bytes_of(&c, 1));
                s.case(f, args);
            }
        }
        // freeze of poison: hod and the interpreter make it 0; LLVM does not.
        let f = s.add(
            &[ty.clone()],
            ty.clone(),
            &format!("  %f = freeze i{bits} poison\n  %r = or i{bits} %f, %a\n"),
        );
        s.funcs[f].llc = false;
        s.case(f, ints(&[value(rng, bits)], bits));
    }
    s
}

/// Every extension and truncation between narrow and wide widths, the
/// pointer casts, and the bitcasts between wide integers and vectors.
pub fn cast_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    let all = [1u32, 8, 33, 63, 64, 65, 96, 127, 128, 129, 200, 256];
    for &from in &all {
        for &to in &all {
            let ops: &[&str] = if to > from {
                &["zext", "sext"]
            } else if to < from {
                &["trunc"]
            } else {
                &[]
            };
            if to <= 64 && from <= 64 {
                continue;
            }
            for op in ops {
                let f =
                    s.add(&[T::I(from)], T::I(to), &format!("  %r = {op} i{from} %a to i{to}\n"));
                for w in edges(from).into_iter().take(10) {
                    s.case(f, ints(&[w], from));
                }
                for _ in 0..6 {
                    let w = value(rng, from);
                    s.case(f, ints(&[w], from));
                }
            }
        }
    }
    let mut add = |s: &mut Suite, params: &[T], ret: T, core: &str| {
        let f = s.add(params, ret, core);
        s.random_cases(rng, f, 12);
    };
    for bits in [65, 128, 256] {
        add(
            &mut s,
            &[T::I(64)],
            T::I(bits),
            &format!("  %p = inttoptr i64 %a to ptr\n  %r = ptrtoint ptr %p to i{bits}\n"),
        );
        add(
            &mut s,
            &[T::I(bits)],
            T::I(64),
            &format!("  %p = inttoptr i{bits} %a to ptr\n  %r = ptrtoint ptr %p to i64\n"),
        );
    }
    let v = |n, e| T::V(n, Box::new(e));
    for (int, vec) in [
        (128, v(2, T::I(64))),
        (128, v(4, T::F)),
        (96, v(3, T::I(32))),
        (256, v(4, T::I(64))),
        (256, v(8, T::F)),
        (192, v(3, T::D)),
        (64, v(8, T::I(8))),
        (8, v(8, T::I(1))),
        (16, v(16, T::I(1))),
        (128, v(128, T::I(1))),
        (64, v(64, T::I(1))),
    ] {
        let vl = vec.ll();
        // A 64- or 128-bit vector of non-`i1` elements is `Fpr`: only
        // through a round trip (its loads and stores are not this group's).
        let fpr = matches!(&vec, T::V(_, e) if **e != T::I(1)) && matches!(int, 64 | 128);
        add(
            &mut s,
            &[T::I(int)],
            T::I(int),
            &format!("  %x = bitcast i{int} %a to {vl}\n  %r = bitcast {vl} %x to i{int}\n"),
        );
        if fpr {
            continue;
        }
        add(&mut s, &[T::I(int)], vec.clone(), &format!("  %r = bitcast i{int} %a to {vl}\n"));
        add(&mut s, &[vec.clone()], T::I(int), &format!("  %r = bitcast {vl} %a to i{int}\n"));
        add(
            &mut s,
            &[vec.clone()],
            vec.clone(),
            &format!("  %x = bitcast {vl} %a to i{int}\n  %r = bitcast i{int} %x to {vl}\n"),
        );
    }
    s
}

/// The aggregate types the tests build.
pub fn aggregates() -> Vec<T> {
    let s = |f: Vec<T>| T::S(f, false);
    let a = |n, e| T::A(n, Box::new(e));
    vec![
        s(vec![T::I(8), T::I(32), T::I(64)]),
        s(vec![T::I(7), T::I(65), T::I(1)]),
        a(3, T::I(24)),
        T::S(vec![T::I(8), T::I(32)], true),
        T::S(vec![T::I(1), T::I(65), T::I(17)], true),
        s(vec![T::I(16), a(2, s(vec![T::I(8), T::I(64)])), T::I(32)]),
        s(vec![]),
        a(0, T::I(32)),
        s(vec![T::D, T::F]),
        s(vec![T::P, T::I(64)]),
        a(2, T::I(128)),
        s(vec![T::I(128), T::I(8)]),
        s(vec![T::I(9), s(vec![]), a(0, T::I(64)), T::I(129)]),
        a(2, a(3, T::I(16))),
        s(vec![T::I(200), T::F]),
    ]
}

/// The leaves of `ty` as index paths and their types.
fn leaves(ty: &T, path: &mut Vec<u64>, out: &mut Vec<(Vec<u64>, T)>) {
    match ty {
        T::S(fields, _) => {
            for (i, f) in fields.iter().enumerate() {
                path.push(i as u64);
                leaves(f, path, out);
                path.pop();
            }
        }
        T::A(n, e) => {
            for i in 0..*n {
                path.push(i);
                leaves(e, path, out);
                path.pop();
            }
        }
        _ => out.push((path.clone(), ty.clone())),
    }
}

fn idx(path: &[u64]) -> String {
    path.iter().map(|i| format!(", {i}")).collect()
}

/// A leaf of type `ty` made from the parameter `src` (an integer of
/// `bits`), and the instructions computing it.
fn leaf_from(ty: &T, src: &str, bits: u32, n: usize) -> (String, String) {
    let name = format!("%v{n}");
    let int = |b: u32| -> String {
        if b == bits {
            format!("  {name} = add i{b} {src}, {n}\n")
        } else if b < bits {
            format!("  {name} = trunc i{bits} {src} to i{b}\n")
        } else {
            format!("  {name} = sext i{bits} {src} to i{b}\n")
        }
    };
    let code = match ty {
        T::I(b) => int(*b),
        T::F => {
            format!("  %t{n} = trunc i{bits} {src} to i32\n  {name} = bitcast i32 %t{n} to float\n")
        }
        T::D => format!(
            "  %t{n} = trunc i{bits} {src} to i64\n  {name} = bitcast i64 %t{n} to double\n"
        ),
        T::P => {
            format!("  %t{n} = trunc i{bits} {src} to i64\n  {name} = inttoptr i64 %t{n} to ptr\n")
        }
        _ => unreachable!(),
    };
    (code, name)
}

/// Every aggregate built leaf by leaf with `insertvalue` (from `undef`,
/// `zeroinitializer` and a constant), read back with `extractvalue`
/// (each leaf, and whole sub-aggregates moved between two values),
/// selected, frozen, merged by a `phi`, loaded and stored; constant
/// aggregates; aggregate parameters (loaded from memory).
pub fn aggregate_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for ty in aggregates() {
        let tl = ty.ll();
        let mut ls = Vec::new();
        leaves(&ty, &mut Vec::new(), &mut ls);
        let ints = [T::I(128), T::I(96), T::I(128)];
        // Built from undef or zeroinitializer, every leaf from a parameter.
        for base in ["undef", "zeroinitializer", "poison"] {
            let mut core = String::new();
            let mut cur = base.to_string();
            for (n, (path, lt)) in ls.iter().enumerate() {
                let (src, bits) = [("%a", 128), ("%b", 96), ("%c", 128)][n % 3];
                let (code, v) = leaf_from(lt, src, bits, n);
                core.push_str(&code);
                writeln!(core, "  %s{n} = insertvalue {tl} {cur}, {} {v}{}", lt.ll(), idx(path))
                    .unwrap();
                cur = format!("%s{n}");
            }
            writeln!(core, "  %r = freeze {tl} {cur}").unwrap();
            let f = s.add(&ints, ty.clone(), &core);
            s.random_cases(rng, f, 6);
        }
        if ls.is_empty() {
            // `{}` and `[0 x i32]`: values without bytes.
            let f = s.add(&[], ty.clone(), &format!("  %x = freeze {tl} undef\n  %r = select i1 true, {tl} %x, {tl} zeroinitializer\n"));
            s.case(f, vec![]);
            continue;
        }
        // Into a zero aggregate, one leaf; then every leaf read back and
        // summed, to check `extractvalue`.
        for (n, (path, lt)) in ls.iter().enumerate() {
            let (code, v) = leaf_from(lt, "%a", 128, n);
            let core = format!(
                "{code}  %r = insertvalue {tl} zeroinitializer, {} {v}{}\n",
                lt.ll(),
                idx(path)
            );
            let f = s.add(&ints, ty.clone(), &core);
            s.random_cases(rng, f, 6);
        }
        // A full value, then each leaf extracted into the result.
        let mut build = String::new();
        let mut cur = "undef".to_string();
        for (n, (path, lt)) in ls.iter().enumerate() {
            let (src, bits) = [("%a", 128), ("%b", 96), ("%c", 128)][n % 3];
            let (code, v) = leaf_from(lt, src, bits, n);
            build.push_str(&code);
            writeln!(build, "  %s{n} = insertvalue {tl} {cur}, {} {v}{}", lt.ll(), idx(path))
                .unwrap();
            cur = format!("%s{n}");
        }
        writeln!(build, "  %full = freeze {tl} {cur}").unwrap();
        for (n, (path, lt)) in ls.iter().enumerate() {
            // A float or pointer leaf is returned as its bits (`Fpr`
            // stores are not this group's).
            let (tail, ret) = match lt {
                T::F => ("  %r = bitcast float %e to i32\n".to_string(), T::I(32)),
                T::D => ("  %r = bitcast double %e to i64\n".to_string(), T::I(64)),
                T::P => ("  %r = ptrtoint ptr %e to i64\n".to_string(), T::I(64)),
                _ => (format!("  %r = freeze {} %e\n", lt.ll()), lt.clone()),
            };
            let core = format!("{build}  %e = extractvalue {tl} %full{}\n{tail}", idx(path));
            let f = s.add(&ints, ret, &core);
            s.random_cases(rng, f, 6);
            let _ = n;
        }
        // Sub-aggregates moved between two values.
        if let T::S(fields, _) = &ty {
            for (i, field) in fields.iter().enumerate() {
                if matches!(field, T::S(..) | T::A(..)) {
                    let fl = field.ll();
                    let core = format!(
                        "{build}  %e = extractvalue {tl} %full, {i}\n  %r = insertvalue {tl} zeroinitializer, {fl} %e, {i}\n"
                    );
                    let f = s.add(&ints, ty.clone(), &core);
                    s.random_cases(rng, f, 6);
                }
            }
        }
        // select, phi, constant.
        let core = format!(
            "{build}  %c1 = trunc i128 %a to i1\n  %r = select i1 %c1, {tl} %full, {tl} zeroinitializer\n"
        );
        let f = s.add(&ints, ty.clone(), &core);
        s.random_cases(rng, f, 6);
        let core = format!(
            "{build}  %c1 = trunc i128 %c to i1\n  br i1 %c1, label %l, label %j\nl:\n  br label %j\nj:\n  %r = phi {tl} [ %full, %l ], [ zeroinitializer, %entry ]\n"
        );
        let f = s.add(&ints, ty.clone(), &core);
        s.random_cases(rng, f, 6);
        let mut img = vec![0u8; ty.store() as usize];
        ty.image(rng, &mut img, 0);
        let k = const_text(&ty, &img, 0);
        let f = s.add(&[], ty.clone(), &format!("  %r = freeze {k}\n"));
        s.case(f, vec![]);
        let core = format!(
            "  %c1 = trunc i128 %a to i1\n  %r = select i1 %c1, {k}, {tl} zeroinitializer\n"
        );
        let f = s.add(&ints, ty.clone(), &core);
        s.random_cases(rng, f, 6);
        // A parameter loaded from memory: copied, and one leaf replaced.
        let f =
            s.add(&[ty.clone()], ty.clone(), "  %r = freeze {} %a\n".replace("{}", &tl).as_str());
        s.random_cases(rng, f, 6);
        let (path, lt) = &ls[ls.len() / 2];
        let core = format!(
            "  %e = extractvalue {tl} %b{}\n  %r = insertvalue {tl} %a, {} %e{}\n",
            idx(path),
            lt.ll(),
            idx(path)
        );
        let f = s.add(&[ty.clone(), ty.clone()], ty.clone(), &core);
        s.random_cases(rng, f, 6);
    }
    // `Mem` vectors: load, store, select, freeze, phi.
    let v = |n, e| T::V(n, Box::new(e));
    for vec in [
        v(3, T::I(32)),
        v(4, T::I(64)),
        v(8, T::I(1)),
        v(5, T::I(1)),
        v(3, T::F),
        v(6, T::I(16)),
        v(3, T::D),
        v(64, T::I(1)),
    ] {
        let vl = vec.ll();
        for core in [
            format!("  %r = freeze {vl} %a\n"),
            format!("  %c1 = trunc i8 %c to i1\n  %r = select i1 %c1, {vl} %a, {vl} %b\n"),
            format!(
                "  %c1 = trunc i8 %c to i1\n  %r = select i1 %c1, {vl} %a, {vl} zeroinitializer\n"
            ),
            format!(
                "  %c1 = trunc i8 %c to i1\n  br i1 %c1, label %l, label %j\nl:\n  br label %j\nj:\n  %r = phi {vl} [ %a, %l ], [ %b, %entry ]\n"
            ),
        ] {
            let f = s.add(&[vec.clone(), vec.clone(), T::I(8)], vec.clone(), &core);
            s.random_cases(rng, f, 6);
        }
    }
    s
}

/// The `.ll` constant of `ty` whose image is `img` at `off`.
fn const_text(ty: &T, img: &[u8], off: u64) -> String {
    let o = off as usize;
    let tl = ty.ll();
    let body = match ty {
        T::I(b) => {
            let mut w = vec![0u64; limbs(*b)];
            for i in 0..ty.store() as usize {
                w[i / 8] |= u64::from(img[o + i]) << (8 * (i % 8));
            }
            lit(&canon(w, *b), *b)
        }
        T::F => {
            let bits = u32::from_le_bytes(img[o..o + 4].try_into().unwrap());
            format!("0x{:016X}", f64::from(f32::from_bits(bits)).to_bits())
        }
        T::D => format!("0x{:016X}", u64::from_le_bytes(img[o..o + 8].try_into().unwrap())),
        T::P => "null".into(),
        T::S(fields, packed) => {
            let list: Vec<String> = fields
                .iter()
                .zip(ty.offsets())
                .map(|(f, at)| const_text(f, img, off + at))
                .collect();
            let inner =
                if list.is_empty() { String::new() } else { format!(" {} ", list.join(", ")) };
            if *packed { format!("<{{{inner}}}>") } else { format!("{{{inner}}}") }
        }
        T::A(n, e) => {
            let list: Vec<String> =
                (0..*n).map(|i| const_text(e, img, off + i * e.alloc())).collect();
            format!("[{}]", list.join(", "))
        }
        T::V(..) => unreachable!(),
    };
    format!("{tl} {body}")
}

/// The overflow intrinsics at every width up to 64 (and add/sub wider),
/// with register and constant operands, edge values.
pub fn overflow_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    let widths = [1u32, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 48, 63, 64, 65, 96, 127, 128, 129, 192];
    for bits in widths {
        for op in ["sadd", "uadd", "ssub", "usub", "smul", "umul"] {
            if bits > 64 && op.ends_with("mul") {
                continue;
            }
            let ret = with_flag(bits);
            let rl = ret.ll();
            s.decls
                .insert(format!("declare {rl} @llvm.{op}.with.overflow.i{bits}(i{bits}, i{bits})"));
            let e = edges(bits);
            let call = |a: &str, b: &str| {
                format!(
                    "  %r = call {rl} @llvm.{op}.with.overflow.i{bits}(i{bits} {a}, i{bits} {b})\n"
                )
            };
            let f = s.add(&[T::I(bits), T::I(bits)], ret.clone(), &call("%a", "%b"));
            for _ in 0..40 {
                let a = if rng.chance(60) { rng.pick(&e).clone() } else { value(rng, bits) };
                let b = if rng.chance(60) { rng.pick(&e).clone() } else { value(rng, bits) };
                s.case(f, ints(&[a, b], bits));
            }
            for _ in 0..2 {
                let k = rng.pick(&e).clone();
                let f = s.add(&[T::I(bits)], ret.clone(), &call("%a", &lit(&k, bits)));
                for _ in 0..8 {
                    let a = if rng.chance(60) { rng.pick(&e).clone() } else { value(rng, bits) };
                    s.case(f, ints(&[a], bits));
                }
            }
            // The two fields used separately, as the frontends do.
            let core = format!(
                "{}  %v = extractvalue {rl} %s, 0\n  %o = extractvalue {rl} %s, 1\n  %r = select i1 %o, i{bits} 0, i{bits} %v\n",
                call("%a", "%b").replace("%r =", "%s =")
            );
            let f = s.add(&[T::I(bits), T::I(bits)], T::I(bits), &core);
            for _ in 0..12 {
                let a = rng.pick(&e).clone();
                let b = value(rng, bits);
                s.case(f, ints(&[a, b], bits));
            }
        }
    }
    s
}

/// Random straight-line functions over wide integers: chains of the
/// binary operations, compares and selects, casts between the widths.
pub fn random_functions(rng: &mut Rng, count: usize) -> Suite {
    let mut s = Suite::default();
    let widths = [65u32, 96, 128, 129, 192, 256];
    for _ in 0..count {
        let bits = *rng.pick(&widths);
        let ty = T::I(bits);
        let mut core = String::new();
        let mut vals = vec!["%a".to_string(), "%b".to_string(), "%c".to_string()];
        let len = 2 + rng.below(8);
        for n in 0..len {
            let x = rng.pick(&vals).clone();
            let y = rng.pick(&vals).clone();
            let name = format!("%t{n}");
            match rng.below(6) {
                0 | 1 => {
                    let ops = ["add", "sub", "mul", "and", "or", "xor"];
                    writeln!(core, "  {name} = {} i{bits} {x}, {y}", rng.pick(&ops)).unwrap();
                }
                2 => {
                    let ops = ["shl", "lshr", "ashr"];
                    let k = rng.below(u64::from(bits));
                    writeln!(core, "  {name} = {} i{bits} {x}, {k}", rng.pick(&ops)).unwrap();
                }
                3 => {
                    let p = rng.pick(&PREDICATES);
                    writeln!(core, "  %c{n} = icmp {p} i{bits} {x}, {y}\n  {name} = select i1 %c{n}, i{bits} {y}, i{bits} {x}").unwrap();
                }
                4 => {
                    let narrow = 1 + rng.below(u64::from(bits) - 1) as u32;
                    let ext = if rng.chance(50) { "zext" } else { "sext" };
                    writeln!(core, "  %n{n} = trunc i{bits} {x} to i{narrow}\n  {name} = {ext} i{narrow} %n{n} to i{bits}").unwrap();
                }
                _ => {
                    let k = value(rng, bits);
                    writeln!(core, "  {name} = add i{bits} {x}, {}", lit(&k, bits)).unwrap();
                }
            }
            vals.push(name);
        }
        writeln!(core, "  %r = xor i{bits} {}, %t{}", rng.pick(&vals), len - 1).unwrap();
        let f = s.add(&[ty.clone(), ty.clone(), ty.clone()], ty, &core);
        s.random_cases(rng, f, 4);
    }
    s
}

// ---- running ----

/// One leg's output line per case (then per register case).
type Lines = Vec<String>;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        write!(s, "{b:02x}").unwrap();
        s
    })
}

fn c_int(bits: u32) -> &'static str {
    if bits <= 64 { "uint64_t" } else { "u128" }
}

fn c_u128(v: u128) -> String {
    format!("(((u128){:#x}ULL << 64) | {:#x}ULL)", (v >> 64) as u64, v as u64)
}

fn to_u128(img: &[u8]) -> u128 {
    img.iter().rev().fold(0, |acc, &b| acc << 8 | u128::from(b))
}

fn mask128(bits: u32) -> u128 {
    if bits >= 128 { u128::MAX } else { (1 << bits) - 1 }
}

/// The C driver: the pointer cases, then the register cases.
fn driver(suite: &Suite, rng: &mut Rng) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n#include <string.h>\n\
         typedef unsigned __int128 u128;\n",
    );
    let mut data = Vec::new();
    for f in &suite.funcs {
        writeln!(c, "void {}(void *, const void *);", f.name).unwrap();
        if f.regs {
            let params: Vec<String> =
                f.params.iter().map(|p| c_int(p.small_int().unwrap()).to_string()).collect();
            let rb = f.ret.small_int().unwrap();
            writeln!(
                c,
                "{} {}_v({});",
                c_int(rb),
                f.name,
                if params.is_empty() { "void".into() } else { params.join(", ") }
            )
            .unwrap();
            let args: Vec<String> =
                (0..f.params.len()).map(|i| format!("({})a[{i}]", params[i])).collect();
            writeln!(
                c,
                "static u128 r_{}(const u128 *a) {{ return {}_v({}); }}",
                f.name,
                f.name,
                args.join(", ")
            )
            .unwrap();
        }
    }
    c.push_str("static const struct { void (*f)(void *, const void *); unsigned off, n, s[3], out; } cases[] = {\n");
    for case in &suite.cases {
        let f = &suite.funcs[case.func];
        let off = data.len();
        let mut sizes = [0usize; 3];
        for (i, a) in case.args.iter().enumerate() {
            sizes[i] = a.len();
            data.extend_from_slice(a);
        }
        writeln!(
            c,
            "  {{ {}, {off}, {}, {{ {}, {}, {} }}, {} }},",
            f.name,
            case.args.len(),
            sizes[0],
            sizes[1],
            sizes[2],
            f.ret.store()
        )
        .unwrap();
    }
    c.push_str("  { 0, 0, 0, { 0, 0, 0 }, 0 }\n};\n");
    c.push_str("static const struct { u128 (*f)(const u128 *); u128 a[3]; u128 mask; unsigned out; } rcases[] = {\n");
    for case in &suite.cases {
        let f = &suite.funcs[case.func];
        if !f.regs {
            continue;
        }
        let mut regs = Vec::new();
        for (p, img) in f.params.iter().zip(&case.args) {
            let bits = p.small_int().unwrap();
            let garbage = u128::from(rng.next()) << 64 | u128::from(rng.next());
            let reg_bits = if bits == 1 { 8 } else { bits };
            let mut v = garbage & !mask128(reg_bits) | to_u128(img);
            if bits <= 64 {
                v &= u128::from(u64::MAX);
            }
            regs.push(c_u128(v));
        }
        regs.resize(3, "0".into());
        let rb = f.ret.small_int().unwrap();
        writeln!(
            c,
            "  {{ r_{}, {{ {} }}, {}, {} }},",
            f.name,
            regs.join(", "),
            c_u128(mask128(rb)),
            f.ret.store()
        )
        .unwrap();
    }
    c.push_str("  { 0, { 0, 0, 0 }, 0, 0 }\n};\n");
    c.push_str("static const unsigned char data[] = {");
    for (i, b) in data.iter().enumerate() {
        if i % 32 == 0 {
            c.push('\n');
        }
        write!(c, "{b},").unwrap();
    }
    c.push_str("0\n};\n");
    c.push_str(
        r#"static void put(const unsigned char *p, unsigned n, int bad) {
  for (unsigned i = 0; i < n; i++) printf("%02x", p[i]);
  printf(bad ? "!\n" : "\n");
}
int main(void) {
  static _Alignas(16) unsigned char in[3 * 128];
  static _Alignas(16) unsigned char out[16 + 512 + 16];
  for (unsigned i = 0; cases[i].f; i++) {
    memset(in, 0xcc, sizeof in);
    unsigned off = cases[i].off;
    for (unsigned k = 0; k < cases[i].n; k++) {
      memcpy(in + 128 * k, data + off, cases[i].s[k]);
      off += cases[i].s[k];
    }
    memset(out, 0xa5, sizeof out);
    cases[i].f(out + 16, in);
    int bad = 0;
    for (unsigned k = 0; k < 16; k++)
      bad |= out[k] != 0xa5 || out[16 + cases[i].out + k] != 0xa5;
    put(out + 16, cases[i].out, bad);
  }
  for (unsigned i = 0; rcases[i].f; i++) {
    u128 r = rcases[i].f(rcases[i].a) & rcases[i].mask;
    put((const unsigned char *)&r, rcases[i].out, 0);
  }
  return 0;
}
"#,
    );
    c
}

/// What [`run`] found.
#[derive(Debug, Default)]
pub struct Report {
    /// Cases compared with the interpreter.
    pub interp: usize,
    /// Cases compared with `llc` only.
    pub llc_only: usize,
    /// Cases the interpreter trapped on (undefined; not compared).
    pub undefined: usize,
    /// Register cases (garbage above the widths) compared.
    pub regs: usize,
    /// Disagreements.
    pub wrong: Vec<String>,
    /// IR instructions compiled, and the time hod took (plain leg).
    pub insts: usize,
    /// See `insts`.
    pub compile: Duration,
    /// Everything else.
    pub tools: Duration,
}

/// Expected images of each case: `Ok(Some(image))` from the interpreter,
/// `Ok(None)` where it has nothing to say, `Err(())` where it traps.
fn interpret(suite: &Suite, header: &str) -> Vec<Result<Option<Vec<u8>>, ()>> {
    let mut expected = vec![Ok(None); suite.cases.len()];
    if suite.twins.is_empty() {
        return expected;
    }
    let mut text = header.to_string();
    for d in &suite.decls {
        writeln!(text, "{d}").unwrap();
    }
    text.push_str(&suite.twins);
    let (module, bodies) = parse(&text);
    let program = Program::with_bodies(&module, &bodies).expect("a valid program");
    let mut interp = Interp::new(&program);
    for (index, case) in suite.cases.iter().enumerate() {
        let f = &suite.funcs[case.func];
        if !f.twin {
            continue;
        }
        let sym = module.find_symbol(format!("{}_v", f.name).as_bytes()).expect("a twin");
        let args: Vec<Val> = f
            .params
            .iter()
            .zip(&case.args)
            .map(|(p, img)| Val::int(p.small_int().unwrap(), to_u128(img)))
            .collect();
        let store = f.ret.store() as usize;
        expected[index] = match interp.run(sym, &args).result {
            Ok(Some(Val::Int { value, .. })) => Ok(Some(value.to_le_bytes()[..store].to_vec())),
            Ok(Some(Val::Ptr(v) | Val::Double(v))) => Ok(Some(v.to_le_bytes().to_vec())),
            Ok(Some(Val::Float(v))) => Ok(Some(v.to_le_bytes().to_vec())),
            Ok(Some(Val::Agg(agg))) => Ok(Some(agg.bytes()[..store].to_vec())),
            Ok(None) => panic!("{}: no result", f.name),
            Err(Stop::Trap(_)) => Err(()),
            // An intermediate the interpreter does not model (a vector,
            // an integer wider than 128 bits): `llc` decides.
            Err(Stop::Unsupported(_)) => Ok(None),
            Err(other) => panic!("interpreter: {other:?} in {}", f.name),
        };
    }
    expected
}

fn parse_line(line: &str) -> (Vec<u8>, bool) {
    let bad = line.ends_with('!');
    let digits = line.trim_end_matches('!');
    let bytes = (0..digits.len() / 2)
        .map(|i| u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    (bytes, bad)
}

/// Whether `got` matches `want` under the masks: the loose bits equal,
/// and with `strict` the other bits of the strict mask zero.
fn matches(ty: &T, want: &[u8], got: &[u8], strict: bool) -> bool {
    let n = ty.store() as usize;
    let (mut loose, mut tight) = (vec![0; n], vec![0; n]);
    ty.mask(false, &mut loose, 0);
    ty.mask(true, &mut tight, 0);
    got.len() == n
        && (0..n).all(|i| {
            (want[i] ^ got[i]) & loose[i] == 0 && (!strict || got[i] & tight[i] & !loose[i] == 0)
        })
}

/// Checks `suite`; `name` labels the scratch directory.
pub fn run(name: &str, suite: &Suite) -> Report {
    let major = llc_major();
    let text = suite.module(&ll_header(major));
    // `HOD_AGGR_DUMP=<dir>` writes each suite's module there.
    if let Ok(dir) = std::env::var("HOD_AGGR_DUMP") {
        std::fs::write(format!("{dir}/{name}.ll"), &text).unwrap();
    }
    let (module, bodies) = parse(&text);
    let mut report =
        Report { insts: bodies.iter().map(hod_ir::Body::inst_count).sum(), ..Report::default() };
    let expected = interpret(suite, &ll_header(major));
    let start = Instant::now();
    let scratch = Scratch::new(&format!("a64-aggr-{name}"));
    let mut legs: Vec<(String, std::path::PathBuf)> = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let options = Options { stress, ..Options::default() };
        let t = Instant::now();
        let emitted =
            compile_module(&module, &bodies, &options).unwrap_or_else(|e| panic!("{mode}: {e}"));
        if mode == "plain" {
            report.compile = t.elapsed();
        }
        legs.push((
            format!("hod-{mode}"),
            write_object(&scratch, &format!("hod-{mode}.o"), &emitted),
        ));
    }
    let llc_object = llc(&scratch, "llc", &text, &[]);
    let Some(toolchain) = Toolchain::host(&scratch) else { return report };
    let Some(linker) = toolchain.linkers.first() else { return report };
    let driver = toolchain.compile_c(&scratch, "driver", &driver(suite, &mut Rng(0x0400)), &[]);
    let mut outputs: Vec<(String, Lines)> = Vec::new();
    if let Some(object) = llc_object {
        legs.push(("llc".into(), object));
    }
    for (leg, object) in &legs {
        let program = scratch.join(leg);
        toolchain.link_ok(linker, &[], &[&driver, object], &program);
        let ran = run_with(&mut Command::new(&program), &[], Duration::from_mins(2));
        assert!(ran.success(), "{leg}: {:?}\n{}", ran.status, ran.stderr_text());
        outputs.push((leg.clone(), ran.stdout_text().lines().map(str::to_string).collect()));
    }
    let llc_lines = outputs.iter().find(|(leg, _)| leg == "llc").map(|(_, lines)| lines.clone());
    let reg_cases: Vec<usize> =
        (0..suite.cases.len()).filter(|&i| suite.funcs[suite.cases[i].func].regs).collect();
    for (leg, lines) in &outputs {
        assert_eq!(lines.len(), suite.cases.len() + reg_cases.len(), "{leg}: one line per case");
        let hod = leg != "llc";
        let mut check = |i: usize, line: &str, reg: bool| {
            let case = &suite.cases[i];
            let f = &suite.funcs[case.func];
            let (got, bad) = parse_line(line);
            let want = match &expected[i] {
                Err(()) => return,
                Ok(Some(img)) => img.clone(),
                Ok(None) => match &llc_lines {
                    Some(l) if hod && f.llc => parse_line(&l[i]).0,
                    _ => return,
                },
            };
            if !hod && !f.llc {
                return;
            }
            if bad || !matches(&f.ret, &want, &got, hod) {
                report.wrong.push(format!(
                    "{leg}{}: {}({}): expected {}, got {line}\n{}",
                    if reg { " (registers)" } else { "" },
                    f.name,
                    case.args.iter().map(|a| hex(a)).collect::<Vec<_>>().join(", "),
                    hex(&want),
                    f.core
                ));
            }
        };
        for (i, line) in lines.iter().take(suite.cases.len()).enumerate() {
            check(i, line, false);
        }
        for (k, line) in lines.iter().skip(suite.cases.len()).enumerate() {
            check(reg_cases[k], line, true);
        }
    }
    report.undefined = expected.iter().filter(|e| e.is_err()).count();
    report.interp = expected.iter().filter(|e| matches!(e, Ok(Some(_)))).count();
    report.llc_only = expected.iter().filter(|e| matches!(e, Ok(None))).count();
    report.regs = reg_cases.len();
    report.tools = start.elapsed();
    report
}

/// Panics with the first disagreements of `report`.
pub fn assert_agrees(report: &Report) {
    if report.wrong.is_empty() {
        return;
    }
    let mut message = format!("{} disagreements; the first:\n", report.wrong.len());
    for line in report.wrong.iter().take(10) {
        writeln!(message, "{line}").unwrap();
    }
    panic!("{message}");
}
