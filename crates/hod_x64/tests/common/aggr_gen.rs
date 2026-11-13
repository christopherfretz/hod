//! The x86-64 `Mem`-class group's programs (WP-126): wide integers,
//! aggregates, `Mem` vectors and the overflow intrinsics, as `.ll` text
//! checked against `hod_interp` and against `llc -O0 -fast-isel=0
//! -relocation-model=pic` of LLVM 19 and 22, under the emulated runner.
//! Adapted from the AArch64 group's generator
//! (`crates/hod_a64/tests/common/aggr_gen.rs`): the same types, values
//! and tables, with the programs reduced to what this branch lowers (no
//! `getelementptr`, no scalar load or store, no conditional branch, no
//! `Gpr` arithmetic).
//!
//! A function of a [`Suite`] is a *core*: instructions over `%a`, `%b`,
//! `%c`, `%d` (its parameters) that define `%r` (its result), starting
//! in a block named `entry`. Each core is emitted twice:
//!
//! - `@<name>(ptr %out, ptr %in0, .., ptr %in3)`: loads the
//!   parameters from the `%inK` (a `Gpr` or `Fpr` parameter as the field
//!   of a one-field struct, whose `load` is this group's, then
//!   `extractvalue`), runs the core and stores `%r` to `%out` (a `Gpr`
//!   or `Fpr` result through `insertvalue` into `{ R }`). The driver
//!   surrounds `%out` with 16-byte sentinels, so a store that writes
//!   outside the store size is caught, and fills the bytes after each
//!   parameter with garbage.
//! - `@<name>_v(<lead>, <params>)` returning `%r`, after `lead` (0 to 5)
//!   `i64` parameters it ignores, so that the parameters land at every
//!   position of the argument registers and on the stack: run by the
//!   interpreter when its parameters and result are scalars it models
//!   (the expected result), and called from C ([`Native`]) when every
//!   parameter is an integer and the result an integer that C can
//!   receive (`<= 128` bits in registers, `> 192` bits in memory,
//!   `ret_memory`), with random bits above every width.
//!
//! The expected image is the interpreter's; where there is none it is
//! `llc` 22's. Images are compared under a significance mask computed
//! from the layouts (padding never counts; the bits above an integer's
//! width must be zero in hod's stores and do not count for `llc`). A
//! case the interpreter traps on (undefined) is run but not compared.

#![allow(clippy::many_single_char_names, clippy::too_many_lines, clippy::cast_possible_truncation)]
#![allow(dead_code, reason = "each test uses part of it")]
#![allow(
    clippy::similar_names,
    clippy::format_collect,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a test generator: `.ll` text built by hand"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hod_cg::{Options, RelocModel, Stress};
use hod_interp::{Interp, Program, Stop, Val};
use hod_ir::{Body, Module};
use hod_testutil::{Scratch, run_with};

use crate::linking::Toolchain;

/// Where the parameters are in the input buffer.
const STRIDE: usize = 128;
/// The most parameters a core has.
const MAX_PARAMS: usize = 4;

/// The module header: the x86-64 data layout and triple.
pub fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// What clang writes for `-fPIE`.
pub const PIE_FLAGS: &str = "\n!llvm.module.flags = !{!0, !1}\n\
    !0 = !{i32 8, !\"PIC Level\", i32 2}\n!1 = !{i32 7, !\"PIE Level\", i32 2}\n";

/// A module and its bodies from complete `.ll` text.
pub fn parse(text: &str) -> (Module, Vec<Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        let body = parsed
            .body
            .unwrap_or_else(|| panic!("hod_ll: unsupported {:?} in\n{text}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    (module, bodies)
}

/// Position-independent options with `stress`.
pub fn pic(stress: Stress) -> Options {
    Options { reloc_model: RelocModel::Pic, stress, ..Options::default() }
}

/// The `llc` of LLVM `major`, if installed (a failure under
/// `HOD_REQUIRE_TOOLS=1`).
pub fn llc_of(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    let found = if installed.is_file() {
        Some(installed)
    } else {
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path)
            .map(|dir| dir.join(format!("llc-{major}")))
            .find(|f| f.is_file())
    };
    if found.is_none() {
        assert!(!hod_testutil::tools_required(), "llc {major} is required");
        eprintln!("note: llc {major} not found; skipping its leg");
    }
    found
}

/// `llc -O0 -fast-isel=0 -relocation-model=pic` of LLVM `major` on
/// `text`, the object written next to it.
pub fn llc(scratch: &Scratch, major: u32, name: &str, text: &str) -> Option<PathBuf> {
    let llc = llc_of(major)?;
    let input = scratch.write(&format!("{name}.ll"), text);
    let output = scratch.join(format!("{name}.o"));
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
    let outcome = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(outcome.success(), "llc {major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

/// The bytes of the `.text` sections of an object (`llvm-size -A`).
pub fn text_bytes(object: &Path) -> Option<u64> {
    let size = hod_testutil::tool("llvm-size")?;
    let mut command = Command::new(size);
    command.arg("-A").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-size: {}", outcome.stderr_text());
    Some(
        outcome
            .stdout_text()
            .lines()
            .filter(|line| line.starts_with(".text"))
            .filter_map(|line| line.split_whitespace().nth(1)?.parse::<u64>().ok())
            .sum(),
    )
}

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

    /// Its ABI alignment (the x86-64 Linux data layout: `i128` 16).
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

    /// Whether it is of class `Mem` on x86-64 (`target.rs` `class_of`).
    pub fn is_mem(&self) -> bool {
        match self {
            T::I(b) => *b > 64,
            T::F | T::D | T::P => false,
            T::S(..) | T::A(..) => true,
            T::V(n, e) => {
                if **e == T::I(1) {
                    *n > 64
                } else {
                    !(n.is_power_of_two() && matches!(self.bits(), 64 | 128))
                }
            }
        }
    }

    /// The type the wrapper stores: itself, or `{ itself }`.
    pub fn out(&self) -> T {
        if self.is_mem() { self.clone() } else { T::S(vec![self.clone()], false) }
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

    /// Whether the interpreter takes it as an argument.
    fn interp_arg(&self) -> bool {
        matches!(self, T::I(1..=128) | T::F | T::D | T::P)
    }

    fn int(&self) -> Option<u32> {
        match self {
            T::I(b) => Some(*b),
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
/// 64 and the width, the limb boundaries (carries across them), patterns.
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

/// A shift amount at `bits`: 0, 63, 64, 65, 127 and around 128 and the
/// width (below it).
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
#[allow(clippy::struct_excessive_bools, reason = "independent properties of one function")]
pub struct Func {
    /// Its name.
    pub name: String,
    /// Its parameters (at most four).
    pub params: Vec<T>,
    /// Its result.
    pub ret: T,
    /// The `i64` parameters before them in the twin.
    pub lead: usize,
    /// Whether the interpreter runs `@<name>_v`.
    pub interp: bool,
    /// Whether `@<name>_v` is compiled and called from C with garbage
    /// above the widths.
    pub native: bool,
    /// Whether every piece of the twin's parameters gets a register: LLVM
    /// 19 keeps a wide integer that does not fit whole on the stack, where
    /// LLVM 22 (and hod, `spec/abi-x64.md` §4.2) splits it, so `llc` 19's
    /// native calls are compared only then.
    pub in_registers: bool,
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
    /// The wrappers' text.
    pub text: String,
    /// The interpreted twins' text.
    pub twins: String,
    /// The twins called natively.
    pub native_twins: String,
    /// Declarations (intrinsics).
    pub decls: BTreeSet<String>,
    /// The functions.
    pub funcs: Vec<Func>,
    /// The cases.
    pub cases: Vec<Case>,
}

const NAMES: [&str; MAX_PARAMS] = ["%a", "%b", "%c", "%d"];

/// Whether C passes and receives the twin of `params` and `ret`: integer
/// parameters of at most 256 bits; an integer result of at most 128 bits
/// (registers) or of 193 to 256 bits (in memory).
fn native(params: &[T], ret: &T) -> bool {
    params.iter().all(|p| matches!(p, T::I(1..=256))) && matches!(ret, T::I(1..=128 | 193..=256))
}

impl Suite {
    /// Adds a function of `core`; returns its index.
    pub fn add(&mut self, params: &[T], ret: T, core: &str) -> usize {
        assert!(params.len() <= MAX_PARAMS);
        let index = self.funcs.len();
        let name = format!("f{index}");
        let interp = params.iter().all(T::interp_arg) && ret.interp();
        let native = native(params, &ret);
        let lead = index % 6;
        let mut w = format!(
            "define void @{name}(ptr %out, ptr %in0, ptr %in1, ptr %in2, ptr %in3) {{\nentry:\n"
        );
        for (i, p) in params.iter().enumerate() {
            let (pl, n) = (p.ll(), NAMES[i]);
            if p.is_mem() {
                writeln!(w, "  {n} = load {pl}, ptr %in{i}").unwrap();
            } else {
                writeln!(w, "  {n}.w = load {{ {pl} }}, ptr %in{i}").unwrap();
                writeln!(w, "  {n} = extractvalue {{ {pl} }} {n}.w, 0").unwrap();
            }
        }
        w.push_str(core);
        let rl = ret.ll();
        if ret.is_mem() {
            writeln!(w, "  store {rl} %r, ptr %out").unwrap();
        } else {
            writeln!(w, "  %r.w = insertvalue {{ {rl} }} undef, {rl} %r, 0").unwrap();
            writeln!(w, "  store {{ {rl} }} %r.w, ptr %out").unwrap();
        }
        w.push_str("  ret void\n}\n\n");
        self.text.push_str(&w);
        if interp || native {
            let mut list: Vec<String> = (0..lead).map(|k| format!("i64 %lead{k}")).collect();
            list.extend(params.iter().enumerate().map(|(i, p)| format!("{} {}", p.ll(), NAMES[i])));
            let text = format!(
                "define {rl} @{name}_v({}) {{\nentry:\n{core}  ret {rl} %r\n}}\n\n",
                list.join(", ")
            );
            if interp {
                self.twins.push_str(&text);
            }
            if native {
                self.native_twins.push_str(&text);
            }
        }
        let core = core.to_string();
        let pieces: usize = lead
            + usize::from(matches!(ret, T::I(193..)))
            + params.iter().map(|p| p.int().map_or(1, limbs)).sum::<usize>();
        self.funcs.push(Func {
            name,
            params: params.to_vec(),
            ret,
            lead,
            interp,
            native,
            in_registers: pieces <= 6,
            llc: true,
            core,
        });
        index
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

    /// The whole module text: header, declarations, wrappers, the twins
    /// called natively.
    pub fn module(&self) -> String {
        let mut text = header();
        for d in &self.decls {
            text.push_str(d);
            text.push('\n');
        }
        text.push_str(&self.text);
        text.push_str(&self.native_twins);
        text.push_str(PIE_FLAGS);
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

/// `w`, or 1 for zero: a divisor (division by zero is undefined, and on
/// x86-64 the runtime's `div` traps).
fn nonzero(w: Vec<u64>, bits: u32) -> Vec<u64> {
    if w.iter().all(|&l| l == 0) { small(bits, 1) } else { w }
}

fn is_division(op: &str) -> bool {
    op.ends_with("div") || op.ends_with("rem")
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
                let mut b = if shift {
                    amount(rng, bits)
                } else if rng.chance(50) {
                    rng.pick(&e).clone()
                } else {
                    value(rng, bits)
                };
                if is_division(op) {
                    b = nonzero(b, bits);
                }
                s.case(f, ints(&[a, b], bits));
            }
            // Constants on the right and on the left.
            for _ in 0..3 {
                let mut k = if shift { amount(rng, bits) } else { value(rng, bits) };
                if is_division(op) {
                    k = nonzero(k, bits);
                }
                let f = s.add(
                    std::slice::from_ref(&ty),
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
                    std::slice::from_ref(&ty),
                    ty.clone(),
                    &format!("  %r = {op} i{bits} {}, %a\n", lit(&k, bits)),
                );
                for _ in 0..6 {
                    let mut a = value(rng, bits);
                    if is_division(op) {
                        a = nonzero(a, bits);
                    }
                    s.case(f, ints(&[a], bits));
                }
            }
        }
        // Every shift amount named by the brief, as a constant.
        for op in ["shl", "lshr", "ashr"] {
            for k in [0u32, 1, 63, 64, 65, 127, 128, bits - 1] {
                if k >= bits {
                    continue;
                }
                let f = s.add(
                    std::slice::from_ref(&ty),
                    ty.clone(),
                    &format!("  %r = {op} i{bits} %a, {k}\n"),
                );
                for w in e.iter().take(8) {
                    s.case(f, ints(std::slice::from_ref(w), bits));
                }
            }
        }
    }
    s
}

/// The producers whose results carry bits above the width in hod's
/// representation, followed by each consumer that depends on them:
/// compares, right shifts, division, extensions, truncation, stores.
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
                (format!("  %r = freeze i{bits} %t\n"), ty.clone()),
            ];
            for p in PREDICATES {
                consumers.push((format!("  %r = icmp {p} i{bits} %t, %c\n"), T::I(1)));
            }
            if bits <= 128 {
                for d in ["udiv", "sdiv", "urem", "srem"] {
                    consumers.push((format!("  %r = {d} i{bits} %t, %c\n"), ty.clone()));
                }
                consumers.push((format!("  %r = sitofp i{bits} %t to double\n"), T::D));
                consumers.push((format!("  %r = uitofp i{bits} %t to float\n"), T::F));
            }
            for (consumer, ret) in consumers {
                let shiftish = consumer.contains("shr");
                let core = format!("  %t = {producer} i{bits} %a, %b\n{consumer}");
                let f = s.add(&[ty.clone(), ty.clone(), ty.clone()], ret, &core);
                for _ in 0..12 {
                    let a = value(rng, bits);
                    let b = if producer == "shl" { amount(rng, bits) } else { value(rng, bits) };
                    let mut c = if shiftish { amount(rng, bits) } else { value(rng, bits) };
                    if consumer.contains("div") || consumer.contains("rem") {
                        c = nonzero(c, bits);
                    }
                    s.case(f, ints(&[a, b, c], bits));
                }
            }
        }
    }
    s
}

/// Every predicate at every width, as a value and feeding `select`;
/// `select` with a parameter condition; `freeze`; `phi`s of wide values
/// (copies between blocks, a swap).
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
        // select on a parameter, with constants and registers; phis.
        let k = value(rng, bits);
        for core in [
            format!("  %r = select i1 %c, i{bits} %a, i{bits} %b\n"),
            format!("  %r = select i1 %c, i{bits} {}, i{bits} %a\n", lit(&k, bits)),
            format!("  %r = select i1 %c, i{bits} %a, i{bits} 0\n"),
            format!("  %r = select i1 true, i{bits} %a, i{bits} %b\n"),
            format!("  %r = select i1 false, i{bits} %a, i{bits} %b\n"),
            format!("  %r = freeze i{bits} %a\n"),
            format!("  %f = freeze i{bits} {}\n  %r = xor i{bits} %f, %a\n", lit(&k, bits)),
            format!(
                "  %x = add i{bits} %a, 1\n  br label %j\nj:\n  %r = phi i{bits} [ %x, %entry ]\n"
            ),
            format!(
                "  br label %c0\nc0:\n  %p = phi i{bits} [ %a, %entry ]\n  %q = phi i{bits} [ %b, %entry ]\n  br label %d0\nd0:\n  %p2 = phi i{bits} [ %q, %c0 ]\n  %q2 = phi i{bits} [ %p, %c0 ]\n  %k = phi i{bits} [ {}, %c0 ]\n  br label %e0\ne0:\n  %s = sub i{bits} %p2, %q2\n  %r = xor i{bits} %s, %k\n",
                lit(&k, bits)
            ),
        ] {
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
            std::slice::from_ref(&ty),
            ty.clone(),
            &format!("  %f = freeze i{bits} poison\n  %r = or i{bits} %f, %a\n"),
        );
        s.funcs[f].llc = false;
        s.case(f, ints(&[value(rng, bits)], bits));
    }
    s
}

/// An `f64` whose integer part fits `bits` (signed when `signed`), or an
/// edge (0, ±0.5, a power of two near the limits).
fn fp_value(rng: &mut Rng, bits: u32, signed: bool) -> f64 {
    let room = if signed { bits - 1 } else { bits };
    let k = rng.below(u64::from(room.min(1000))) as i32;
    let mut v = (rng.next() as f64 / u64::MAX as f64 + 0.5) * 2f64.powi(k);
    if v >= 2f64.powi(room as i32) {
        v /= 2.0;
    }
    if signed && rng.chance(50) {
        v = -v;
    }
    match rng.below(10) {
        0 => 0.0,
        1 => 0.5,
        2 if signed => -0.75,
        3 => 2f64.powi(room as i32 - 1),
        _ => v,
    }
}

/// Every extension and truncation between narrow and wide widths, the
/// pointer casts, the bitcasts between wide integers and vectors, the
/// conversions between floating point and wide integers.
pub fn cast_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    let all = [1u32, 8, 33, 63, 64, 65, 96, 127, 128, 129, 200, 256];
    for &from in &all {
        for &to in &all {
            let ops: &[&str] = match to.cmp(&from) {
                std::cmp::Ordering::Greater => &["zext", "sext"],
                std::cmp::Ordering::Less => &["trunc"],
                std::cmp::Ordering::Equal => &[],
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
    let add = |s: &mut Suite, rng: &mut Rng, params: &[T], ret: T, core: &str| {
        let f = s.add(params, ret, core);
        s.random_cases(rng, f, 12);
    };
    for bits in [65, 128, 256] {
        add(&mut s, rng, &[T::P], T::I(bits), &format!("  %r = ptrtoint ptr %a to i{bits}\n"));
        add(
            &mut s,
            rng,
            &[T::I(bits)],
            T::I(bits),
            &format!("  %p = inttoptr i{bits} %a to ptr\n  %r = ptrtoint ptr %p to i{bits}\n"),
        );
        add(&mut s, rng, &[T::I(bits)], T::P, &format!("  %r = inttoptr i{bits} %a to ptr\n"));
    }
    let v = |n, e| T::V(n, Box::new(e));
    for (int, vec) in [
        (128, v(2, T::I(64))),
        (128, v(4, T::F)),
        (128, v(16, T::I(8))),
        (96, v(3, T::I(32))),
        (256, v(4, T::I(64))),
        (256, v(8, T::F)),
        (192, v(3, T::D)),
        (128, v(128, T::I(1))),
        (256, v(256, T::I(1))),
    ] {
        let vl = vec.ll();
        add(
            &mut s,
            rng,
            &[T::I(int)],
            T::I(int),
            &format!("  %x = bitcast i{int} %a to {vl}\n  %r = bitcast {vl} %x to i{int}\n"),
        );
        add(&mut s, rng, &[T::I(int)], vec.clone(), &format!("  %r = bitcast i{int} %a to {vl}\n"));
        add(
            &mut s,
            rng,
            std::slice::from_ref(&vec),
            T::I(int),
            &format!("  %r = bitcast {vl} %a to i{int}\n"),
        );
        add(
            &mut s,
            rng,
            std::slice::from_ref(&vec),
            vec.clone(),
            &format!("  %x = bitcast {vl} %a to i{int}\n  %r = bitcast i{int} %x to {vl}\n"),
        );
    }
    // Floating point and integers of 65 to 128 bits, through the runtime.
    for bits in [65u32, 96, 127, 128] {
        for (fp, t) in [("double", T::D), ("float", T::F)] {
            for (op, signed) in [("fptosi", true), ("fptoui", false)] {
                let f = s.add(
                    std::slice::from_ref(&t),
                    T::I(bits),
                    &format!("  %r = {op} {fp} %a to i{bits}\n"),
                );
                for _ in 0..16 {
                    let v = fp_value(rng, bits.min(if t == T::F { 127 } else { 1023 }), signed);
                    let img = if t == T::F {
                        (v as f32).to_bits().to_le_bytes().to_vec()
                    } else {
                        v.to_bits().to_le_bytes().to_vec()
                    };
                    s.case(f, vec![img]);
                }
            }
            for op in ["sitofp", "uitofp"] {
                let f =
                    s.add(&[T::I(bits)], t.clone(), &format!("  %r = {op} i{bits} %a to {fp}\n"));
                for w in edges(bits).into_iter().take(12) {
                    s.case(f, ints(&[w], bits));
                }
                for _ in 0..6 {
                    let w = value(rng, bits);
                    s.case(f, ints(&[w], bits));
                }
            }
        }
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
        a(5, T::I(64)),
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

/// The parameters of the aggregate cores: two integers, a `double` and
/// a `float`.
fn agg_params() -> [T; 4] {
    [T::I(128), T::I(96), T::D, T::F]
}

/// A leaf of type `ty` made from the parameters (an integer from `%a`
/// or `%b`, a `double` from `%c`, a `float` from `%d`, a pointer from
/// `%a`), and the instructions computing it.
fn leaf_from(ty: &T, n: usize) -> (String, String) {
    let name = format!("%v{n}");
    let (src, bits) = [("%a", 128), ("%b", 96)][n % 2];
    let code = match ty {
        T::I(b) if *b == bits => format!("  {name} = add i{b} {src}, {n}\n"),
        T::I(b) if *b < bits => format!("  {name} = trunc i{bits} {src} to i{b}\n"),
        T::I(b) => format!("  {name} = sext i{bits} {src} to i{b}\n"),
        T::F => return (String::new(), "%d".into()),
        T::D => return (String::new(), "%c".into()),
        T::P => format!("  {name} = inttoptr i128 %a to ptr\n"),
        _ => unreachable!(),
    };
    (code, name)
}

/// Every aggregate built leaf by leaf with `insertvalue` (from `undef`,
/// `zeroinitializer` and `poison`), read back with `extractvalue` (each
/// leaf, and whole sub-aggregates moved between two values), selected,
/// frozen, merged by a `phi`, loaded and stored; constant aggregates;
/// aggregate parameters (loaded from memory); `Mem` vectors.
pub fn aggregate_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    let params = agg_params();
    for ty in aggregates() {
        let tl = ty.ll();
        let mut ls = Vec::new();
        leaves(&ty, &mut Vec::new(), &mut ls);
        for base in ["undef", "zeroinitializer", "poison"] {
            let mut core = String::new();
            let mut cur = base.to_string();
            for (n, (path, lt)) in ls.iter().enumerate() {
                let (code, v) = leaf_from(lt, n);
                core.push_str(&code);
                writeln!(core, "  %s{n} = insertvalue {tl} {cur}, {} {v}{}", lt.ll(), idx(path))
                    .unwrap();
                cur = format!("%s{n}");
            }
            writeln!(core, "  %r = freeze {tl} {cur}").unwrap();
            let f = s.add(&params, ty.clone(), &core);
            s.random_cases(rng, f, 6);
        }
        if ls.is_empty() {
            // `{}` and `[0 x i32]`: values without bytes.
            let f = s.add(&[], ty.clone(), &format!("  %x = freeze {tl} undef\n  %r = select i1 true, {tl} %x, {tl} zeroinitializer\n"));
            s.case(f, vec![]);
            continue;
        }
        // Into a zero aggregate, one leaf.
        for (n, (path, lt)) in ls.iter().enumerate() {
            let (code, v) = leaf_from(lt, n);
            let core = format!(
                "{code}  %r = insertvalue {tl} zeroinitializer, {} {v}{}\n",
                lt.ll(),
                idx(path)
            );
            let f = s.add(&params, ty.clone(), &core);
            s.random_cases(rng, f, 6);
        }
        // A full value, then each leaf extracted into the result.
        let mut build = String::new();
        let mut cur = "undef".to_string();
        for (n, (path, lt)) in ls.iter().enumerate() {
            let (code, v) = leaf_from(lt, n);
            build.push_str(&code);
            writeln!(build, "  %s{n} = insertvalue {tl} {cur}, {} {v}{}", lt.ll(), idx(path))
                .unwrap();
            cur = format!("%s{n}");
        }
        writeln!(build, "  %full = freeze {tl} {cur}").unwrap();
        for (path, lt) in &ls {
            let core = format!("{build}  %r = extractvalue {tl} %full{}\n", idx(path));
            let f = s.add(&params, lt.clone(), &core);
            s.random_cases(rng, f, 6);
        }
        // Sub-aggregates moved between two values.
        if let T::S(fields, _) = &ty {
            for (i, field) in fields.iter().enumerate() {
                if matches!(field, T::S(..) | T::A(..)) {
                    let fl = field.ll();
                    let core = format!(
                        "{build}  %e = extractvalue {tl} %full, {i}\n  %r = insertvalue {tl} zeroinitializer, {fl} %e, {i}\n"
                    );
                    let f = s.add(&params, ty.clone(), &core);
                    s.random_cases(rng, f, 6);
                }
            }
        }
        // select, phi, constant.
        let core = format!(
            "{build}  %c1 = trunc i128 %a to i1\n  %r = select i1 %c1, {tl} %full, {tl} zeroinitializer\n"
        );
        let f = s.add(&params, ty.clone(), &core);
        s.random_cases(rng, f, 6);
        let core = format!(
            "{build}  br label %j\nj:\n  %m = phi {tl} [ %full, %entry ]\n  %z = phi {tl} [ zeroinitializer, %entry ]\n  %c1 = trunc i96 %b to i1\n  %r = select i1 %c1, {tl} %z, {tl} %m\n"
        );
        let f = s.add(&params, ty.clone(), &core);
        s.random_cases(rng, f, 6);
        let mut img = vec![0u8; ty.store() as usize];
        ty.image(rng, &mut img, 0);
        let k = const_text(&ty, &img, 0);
        let f = s.add(&[], ty.clone(), &format!("  %r = freeze {k}\n"));
        s.case(f, vec![]);
        let core = format!(
            "  %c1 = trunc i128 %a to i1\n  %r = select i1 %c1, {k}, {tl} zeroinitializer\n"
        );
        let f = s.add(&params, ty.clone(), &core);
        s.random_cases(rng, f, 6);
        // A parameter loaded from memory: copied, and one leaf replaced.
        let f = s.add(std::slice::from_ref(&ty), ty.clone(), &format!("  %r = freeze {tl} %a\n"));
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
        v(3, T::F),
        v(6, T::I(16)),
        v(3, T::D),
        v(128, T::I(1)),
        v(5, T::I(8)),
    ] {
        let vl = vec.ll();
        for core in [
            format!("  %r = freeze {vl} %a\n"),
            format!("  %r = select i1 %c, {vl} %a, {vl} %b\n"),
            format!("  %r = select i1 %c, {vl} %a, {vl} zeroinitializer\n"),
            format!(
                "  br label %j\nj:\n  %x = phi {vl} [ %a, %entry ]\n  %y = phi {vl} [ %b, %entry ]\n  %r = select i1 %c, {vl} %y, {vl} %x\n"
            ),
        ] {
            let f = s.add(&[vec.clone(), vec.clone(), T::I(1)], vec.clone(), &core);
            s.random_cases(rng, f, 6);
        }
    }
    s
}

/// The `.ll` constant of `ty` whose image is `img` at `off`.
pub fn const_text(ty: &T, img: &[u8], off: u64) -> String {
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

/// The overflow intrinsics at every width (multiplication up to 256
/// bits), with register and constant operands, edge values; the two
/// fields used separately.
pub fn overflow_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    let widths =
        [1u32, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 48, 63, 64, 65, 96, 127, 128, 129, 192, 256];
    for bits in widths {
        for op in ["sadd", "uadd", "ssub", "usub", "smul", "umul"] {
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
            // The two fields used separately, as the front ends do.
            let s_call = call("%a", "%b").replace("%r =", "%s =");
            let mut cores = vec![
                (format!("{s_call}  %r = extractvalue {rl} %s, 0\n"), T::I(bits)),
                (format!("{s_call}  %r = extractvalue {rl} %s, 1\n"), T::I(1)),
            ];
            if bits > 64 {
                cores.push((
                    format!(
                        "{s_call}  %v = extractvalue {rl} %s, 0\n  %o = extractvalue {rl} %s, 1\n  %r = select i1 %o, i{bits} 0, i{bits} %v\n"
                    ),
                    T::I(bits),
                ));
            }
            for (core, ret) in cores {
                let f = s.add(&[T::I(bits), T::I(bits)], ret, &core);
                for _ in 0..12 {
                    let a = rng.pick(&e).clone();
                    let b = value(rng, bits);
                    s.case(f, ints(&[a, b], bits));
                }
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

/// One leg's output line per case (then per native case).
type Lines = Vec<String>;

/// Lower-case hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        write!(s, "{b:02x}").unwrap();
        s
    })
}

fn to_u128(img: &[u8]) -> u128 {
    img.iter().rev().take(16).fold(0, |acc, &b| acc << 8 | u128::from(b))
}

/// The C type and the 64-bit pieces of a native parameter.
fn c_param(bits: u32) -> (&'static str, usize) {
    match bits {
        ..=64 => ("uint64_t", 1),
        128 => ("u128", 2),
        _ => ("uint64_t", limbs(bits)),
    }
}

/// The C driver: the pointer cases, then the native cases.
fn driver(suite: &Suite, rng: &mut Rng) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n#include <string.h>\n\
         typedef unsigned __int128 u128;\n\
         typedef struct { uint64_t w[4]; } w4;\n\
         /* What llc 19 calls for a signed 128-bit multiply with overflow; libgcc lacks it. */\n\
         __int128 __muloti4(__int128 a, __int128 b, int *o) { __int128 r; *o = __builtin_mul_overflow(a, b, &r); return r; }\n",
    );
    let mut data = Vec::new();
    for f in &suite.funcs {
        writeln!(
            c,
            "void {}(void *, const void *, const void *, const void *, const void *);",
            f.name
        )
        .unwrap();
        if !f.native {
            continue;
        }
        let mut params: Vec<&str> = vec!["uint64_t"; f.lead];
        let mut args: Vec<String> = (0..f.lead).map(|k| format!("a[{k}]")).collect();
        let mut at = f.lead;
        for p in &f.params {
            let (cty, pieces) = c_param(p.int().unwrap());
            if cty == "u128" {
                params.push(cty);
                args.push(format!("((u128)a[{}] << 64 | a[{at}])", at + 1));
            } else {
                for k in 0..pieces {
                    params.push(cty);
                    args.push(format!("a[{}]", at + k));
                }
            }
            at += pieces;
        }
        let rb = f.ret.int().unwrap();
        let store = f.ret.store();
        let (rty, mask) = match rb {
            ..=64 => ("uint64_t", true),
            65..=128 => ("u128", true),
            _ => ("w4", false),
        };
        writeln!(
            c,
            "{rty} {}_v({});",
            f.name,
            if params.is_empty() { "void".into() } else { params.join(", ") }
        )
        .unwrap();
        let masking = if mask && rb < 128 {
            format!(" r &= ((({rty})1 << {rb}) - 1);")
        } else {
            String::new()
        };
        writeln!(
            c,
            "static void r_{}(const uint64_t *a, unsigned char *o) {{ {rty} r = {}_v({});{masking} memcpy(o, &r, {store}); }}",
            f.name,
            f.name,
            args.join(", ")
        )
        .unwrap();
    }
    c.push_str("static const struct { void (*f)(void *, const void *, const void *, const void *, const void *); unsigned off, n, s[4], out; } cases[] = {\n");
    for case in &suite.cases {
        let f = &suite.funcs[case.func];
        let off = data.len();
        let mut sizes = [0usize; MAX_PARAMS];
        for (i, a) in case.args.iter().enumerate() {
            sizes[i] = a.len();
            data.extend_from_slice(a);
        }
        writeln!(
            c,
            "  {{ {}, {off}, {}, {{ {}, {}, {}, {} }}, {} }},",
            f.name,
            case.args.len(),
            sizes[0],
            sizes[1],
            sizes[2],
            sizes[3],
            f.ret.out().store()
        )
        .unwrap();
    }
    c.push_str("  { 0, 0, 0, { 0, 0, 0, 0 }, 0 }\n};\n");
    c.push_str(
        "static const struct { void (*f)(const uint64_t *, unsigned char *); unsigned out; uint64_t a[18]; } rcases[] = {\n",
    );
    for case in &suite.cases {
        let f = &suite.funcs[case.func];
        if !f.native {
            continue;
        }
        let mut words: Vec<u64> = (0..f.lead).map(|_| rng.next()).collect();
        for (p, img) in f.params.iter().zip(&case.args) {
            let bits = p.int().unwrap();
            let pieces = c_param(bits).1;
            let mut limbs_of: Vec<u64> = (0..pieces)
                .map(|k| {
                    let mut word = [0u8; 8];
                    for (i, byte) in word.iter_mut().enumerate() {
                        *byte = img.get(8 * k + i).copied().unwrap_or(0);
                    }
                    u64::from_le_bytes(word)
                })
                .collect();
            // Garbage above the width (an `i1` as a byte: `bool`).
            let reg_bits = if bits == 1 { 8 } else { bits };
            let top = reg_bits - 64 * (pieces as u32 - 1);
            if top < 64 {
                *limbs_of.last_mut().unwrap() |= rng.next() & !((1u64 << top) - 1);
            }
            words.extend(limbs_of);
        }
        let list: Vec<String> = words.iter().map(|w| format!("{w:#x}ULL")).collect();
        writeln!(c, "  {{ r_{}, {}, {{ {} }} }},", f.name, f.ret.store(), list.join(", ")).unwrap();
    }
    c.push_str("  { 0, 0, { 0 } }\n};\n");
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
  static _Alignas(16) unsigned char in[4 * 128];
  static _Alignas(16) unsigned char out[16 + 512 + 16];
  for (unsigned i = 0; cases[i].f; i++) {
    memset(in, 0xcc, sizeof in);
    unsigned off = cases[i].off;
    for (unsigned k = 0; k < cases[i].n; k++) {
      memcpy(in + 128 * k, data + off, cases[i].s[k]);
      off += cases[i].s[k];
    }
    memset(out, 0xa5, sizeof out);
    cases[i].f(out + 16, in, in + 128, in + 256, in + 384);
    int bad = 0;
    for (unsigned k = 0; k < 16; k++)
      bad |= out[k] != 0xa5 || out[16 + cases[i].out + k] != 0xa5;
    put(out + 16, cases[i].out, bad);
  }
  for (unsigned i = 0; rcases[i].f; i++) {
    unsigned char o[64];
    rcases[i].f(rcases[i].a, o);
    put(o, rcases[i].out, 0);
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
    /// Native cases (garbage above the widths) compared.
    pub native: usize,
    /// Of them, results returned in memory.
    pub in_memory: usize,
    /// The legs run.
    pub legs: Vec<String>,
    /// Disagreements.
    pub wrong: Vec<String>,
    /// IR instructions compiled, and the time hod took (plain leg).
    pub insts: usize,
    /// See `insts`.
    pub compile: Duration,
    /// Text bytes of hod's plain object and of `llc` 22's.
    pub text: (u64, u64),
    /// Everything else.
    pub tools: Duration,
}

/// Expected images of each case: `Ok(Some(image))` from the interpreter,
/// `Ok(None)` where it has nothing to say, `Err(())` where it traps.
fn interpret(suite: &Suite) -> Vec<Result<Option<Vec<u8>>, ()>> {
    let mut expected = vec![Ok(None); suite.cases.len()];
    if suite.twins.is_empty() {
        return expected;
    }
    let mut text = header();
    for d in &suite.decls {
        writeln!(text, "{d}").unwrap();
    }
    text.push_str(&suite.twins);
    let (module, bodies) = parse(&text);
    let program = Program::with_bodies(&module, &bodies).expect("a valid program");
    let mut interp = Interp::new(&program);
    for (index, case) in suite.cases.iter().enumerate() {
        let f = &suite.funcs[case.func];
        if !f.interp {
            continue;
        }
        let sym = module.find_symbol(format!("{}_v", f.name).as_bytes()).expect("a twin");
        let mut args: Vec<Val> =
            (0..f.lead).map(|k| Val::int(64, k as u128 * 0x1234_5678)).collect();
        args.extend(f.params.iter().zip(&case.args).map(|(p, img)| match p {
            T::I(b) => Val::int(*b, to_u128(img)),
            T::F => Val::Float(to_u128(img) as u32),
            T::D => Val::Double(to_u128(img) as u64),
            T::P => Val::Ptr(to_u128(img) as u64),
            _ => unreachable!(),
        }));
        let store = f.ret.store() as usize;
        expected[index] = match interp.run(sym, &args).result {
            Ok(Some(Val::Int { value, .. })) => Ok(Some(value.to_le_bytes()[..store].to_vec())),
            Ok(Some(Val::Ptr(v) | Val::Double(v))) => Ok(Some(v.to_le_bytes().to_vec())),
            Ok(Some(Val::Float(v))) => Ok(Some(v.to_le_bytes().to_vec())),
            Ok(Some(Val::X86Fp80(v))) => Ok(Some(v.to_le_bytes()[..10].to_vec())),
            Ok(Some(Val::Agg(agg))) => Ok(Some(agg.bytes()[..store].to_vec())),
            Ok(None) => panic!("{}: no result", f.name),
            Err(Stop::Trap(_)) => Err(()),
            // An intermediate the interpreter does not model: `llc` decides.
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

/// Whether `got` matches `want` under the masks of `ty`: the loose bits
/// equal, and with `strict` the other bits of the strict mask zero.
fn matches(ty: &T, want: &[u8], got: &[u8], strict: bool) -> bool {
    let n = ty.store() as usize;
    let (mut loose, mut tight) = (vec![0; n], vec![0; n]);
    ty.mask(false, &mut loose, 0);
    ty.mask(true, &mut tight, 0);
    let want_at = |i: usize| want.get(i).copied().unwrap_or(0);
    got.len() == n
        && (0..n).all(|i| {
            (want_at(i) ^ got[i]) & loose[i] == 0 && (!strict || got[i] & tight[i] & !loose[i] == 0)
        })
}

/// A hook that changes the module text before every leg compiles it
/// (the mixed objects of a test).
pub type Prepare<'a> = &'a dyn Fn(&str) -> String;

/// Checks `suite`; `name` labels the scratch directory. The legs: hod
/// (plain and under every stress mode), `llc` 22 and `llc` 19, each one
/// object and one emulated process.
pub fn run(name: &str, suite: &Suite) -> Report {
    let text = suite.module();
    // `HOD_AGGR_DUMP=<dir>` writes each suite's module there.
    if let Ok(dir) = std::env::var("HOD_AGGR_DUMP") {
        std::fs::write(format!("{dir}/x64-{name}.ll"), &text).unwrap();
    }
    let (module, bodies) = parse(&text);
    let mut report =
        Report { insts: bodies.iter().map(Body::inst_count).sum(), ..Report::default() };
    let expected = interpret(suite);
    let start = Instant::now();
    let scratch = Scratch::new(&format!("x64-aggr-{name}"));
    let mut legs: Vec<(String, PathBuf)> = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let t = Instant::now();
        let bytes = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{mode}: {e}"));
        if mode == "plain" {
            report.compile = t.elapsed();
        }
        legs.push((format!("hod-{mode}"), scratch.write(&format!("hod-{mode}.o"), bytes)));
    }
    for major in [22, 19] {
        if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &text) {
            legs.push((format!("llc{major}"), object));
        }
    }
    if let (Some(hod), Some(llc)) = (
        text_bytes(&legs[0].1),
        legs.iter().find(|(leg, _)| leg == "llc22").and_then(|(_, o)| text_bytes(o)),
    ) {
        report.text = (hod, llc);
    }
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return report };
    let Some(linker) = toolchain.linkers.first() else { return report };
    let driver =
        toolchain.compile_c(&scratch, "driver", &driver(suite, &mut Rng(0x0126)), &["-fPIE"]);
    let mut outputs: Vec<(String, Lines)> = Vec::new();
    for (leg, object) in &legs {
        let program = scratch.join(leg);
        toolchain.link_ok(linker, &["-pie"], &[&driver, object], &program);
        let ran = run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(10));
        assert!(ran.success(), "{leg}: {:?}\n{}", ran.status, ran.stderr_text());
        outputs.push((leg.clone(), ran.stdout_text().lines().map(str::to_string).collect()));
        report.legs.push(leg.clone());
    }
    let reference = outputs.iter().find(|(leg, _)| leg == "llc22").map(|(_, l)| l.clone());
    let native: Vec<usize> =
        (0..suite.cases.len()).filter(|&i| suite.funcs[suite.cases[i].func].native).collect();
    for (leg, lines) in &outputs {
        assert_eq!(lines.len(), suite.cases.len() + native.len(), "{leg}: one line per case");
        let hod = leg.starts_with("hod");
        let mut check = |i: usize, line: &str, reg: Option<usize>| {
            let case = &suite.cases[i];
            let f = &suite.funcs[case.func];
            if !hod && !f.llc || leg == "llc19" && reg.is_some() && !f.in_registers {
                return;
            }
            let ty = if reg.is_some() { f.ret.clone() } else { f.ret.out() };
            let (got, bad) = parse_line(line);
            let want = match &expected[i] {
                Err(()) => return,
                Ok(Some(img)) => img.clone(),
                Ok(None) => match &reference {
                    Some(l) if leg != "llc22" && f.llc => {
                        let at = reg.map_or(i, |k| suite.cases.len() + k);
                        parse_line(&l[at]).0
                    }
                    _ => return,
                },
            };
            if bad || !matches(&ty, &want, &got, hod) {
                report.wrong.push(format!(
                    "{leg}{}: {}({}): expected {}, got {line}\n{}",
                    if reg.is_some() { " (native)" } else { "" },
                    f.name,
                    case.args.iter().map(|a| hex(a)).collect::<Vec<_>>().join(", "),
                    hex(&want),
                    f.core
                ));
            }
        };
        for (i, line) in lines.iter().take(suite.cases.len()).enumerate() {
            check(i, line, None);
        }
        for (k, line) in lines.iter().skip(suite.cases.len()).enumerate() {
            check(native[k], line, Some(k));
        }
    }
    report.undefined = expected.iter().filter(|e| e.is_err()).count();
    report.interp = expected.iter().filter(|e| matches!(e, Ok(Some(_)))).count();
    report.llc_only = expected.iter().filter(|e| matches!(e, Ok(None))).count();
    report.native = native.len();
    report.in_memory = native
        .iter()
        .filter(|&&i| matches!(suite.funcs[suite.cases[i].func].ret, T::I(193..)))
        .count();
    report.tools = start.elapsed();
    report
}

/// Panics with the first disagreements of `report`.
pub fn assert_agrees(report: &Report) {
    if report.wrong.is_empty() {
        return;
    }
    let mut message = format!("{} disagreements;", report.wrong.len());
    let mut legs: Vec<&str> =
        report.wrong.iter().map(|w| w.split(':').next().unwrap_or("")).collect();
    legs.dedup();
    for leg in &legs {
        let count = report.wrong.iter().filter(|w| w.split(':').next() == Some(leg)).count();
        write!(message, " {leg}: {count}").unwrap();
    }
    message.push_str("; the first of each:\n");
    for leg in legs {
        for line in report.wrong.iter().filter(|w| w.split(':').next() == Some(leg)).take(4) {
            writeln!(message, "{line}").unwrap();
        }
    }
    panic!("{message}");
}
