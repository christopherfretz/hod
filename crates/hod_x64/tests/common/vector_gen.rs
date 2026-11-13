//! Generated vector programs for the x86-64 vector group (WP-132):
//! the random functions of the interpreter's vector oracle
//! (`crates/hod_interp/tests/vector_oracle.rs`, copied: a test file of
//! another crate cannot be shared), compiled by hod (plain and under
//! every stress mode) and by `llc -O0 -fast-isel=0` of LLVM 22 and 19,
//! linked with a C driver and run under the x86-64 runner. Every function
//! stores what it computes into a global and calls `emit(ptr, i64 len)`;
//! the interpreter's `emit` prints a byte that is not written (a poison
//! lane's) as `??`, which matches any byte.
//!
//! The interpreter is the reference: hod's legs must match it on every
//! function it runs; `llc`'s disagreements are counted and reported (its
//! `-O0` vector lowering has known bugs), not failed. A function hod
//! refuses is dropped from the batch and its refusal counted by name.

#![allow(clippy::many_single_char_names, clippy::too_many_lines, clippy::cast_precision_loss)]
#![allow(dead_code, reason = "each test uses part of it")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "a test generator"
)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use hod_cg::Stress;
use hod_interp::{Interp, Program, Stop, Val};
use hod_testutil::{Scratch, run_with};

use crate::aggr_gen::{header, llc_of, parse, pic, text_bytes};
use crate::linking::Toolchain;

// ---- types ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum E {
    I(u32),
    F,
    D,
    P,
}

impl E {
    fn bits(self) -> u32 {
        match self {
            E::I(bits) => bits,
            E::F => 32,
            E::D | E::P => 64,
        }
    }

    fn ir(self) -> String {
        match self {
            E::I(bits) => format!("i{bits}"),
            E::F => "float".into(),
            E::D => "double".into(),
            E::P => "ptr".into(),
        }
    }

    fn mangle(self) -> String {
        match self {
            E::I(bits) => format!("i{bits}"),
            E::F => "f32".into(),
            E::D => "f64".into(),
            E::P => "p0".into(),
        }
    }

    fn is_float(self) -> bool {
        matches!(self, E::F | E::D)
    }
}

/// A vector of `n` lanes, or a scalar when `n` is 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct T {
    e: E,
    n: u32,
}

impl T {
    const fn v(e: E, n: u32) -> T {
        T { e, n }
    }

    const fn s(e: E) -> T {
        T { e, n: 0 }
    }

    fn ir(self) -> String {
        if self.n == 0 { self.e.ir() } else { format!("<{} x {}>", self.n, self.e.ir()) }
    }

    fn mangle(self) -> String {
        if self.n == 0 { self.e.mangle() } else { format!("v{}{}", self.n, self.e.mangle()) }
    }

    fn lanes(self) -> u32 {
        self.n.max(1)
    }

    fn bits(self) -> u64 {
        u64::from(self.lanes()) * u64::from(self.e.bits())
    }

    /// The store size.
    fn size(self) -> u64 {
        self.bits().div_ceil(8)
    }

    fn with(self, e: E) -> T {
        T { e, n: self.n }
    }

    fn mask(self) -> T {
        self.with(E::I(1))
    }
}

// ---- randomness ----

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

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<X: Copy>(&mut self, xs: &[X]) -> X {
        xs[self.below(xs.len() as u64) as usize]
    }

    /// A lane value of `bits` bits: mostly the edges.
    fn int(&mut self, bits: u32) -> u128 {
        let mask = if bits >= 128 { u128::MAX } else { (1u128 << bits) - 1 };
        let top = 1u128 << (bits - 1);
        let wide = u128::from(self.next()) | (u128::from(self.next()) << 64);
        let value = match self.below(10) {
            0 => 0,
            1 => 1,
            2 => mask,
            3 => top,
            4 => top.wrapping_sub(1),
            5 => u128::from(self.below(16)),
            6 => mask.wrapping_sub(u128::from(self.below(16))),
            _ => wide,
        };
        value & mask
    }

    /// A float lane (as the bits of an `f64` that holds it exactly when
    /// `single`): mostly ordinary numbers, now and then an edge.
    fn float(&mut self, single: bool) -> u64 {
        let ordinary = [0.0, -0.0, 1.0, -1.0, 0.5, 1.5, -2.5, 3.0, 100.25, -7.75, 1e10, -1e-3];
        let value: f64 = match self.below(20) {
            0 => f64::INFINITY,
            1 => {
                if single {
                    f64::from(f32::MAX)
                } else {
                    f64::MAX
                }
            }
            2 => {
                if single {
                    f64::from(f32::from_bits(1))
                } else {
                    f64::from_bits(1)
                }
            }
            3 => 16_777_217.0,
            4..10 => {
                // A random finite number of some size.
                let mantissa = (self.next() % 1_000_000) as f64 / 1000.0;
                let scale = [1e-6, 1e-3, 1.0, 1e3, 1e9][self.below(5) as usize];
                if self.chance(50) { -mantissa * scale } else { mantissa * scale }
            }
            _ => self.pick(&ordinary),
        };
        if single { f64::from(value as f32).to_bits() } else { value.to_bits() }
    }
}

/// The text of a lane constant.
fn lane_text(e: E, raw: u128) -> String {
    match e {
        E::I(1) => if raw & 1 == 1 { "true" } else { "false" }.into(),
        E::I(bits) => {
            let shift = 128 - bits;
            (((raw << shift) as i128) >> shift).to_string()
        }
        E::F | E::D => format!("0x{:016X}", raw as u64),
        E::P => unreachable!("no pointer constants"),
    }
}

// ---- one function ----

const INT_OPS: [&str; 13] = [
    "add", "sub", "mul", "and", "or", "xor", "shl", "lshr", "ashr", "udiv", "sdiv", "urem", "srem",
];
const ICMP: [&str; 10] = ["eq", "ne", "ugt", "uge", "ult", "ule", "sgt", "sge", "slt", "sle"];
const FCMP: [&str; 16] = [
    "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "ueq", "ugt", "uge", "ult", "ule",
    "une", "uno", "true",
];

pub struct Fun<'r> {
    rng: &'r mut Rng,
    decls: &'r mut BTreeMap<String, String>,
    name: String,
    globals: Vec<String>,
    helpers: Vec<String>,
    lines: Vec<String>,
    vals: Vec<(String, T)>,
    temps: u32,
    globals_made: u32,
    blocks: u32,
    base: bool,
}

impl Fun<'_> {
    fn tmp(&mut self) -> String {
        self.temps += 1;
        format!("%v{}", self.temps)
    }

    fn line(&mut self, text: impl std::fmt::Display) {
        self.lines.push(format!("  {text}"));
    }

    /// `%v = <text>`, which is of type `t`; not offered as an operand.
    fn def(&mut self, text: impl std::fmt::Display) -> String {
        let value = self.tmp();
        self.line(format!("{value} = {text}"));
        value
    }

    fn global(&mut self, ty: &str, init: &str) -> String {
        self.globals_made += 1;
        let name = format!("@{}.g{}", self.name, self.globals_made);
        self.globals.push(format!("{name} = global {ty} {init}"));
        name
    }

    fn konst(&mut self, t: T) -> String {
        let lane = |rng: &mut Rng| match t.e {
            E::I(bits) => lane_text(t.e, rng.int(bits)),
            E::F => lane_text(t.e, u128::from(rng.float(true))),
            E::D => lane_text(t.e, u128::from(rng.float(false))),
            E::P => unreachable!(),
        };
        if t.n == 0 {
            return lane(self.rng);
        }
        let lanes: Vec<String> =
            (0..t.n).map(|_| format!("{} {}", t.e.ir(), lane(self.rng))).collect();
        format!("<{}>", lanes.join(", "))
    }

    fn splat(t: T, value: &str) -> String {
        if t.n == 0 {
            return value.to_owned();
        }
        let lanes: Vec<String> = (0..t.n).map(|_| format!("{} {value}", t.e.ir())).collect();
        format!("<{}>", lanes.join(", "))
    }

    fn base_global(&mut self) -> String {
        let name = format!("@{}.base", self.name);
        if !self.base {
            self.base = true;
            self.globals.push(format!("{name} = global [272 x i8] zeroinitializer"));
        }
        name
    }

    /// The address of the base variable, as an `i64` or a splat of it.
    fn base_int(&mut self, n: u32) -> String {
        let base = self.base_global();
        let int = self.def(format!("ptrtoint ptr {base} to i64"));
        if n == 0 {
            return int;
        }
        let one = self.def(format!("insertelement <{n} x i64> poison, i64 {int}, i64 0"));
        self.def(format!(
            "shufflevector <{n} x i64> {one}, <{n} x i64> poison, <{n} x i32> zeroinitializer"
        ))
    }

    /// Pointers into the base variable.
    fn pointers(&mut self, n: u32) -> String {
        let base = self.base_global();
        if n == 0 {
            let at = self.rng.below(256);
            return self.def(format!("getelementptr i8, ptr {base}, i64 {at}"));
        }
        let splat = self.base_int(n);
        let offsets: Vec<String> = (0..n).map(|_| format!("i64 {}", self.rng.below(256))).collect();
        let global = self.global(&format!("<{n} x i64>"), &format!("<{}>", offsets.join(", ")));
        let offsets = self.def(format!("load <{n} x i64>, ptr {global}"));
        let sum = self.def(format!("add <{n} x i64> {splat}, {offsets}"));
        self.def(format!("inttoptr <{n} x i64> {sum} to <{n} x ptr>"))
    }

    /// A new input of type `t`, loaded from a variable.
    fn input(&mut self, t: T) -> String {
        let value = if t.e == E::P {
            self.pointers(t.n)
        } else {
            let init = self.konst(t);
            let global = self.global(&t.ir(), &init);
            self.def(format!("load {}, ptr {global}", t.ir()))
        };
        self.vals.push((value.clone(), t));
        value
    }

    /// An operand of type `t`: an earlier value or a new input.
    fn get(&mut self, t: T) -> String {
        let have: Vec<usize> = (0..self.vals.len()).filter(|&at| self.vals[at].1 == t).collect();
        if !have.is_empty() && self.rng.chance(60) {
            let at = self.rng.pick(&have);
            return self.vals[at].0.clone();
        }
        self.input(t)
    }

    /// Offers `value` as an operand and has the function emit it.
    fn result(&mut self, value: &str, t: T) {
        self.vals.push((value.to_owned(), t));
        let (value, t) = if t.e == E::P {
            let int = t.with(E::I(64));
            let base = self.base_int(t.n);
            let address = self.def(format!("ptrtoint {} {value} to {}", t.ir(), int.ir()));
            (self.def(format!("sub {} {address}, {base}", int.ir())), int)
        } else {
            (value.to_owned(), t)
        };
        let out = self.global(&t.ir(), "zeroinitializer");
        self.line(format!("store {} {value}, ptr {out}", t.ir()));
        self.line(format!("call void @emit(ptr {out}, i64 {})", t.size()));
    }

    fn call(&mut self, ret: T, name: &str, args: &[(T, String)]) -> String {
        let types: Vec<String> = args.iter().map(|(t, _)| t.ir()).collect();
        self.decls
            .entry(name.to_owned())
            .or_insert_with(|| format!("declare {} @{name}({})", ret.ir(), types.join(", ")));
        let args: Vec<String> = args.iter().map(|(t, v)| format!("{} {v}", t.ir())).collect();
        self.def(format!("call {} @{name}({})", ret.ir(), args.join(", ")))
    }

    /// The smaller of `a` and `b` by some comparison, in a block's
    /// `lines`: a function of the lines and a fresh name.
    fn min_text(&mut self, t: T, a: &str, b: &str) -> (Vec<String>, String) {
        let (c, r) = (self.tmp(), self.tmp());
        let compare = if t.e.is_float() {
            format!("{c} = fcmp olt {} {a}, {b}", t.ir())
        } else {
            format!("{c} = icmp ult {} {a}, {b}", t.ir())
        };
        let select = format!("{r} = select {} {c}, {} {a}, {} {b}", t.mask().ir(), t.ir(), t.ir());
        (vec![compare, select], r)
    }

    // ---- the operations ----

    fn op(&mut self, t: T) {
        if self.rng.chance(40) {
            return self.common(t);
        }
        match t.e {
            E::I(1) => self.mask_op(t),
            E::I(_) => self.int_op(t),
            E::F | E::D => self.float_op(t),
            E::P => self.ptr_op(t),
        }
    }

    fn int_op(&mut self, t: T) {
        let E::I(bits) = t.e else { unreachable!() };
        match self.rng.below(9) {
            0..3 => {
                let op = self.rng.pick(&INT_OPS);
                let a = self.get(t);
                let mut b = self.get(t);
                let top = (1u128 << (bits - 1)) - 1;
                match op {
                    "shl" | "lshr" | "ashr" if self.rng.chance(85) => {
                        let limit = if self.rng.chance(85) { bits - 1 } else { 2 * bits - 1 };
                        let limit = Self::splat(t, &limit.to_string());
                        b = self.def(format!("and {} {b}, {limit}", t.ir()));
                    }
                    "udiv" | "urem" => {
                        b = self.def(format!("or {} {b}, {}", t.ir(), Self::splat(t, "1")));
                    }
                    "sdiv" | "srem" => {
                        if self.rng.chance(80) {
                            let keep = Self::splat(t, &top.to_string());
                            b = self.def(format!("and {} {b}, {keep}", t.ir()));
                        }
                        b = self.def(format!("or {} {b}, {}", t.ir(), Self::splat(t, "1")));
                    }
                    _ => {}
                }
                let r = self.def(format!("{op} {} {a}, {b}", t.ir()));
                self.result(&r, t);
            }
            3 => {
                let pred = self.rng.pick(&ICMP);
                let (a, b) = (self.get(t), self.get(t));
                let c = self.def(format!("icmp {pred} {} {a}, {b}", t.ir()));
                if self.rng.chance(50) {
                    self.result(&c, t.mask());
                } else {
                    let r = self.def(format!(
                        "select {} {c}, {} {a}, {} {b}",
                        t.mask().ir(),
                        t.ir(),
                        t.ir()
                    ));
                    self.result(&r, t);
                }
            }
            4 => {
                let a = self.get(t);
                if self.rng.chance(25) {
                    let to = t.with(self.rng.pick(&[E::F, E::D]));
                    let op = self.rng.pick(&["sitofp", "uitofp"]);
                    let r = self.def(format!("{op} {} {a} to {}", t.ir(), to.ir()));
                    return self.result(&r, to);
                }
                let mut to_bits = self.rng.pick(&[1, 8, 16, 32, 64, 128]);
                if t.n == 3 && to_bits == 8 && bits >= 64 {
                    // The selection DAG of `llc` 19 and 22 truncates
                    // `<3 x i64>` and `<3 x i128>` to `<3 x i8>` wrongly
                    // (`ff ff 00` for `ff 0b ff`; GlobalISel is right;
                    // `vectors.rs` has the case), and those alone of
                    // lengths 2 to 16 and widths 16 to 128.
                    to_bits = 16;
                }
                if to_bits == bits {
                    return self.result(&a, t);
                }
                let to = t.with(E::I(to_bits));
                let op = if to_bits < bits { "trunc" } else { self.rng.pick(&["zext", "sext"]) };
                let r = self.def(format!("{op} {} {a} to {}", t.ir(), to.ir()));
                self.result(&r, to);
            }
            5 => {
                let op = self
                    .rng
                    .pick(&["add", "mul", "and", "or", "xor", "smax", "smin", "umax", "umin"]);
                let a = self.get(t);
                let r = self.call(
                    T::s(t.e),
                    &format!("llvm.vector.reduce.{op}.{}", t.mangle()),
                    &[(t, a)],
                );
                self.result(&r, T::s(t.e));
            }
            6 | 7 => {
                let mut ops = vec![
                    "abs",
                    "ctpop",
                    "ctlz",
                    "cttz",
                    "bitreverse",
                    "smax",
                    "smin",
                    "umax",
                    "umin",
                ];
                ops.extend(["uadd.sat", "usub.sat", "sadd.sat", "ssub.sat", "fshl", "fshr"]);
                if bits % 16 == 0 {
                    ops.push("bswap");
                }
                let op = self.rng.pick(&ops);
                let name = format!("llvm.{op}.{}", t.mangle());
                let a = self.get(t);
                let flag = T::s(E::I(1));
                let flag_value = || if op == "abs" { "false" } else { "true" };
                let args = match op {
                    "abs" | "ctlz" | "cttz" => {
                        let on = if self.rng.chance(50) { "true" } else { flag_value() };
                        vec![(t, a), (flag, on.to_owned())]
                    }
                    "ctpop" | "bitreverse" | "bswap" => vec![(t, a)],
                    "fshl" | "fshr" => {
                        let (b, c) = (self.get(t), self.get(t));
                        vec![(t, a), (t, b), (t, c)]
                    }
                    _ => {
                        let b = self.get(t);
                        vec![(t, a), (t, b)]
                    }
                };
                let r = self.call(t, &name, &args);
                self.result(&r, t);
            }
            _ => {
                let op = self.rng.pick(&["scmp", "ucmp"]);
                let to = t.with(E::I(self.rng.pick(&[8, 32])));
                let (a, b) = (self.get(t), self.get(t));
                let name = format!("llvm.{op}.{}.{}", to.mangle(), t.mangle());
                let r = self.call(to, &name, &[(t, a), (t, b)]);
                self.result(&r, to);
            }
        }
    }

    fn mask_op(&mut self, t: T) {
        match self.rng.below(6) {
            0 | 1 => {
                let op = self.rng.pick(&["and", "or", "xor", "add", "sub", "mul"]);
                let (a, b) = (self.get(t), self.get(t));
                let r = self.def(format!("{op} {} {a}, {b}", t.ir()));
                self.result(&r, t);
            }
            2 => {
                let pred = self.rng.pick(&ICMP);
                let (a, b) = (self.get(t), self.get(t));
                let r = self.def(format!("icmp {pred} {} {a}, {b}", t.ir()));
                self.result(&r, t);
            }
            3 => {
                let to = t.with(E::I(self.rng.pick(&[8, 16, 32, 64])));
                let op = self.rng.pick(&["zext", "sext"]);
                let a = self.get(t);
                let r = self.def(format!("{op} {} {a} to {}", t.ir(), to.ir()));
                self.result(&r, to);
            }
            4 => {
                let op = self.rng.pick(&["and", "or", "xor", "add", "umax", "smin"]);
                let a = self.get(t);
                let r = self.call(
                    T::s(t.e),
                    &format!("llvm.vector.reduce.{op}.{}", t.mangle()),
                    &[(t, a)],
                );
                self.result(&r, T::s(t.e));
            }
            _ => {
                // A mask from a comparison of integers.
                let from = t.with(E::I(self.rng.pick(&[8, 32, 64])));
                let pred = self.rng.pick(&ICMP);
                let (a, b) = (self.get(from), self.get(from));
                let r = self.def(format!("icmp {pred} {} {a}, {b}", from.ir()));
                self.result(&r, t);
            }
        }
    }

    fn float_op(&mut self, t: T) {
        match self.rng.below(8) {
            0 | 1 => {
                let op = self.rng.pick(&["fadd", "fsub", "fmul", "fdiv", "frem"]);
                let (a, b) = (self.get(t), self.get(t));
                let r = self.def(format!("{op} {} {a}, {b}", t.ir()));
                self.result(&r, t);
            }
            2 => {
                let a = self.get(t);
                let r = self.def(format!("fneg {} {a}", t.ir()));
                self.result(&r, t);
            }
            3 => {
                let pred = self.rng.pick(&FCMP);
                let (a, b) = (self.get(t), self.get(t));
                let r = self.def(format!("fcmp {pred} {} {a}, {b}", t.ir()));
                self.result(&r, t.mask());
            }
            4 => {
                let a = self.get(t);
                let (op, to) = match self.rng.below(3) {
                    0 if t.e == E::F => ("fpext", t.with(E::D)),
                    0 => ("fptrunc", t.with(E::F)),
                    _ => (
                        self.rng.pick(&["fptosi", "fptoui"]),
                        t.with(E::I(self.rng.pick(&[8, 16, 32, 64]))),
                    ),
                };
                let r = self.def(format!("{op} {} {a} to {}", t.ir(), to.ir()));
                self.result(&r, to);
            }
            5 => {
                let op = self.rng.pick(&["fadd", "fmul"]);
                let start = self.get(T::s(t.e));
                let a = self.get(t);
                let name = format!("llvm.vector.reduce.{op}.{}", t.mangle());
                let r = self.call(T::s(t.e), &name, &[(T::s(t.e), start), (t, a)]);
                self.result(&r, T::s(t.e));
            }
            _ => {
                let op = self.rng.pick(&[
                    "fabs",
                    "copysign",
                    "sqrt",
                    "floor",
                    "ceil",
                    "trunc",
                    "round",
                    "roundeven",
                    "rint",
                    "nearbyint",
                    "fma",
                ]);
                let mut a = self.get(t);
                let args = match op {
                    "copysign" => vec![(t, a), (t, self.get(t))],
                    "fma" => {
                        let (b, c) = (self.get(t), self.get(t));
                        vec![(t, a), (t, b), (t, c)]
                    }
                    "sqrt" => {
                        a = self.call(t, &format!("llvm.fabs.{}", t.mangle()), &[(t, a)]);
                        vec![(t, a)]
                    }
                    _ => vec![(t, a)],
                };
                let r = self.call(t, &format!("llvm.{op}.{}", t.mangle()), &args);
                self.result(&r, t);
            }
        }
    }

    fn ptr_op(&mut self, t: T) {
        let (a, b) = (self.get(t), self.get(t));
        if self.rng.chance(50) {
            let pred = self.rng.pick(&["eq", "ne", "ult", "ugt", "ule", "uge"]);
            let r = self.def(format!("icmp {pred} {} {a}, {b}", t.ir()));
            self.result(&r, t.mask());
        } else {
            let (lines, r) = self.min_text(t, &a, &b);
            for line in lines {
                self.line(line);
            }
            self.result(&r, t);
        }
    }

    /// The types of as many bits as `t`, other than `t`.
    fn same_bits(t: T) -> Vec<T> {
        let bits = t.bits();
        let mut out = Vec::new();
        for e in [E::I(1), E::I(8), E::I(16), E::I(32), E::I(64), E::F, E::D] {
            let lane = u64::from(e.bits());
            if bits.is_multiple_of(lane) && bits / lane <= 256 {
                out.push(T::v(e, (bits / lane) as u32));
            }
        }
        if bits <= 128 {
            out.push(T::s(E::I(bits as u32)));
        }
        match bits {
            32 => out.push(T::s(E::F)),
            64 => out.push(T::s(E::D)),
            _ => {}
        }
        out.retain(|&other| other != t);
        out
    }

    fn common(&mut self, t: T) {
        let n = t.n;
        let lane_t = T::s(t.e);
        match self.rng.below(12) {
            0 => {
                let cond = self.get(t.mask());
                let (a, b) = (self.get(t), self.get(t));
                let r = self.def(format!(
                    "select {} {cond}, {} {a}, {} {b}",
                    t.mask().ir(),
                    t.ir(),
                    t.ir()
                ));
                self.result(&r, t);
            }
            1 => {
                let cond = self.get(T::s(E::I(1)));
                let (a, b) = (self.get(t), self.get(t));
                let r = self.def(format!("select i1 {cond}, {} {a}, {} {b}", t.ir(), t.ir()));
                self.result(&r, t);
            }
            2 => {
                let a = self.get(t);
                let index = self.index(n);
                let r = self.def(format!("extractelement {} {a}, {index}", t.ir()));
                self.result(&r, lane_t);
            }
            3 => {
                let a = self.get(t);
                let x = self.get(lane_t);
                let index = self.index(n);
                let r =
                    self.def(format!("insertelement {} {a}, {} {x}, {index}", t.ir(), lane_t.ir()));
                self.result(&r, t);
            }
            4 => {
                let (a, b) = (self.get(t), self.get(t));
                let m = self.rng.pick(&[1, 2, 3, 4, 8, 16, n]);
                let picks: Vec<String> = (0..m)
                    .map(|_| {
                        if self.rng.chance(10) {
                            "i32 poison".to_owned()
                        } else {
                            format!("i32 {}", self.rng.below(2 * u64::from(n)))
                        }
                    })
                    .collect();
                let to = T::v(t.e, m);
                let r = self.def(format!(
                    "shufflevector {} {a}, {} {b}, <{m} x i32> <{}>",
                    t.ir(),
                    t.ir(),
                    picks.join(", ")
                ));
                self.result(&r, to);
            }
            5 => {
                let a = self.get(t);
                let r = self.def(format!("freeze {} {a}", t.ir()));
                self.result(&r, t);
            }
            6 if t.e != E::P => {
                let to = self.rng.pick(&Self::same_bits(t));
                let a = self.get(t);
                let r = self.def(format!("bitcast {} {a} to {}", t.ir(), to.ir()));
                self.result(&r, to);
            }
            7 if t.e != E::P => {
                // Stored as one type and loaded as another of the same
                // size, at an address that need not be aligned.
                let size = t.size();
                let mut kinds: Vec<T> = Self::same_bits(T::v(E::I(8), size as u32))
                    .into_iter()
                    .filter(|other| other.size() == size)
                    .collect();
                kinds.push(t);
                let to = self.rng.pick(&kinds);
                let a = self.get(t);
                let base = self.base_global();
                let at = self.rng.below(16);
                let p = self.def(format!("getelementptr i8, ptr {base}, i64 {at}"));
                self.line(format!("store {} {a}, ptr {p}, align 1", t.ir()));
                let r = self.def(format!("load {}, ptr {p}, align 1", to.ir()));
                self.result(&r, to);
            }
            // `llc` 19 cannot widen the operands of `llvm.vector.insert`
            // for other lengths, one lane, or masks ("Don't know how to
            // widen the operands for INSERT_SUBVECTOR").
            8 if n >= 2 && n.is_power_of_two() && t.e != E::I(1) => {
                let sub = 2 << self.rng.below(u64::from(n.ilog2()));
                let at = self.rng.below(u64::from(n / sub)) * u64::from(sub);
                let part = T::v(t.e, sub);
                let a = self.get(t);
                if self.rng.chance(50) {
                    let name = format!("llvm.vector.extract.{}.{}", part.mangle(), t.mangle());
                    let r = self.call(part, &name, &[(t, a), (T::s(E::I(64)), at.to_string())]);
                    self.result(&r, part);
                } else {
                    let b = self.get(part);
                    let name = format!("llvm.vector.insert.{}.{}", t.mangle(), part.mangle());
                    let r =
                        self.call(t, &name, &[(t, a), (part, b), (T::s(E::I(64)), at.to_string())]);
                    self.result(&r, t);
                }
            }
            9 => {
                // A call of a function that takes and returns vectors.
                self.blocks += 1;
                let helper = format!("@{}.h{}", self.name, self.blocks);
                let (lines, r) = self.min_text(t, "%a", "%b");
                let mut text = format!(
                    "define internal {} {helper}({} %a, {} %b) {{\n",
                    t.ir(),
                    t.ir(),
                    t.ir()
                );
                for line in lines {
                    let _ = writeln!(text, "  {line}");
                }
                let _ = writeln!(text, "  ret {} {r}\n}}", t.ir());
                self.helpers.push(text);
                let (a, b) = (self.get(t), self.get(t));
                let r =
                    self.def(format!("call {} {helper}({} {a}, {} {b})", t.ir(), t.ir(), t.ir()));
                self.result(&r, t);
            }
            10 => {
                // Two arms and a `phi`.
                let cond = self.get(T::s(E::I(1)));
                let (a, b) = (self.get(t), self.get(t));
                self.blocks += 1;
                let k = self.blocks;
                self.line(format!("br i1 {cond}, label %l{k}, label %r{k}"));
                self.lines.push(format!("l{k}:"));
                let (lines, x) = self.min_text(t, &a, &b);
                for line in lines {
                    self.line(line);
                }
                self.line(format!("br label %j{k}"));
                self.lines.push(format!("r{k}:"));
                let (lines, y) = self.min_text(t, &b, &a);
                for line in lines {
                    self.line(line);
                }
                self.line(format!("br label %j{k}"));
                self.lines.push(format!("j{k}:"));
                let r = self.def(format!("phi {} [ {x}, %l{k} ], [ {y}, %r{k} ]", t.ir()));
                self.result(&r, t);
            }
            _ => {
                // Back to the kind's own operations.
                match t.e {
                    E::I(1) => self.mask_op(t),
                    E::I(_) => self.int_op(t),
                    E::F | E::D => self.float_op(t),
                    E::P => self.ptr_op(t),
                }
            }
        }
    }

    /// An index operand for a vector of `n` lanes: a constant, or loaded,
    /// and now and then past the end.
    fn index(&mut self, n: u32) -> String {
        let ty = self.rng.pick(&["i32", "i64"]);
        let value = if self.rng.chance(5) {
            u64::from(n) + self.rng.below(3)
        } else {
            self.rng.below(u64::from(n))
        };
        if self.rng.chance(50) {
            return format!("{ty} {value}");
        }
        let global = self.global(ty, &value.to_string());
        let loaded = self.def(format!("load {ty}, ptr {global}"));
        format!("{ty} {loaded}")
    }
}

/// The primary type of function `seed`.
fn shape(rng: &mut Rng) -> T {
    let e = rng.pick(&[
        E::I(8),
        E::I(8),
        E::I(16),
        E::I(32),
        E::I(32),
        E::I(64),
        E::I(1),
        E::I(1),
        E::F,
        E::F,
        E::D,
        E::P,
        E::I(128),
    ]);
    let large = if e.bits() <= 32 { 64 } else { 32 };
    let mut n = rng.pick(&[1, 2, 2, 3, 3, 4, 4, 4, 5, 8, 8, 8, 13, 16, 16, large]);
    if e == E::I(128) {
        n = n.min(4);
    }
    T::v(e, n)
}

/// One function, `@t<seed>`: its text (globals, helpers, definition).
pub fn function(seed: u64, decls: &mut BTreeMap<String, String>) -> String {
    let mut rng = Rng(seed.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ 0x5eed);
    let t = shape(&mut rng);
    let ops = 1 + rng.below(4);
    let mut f = Fun {
        rng: &mut rng,
        decls,
        name: format!("t{seed}"),
        globals: Vec::new(),
        helpers: Vec::new(),
        lines: Vec::new(),
        vals: Vec::new(),
        temps: 0,
        globals_made: 0,
        blocks: 0,
        base: false,
    };
    for _ in 0..ops {
        f.op(t);
    }
    let mut text = String::new();
    for global in &f.globals {
        let _ = writeln!(text, "{global}");
    }
    for helper in &f.helpers {
        let _ = writeln!(text, "{helper}");
    }
    let _ = writeln!(text, "define void @t{seed}() {{\nentry:");
    for line in &f.lines {
        let _ = writeln!(text, "{line}");
    }
    let _ = writeln!(text, "  ret void\n}}\n");
    text
}

// ---- running ----

/// What the interpreter says function `name` emits: `e <hex>` lines with
/// `??` for an unwritten byte; or the trap.
fn interpret(module: &hod_ir::Module, program: &Program<'_>, name: &str) -> Result<String, Stop> {
    let out = Rc::new(RefCell::new(String::new()));
    let mut interp = Interp::new(program);
    let sink = Rc::clone(&out);
    interp.host("emit", move |cx, args| {
        let (Some(&Val::Ptr(addr)), Some(len)) = (args.first(), args.get(1).and_then(Val::as_u64))
        else {
            panic!("emit({args:?})");
        };
        let mut line = String::from("e ");
        for at in 0..len {
            match cx.memory.read(addr + at, 1) {
                Ok(byte) => {
                    let _ = write!(line, "{:02x}", byte[0]);
                }
                Err(_) => line.push_str("??"),
            }
        }
        sink.borrow_mut().push_str(&line);
        sink.borrow_mut().push('\n');
        Ok(None)
    });
    let sym = module.find_symbol(name.as_bytes()).unwrap_or_else(|| panic!("no @{name}"));
    interp.run(sym, &[]).result?;
    let lines = out.borrow().clone();
    Ok(lines)
}

/// Whether `actual` is `expected` with each `??` any byte.
fn matches(expected: &str, actual: &str) -> bool {
    let (e, a): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    e.len() == a.len()
        && e.iter().zip(&a).all(|(e, a)| {
            e.len() == a.len()
                && e.as_bytes()
                    .chunks(2)
                    .zip(a.as_bytes().chunks(2))
                    .all(|(e, a)| e == b"??" || e == a)
        })
}

const DRIVER: &str = r#"#include <stdio.h>
void emit(const unsigned char *p, long n) {
  fputs("e ", stdout);
  for (long i = 0; i < n; i++) printf("%02x", p[i]);
  putchar('\n');
}
"#;

/// What a run of generated functions showed.
#[derive(Default, Debug)]
pub struct Tally {
    /// Functions generated.
    pub generated: usize,
    /// Functions hod compiled and the interpreter ran (compared).
    pub run: usize,
    /// Functions the interpreter trapped on (undefined; not compared).
    pub trapped: usize,
    /// The traps, by kind.
    pub traps: BTreeMap<String, usize>,
    /// Functions hod refused, by stable name.
    pub refused: BTreeMap<&'static str, usize>,
    /// Bytes the interpreter left unwritten (poison).
    pub poison_bytes: usize,
    /// hod's disagreements with the interpreter (failures).
    pub wrong: Vec<String>,
    /// `llc`'s disagreements with the interpreter, by major (reported).
    pub llc_wrong: BTreeMap<u32, usize>,
    /// Batches `llc` failed on (it aborts on some vector shapes), by
    /// major (reported).
    pub llc_crashed: BTreeMap<u32, usize>,
    /// The legs run.
    pub legs: Vec<String>,
    /// Calls the interpreter has no values for, by what it lacks, checked
    /// against `llc` 22 on both sides instead.
    pub by_llc: BTreeMap<String, usize>,
    /// Text bytes of hod's plain objects and of `llc` 22's.
    pub text: (u64, u64),
    /// Instructions compiled and hod's (plain) compile time.
    pub insts: usize,
    /// See `insts`.
    pub compile: Duration,
}

fn module_text(functions: &[(u64, String)], decls: &BTreeMap<String, String>) -> String {
    let mut source = header();
    source.push_str("\ndeclare void @emit(ptr, i64)\n");
    for decl in decls.values() {
        let _ = writeln!(source, "{decl}");
    }
    source.push('\n');
    for (_, text) in functions {
        source.push_str(text);
    }
    source
}

/// `llc -O0 -fast-isel=0` of LLVM `major` on `text`: the object, `Err`
/// where it fails, `None` if it is not installed.
fn llc(scratch: &Scratch, major: u32, stem: &str, text: &str) -> Option<Result<PathBuf, ()>> {
    let llc = llc_of(major)?;
    let input = scratch.write(&format!("{stem}llc{major}.ll"), text);
    let output = scratch.join(format!("{stem}llc{major}.o"));
    let mut command = std::process::Command::new(llc);
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
    Some(if outcome.success() { Ok(output) } else { Err(()) })
}

/// hod's refusal of one function's module (`Ok(None)` if it compiles);
/// an internal error is `Err`.
fn refusal(text: &str) -> Result<Option<&'static str>, String> {
    let (module, bodies) = parse(text);
    match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::ALL)) {
        Ok(_) => Ok(None),
        Err(hod_x64::ModuleError::Function {
            error: hod_cg::CgError::Unsupported { construct },
            ..
        }) => Ok(Some(construct)),
        Err(e) => Err(format!("hod: {e}")),
    }
}

/// Generates functions `seeds`, keeps those hod compiles, and checks hod
/// (plain, stressed) and `llc` 22/19 against the interpreter.
pub fn check_batch(seeds: std::ops::Range<u64>, tally: &mut Tally) {
    check_functions(
        &format!("{}-{}", seeds.start, seeds.end),
        seeds.map(|seed| (seed, None)),
        tally,
    );
}

/// Checks the given functions (`(seed, text)`, a `None` text generated
/// from the seed).
pub fn check_functions(
    name: &str,
    functions: impl IntoIterator<Item = (u64, Option<String>)>,
    tally: &mut Tally,
) {
    let mut decls = BTreeMap::new();
    let mut kept = Vec::new();
    for (seed, text) in functions {
        tally.generated += 1;
        let text = text.unwrap_or_else(|| function(seed, &mut decls));
        let alone = module_text(&[(seed, text.clone())], &decls);
        match refusal(&alone) {
            Ok(Some(construct)) => *tally.refused.entry(construct).or_default() += 1,
            Ok(None) => kept.push((seed, text)),
            Err(e) => tally.wrong.push(format!("t{seed}: {e}\n{text}")),
        }
    }
    let source = module_text(&kept, &decls);
    let (module, bodies) = parse(&source);
    let program = Program::with_bodies(&module, &bodies)
        .unwrap_or_else(|stop| panic!("interpreter: {stop:?}"));
    let mut expected = Vec::new();
    for (seed, text) in &kept {
        match interpret(&module, &program, &format!("t{seed}")) {
            Ok(lines) => {
                tally.poison_bytes += lines.matches("??").count();
                expected.push((*seed, text, lines));
            }
            Err(Stop::Trap(trap)) => {
                tally.trapped += 1;
                *tally.traps.entry(format!("{trap:?}")).or_default() += 1;
            }
            Err(other) => panic!("t{seed}: {other:?}\n{text}"),
        }
    }
    tally.run += expected.len();
    if expected.is_empty() {
        return;
    }
    let mut driver = String::from(DRIVER);
    for (seed, ..) in &expected {
        let _ = writeln!(driver, "void t{seed}(void);");
    }
    driver.push_str("int main(void) {\n");
    for (seed, ..) in &expected {
        let _ = writeln!(driver, "  puts(\"t{seed}\"); fflush(stdout); t{seed}();");
    }
    driver.push_str("  return 0;\n}\n");

    let scratch = Scratch::new(&format!("x64-vector-{name}"));
    if let Ok(dir) = std::env::var("HOD_VECTOR_DUMP") {
        std::fs::write(format!("{dir}/x64-vector-{name}.ll"), &source).expect("write");
    }
    let mut legs: Vec<(String, PathBuf)> = Vec::new();
    tally.insts += bodies.iter().map(hod_ir::Body::inst_count).sum::<usize>();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let t = Instant::now();
        let bytes = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{mode}: {e}"));
        if mode == "plain" {
            tally.compile += t.elapsed();
        }
        legs.push((format!("hod-{mode}"), scratch.write(&format!("hod-{mode}.o"), bytes)));
    }
    for major in [22, 19] {
        match llc(&scratch, major, "", &source) {
            Some(Ok(object)) => legs.push((format!("llc{major}"), object)),
            Some(Err(())) => *tally.llc_crashed.entry(major).or_default() += 1,
            None => {}
        }
    }
    if let (Some(hod), Some(llc)) = (
        text_bytes(&legs[0].1),
        legs.iter().find(|(leg, _)| leg == "llc22").and_then(|(_, o)| text_bytes(o)),
    ) {
        tally.text.0 += hod;
        tally.text.1 += llc;
    }
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let Some(linker) = toolchain.linkers.first() else { return };
    let c = toolchain.compile_c(&scratch, "driver", &driver, &["-fPIE"]);
    for (leg, object) in &legs {
        let program = scratch.join(leg);
        let lm = std::path::Path::new("-lm");
        toolchain.link_ok(linker, &["-pie"], &[c.as_path(), object.as_path(), lm], &program);
        let ran = run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(5));
        if !tally.legs.contains(leg) {
            tally.legs.push(leg.clone());
        }
        let mut got: BTreeMap<u64, String> = BTreeMap::new();
        let mut current = None;
        for line in ran.stdout_text().lines() {
            if let Some(seed) = line.strip_prefix('t').and_then(|seed| seed.parse().ok()) {
                current = Some(seed);
                got.insert(seed, String::new());
            } else if let Some(seed) = current {
                let entry = got.get_mut(&seed).expect("inserted");
                entry.push_str(line);
                entry.push('\n');
            }
        }
        let hod = leg.starts_with("hod");
        if hod && !ran.success() {
            tally.wrong.push(format!("{leg}: exit {:?}\n{}", ran.status, ran.stderr_text()));
        }
        for (seed, text, lines) in &expected {
            let actual = got.get(seed).map_or("(not run)\n", String::as_str);
            if matches(lines, actual) {
                continue;
            }
            if hod {
                tally
                    .wrong
                    .push(format!("{leg}: t{seed}:\n{text}interpreter:\n{lines}hod:\n{actual}"));
            } else {
                let major = leg.trim_start_matches("llc").parse().unwrap_or(0);
                *tally.llc_wrong.entry(major).or_default() += 1;
            }
        }
    }
}

/// Panics with hod's first disagreements.
pub fn assert_agrees(tally: &Tally) {
    assert!(
        tally.wrong.is_empty(),
        "{} disagreements; the first:\n{}",
        tally.wrong.len(),
        tally.wrong.iter().take(4).cloned().collect::<Vec<_>>().join("\n")
    );
}

// ---- calls between hod's objects and llc's ----

/// One call of [`check_mixed`]: the caller `@t<seed>` (a `void()` that
/// emits what it gets back), the callee's definition and its
/// declaration.
pub struct Mixed {
    /// The caller's number.
    pub seed: u64,
    /// The caller (with its globals).
    pub caller: String,
    /// The callee's `declare` line.
    pub decl: String,
    /// The callee (with its globals).
    pub callee: String,
}

/// Checks calls across objects: the callers in one module, the callees
/// in another, each compiled by hod (plain, stressed) and `llc` 22/19,
/// linked in every pairing with hod on at least one side, against the
/// interpreter on both together. Nothing may be refused.
#[allow(clippy::similar_names, reason = "callers and callees")]
pub fn check_mixed(name: &str, cases: &[Mixed], tally: &mut Tally) {
    let none = BTreeMap::new();
    let decls: BTreeMap<String, String> =
        cases.iter().map(|c| (c.decl.clone(), c.decl.clone())).collect();
    let callers: Vec<(u64, String)> = cases.iter().map(|c| (c.seed, c.caller.clone())).collect();
    let callees: Vec<(u64, String)> = cases.iter().map(|c| (c.seed, c.callee.clone())).collect();
    let both: Vec<(u64, String)> = callers.iter().chain(&callees).cloned().collect();
    let caller_text = module_text(&callers, &decls);
    let callee_text = module_text(&callees, &none);
    let source = module_text(&both, &none);
    tally.generated += cases.len();
    let (module, bodies) = parse(&source);
    let program = Program::with_bodies(&module, &bodies)
        .unwrap_or_else(|stop| panic!("interpreter: {stop:?}"));
    let mut expected = Vec::new();
    for case in cases {
        match interpret(&module, &program, &format!("t{}", case.seed)) {
            Ok(lines) => {
                tally.poison_bytes += lines.matches("??").count();
                expected.push((case.seed, lines));
            }
            Err(Stop::Trap(trap)) => {
                tally.trapped += 1;
                *tally.traps.entry(format!("{trap:?}")).or_default() += 1;
            }
            // A signature the interpreter has no values for (`half`
            // lanes): `llc` 22 on both sides is the reference.
            Err(Stop::Unsupported(what)) => {
                *tally.by_llc.entry(what.clone()).or_default() += 1;
                expected.push((case.seed, String::new()));
            }
            Err(other) => panic!("t{}: {other:?}", case.seed),
        }
    }
    tally.run += expected.len();
    let scratch = Scratch::new(&format!("x64-vcall-{name}"));
    if let Ok(dir) = std::env::var("HOD_VECTOR_DUMP") {
        std::fs::write(format!("{dir}/x64-vcall-{name}.ll"), &source).expect("write");
    }
    // The objects of each side, by leg.
    let mut sides: [Vec<(String, PathBuf)>; 2] = [Vec::new(), Vec::new()];
    for (side, (stem, text)) in
        [("caller", &caller_text), ("callee", &callee_text)].iter().enumerate()
    {
        let (module, bodies) = parse(text);
        tally.insts += bodies.iter().map(hod_ir::Body::inst_count).sum::<usize>();
        for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
            let t = Instant::now();
            let bytes = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
                .unwrap_or_else(|e| panic!("{stem} {mode}: {e}"));
            if mode == "plain" {
                tally.compile += t.elapsed();
            }
            let object = scratch.write(&format!("{stem}-hod-{mode}.o"), bytes);
            sides[side].push((format!("hod-{mode}"), object));
        }
        for major in [22, 19] {
            match llc(&scratch, major, stem, text) {
                Some(Ok(object)) => sides[side].push((format!("llc{major}"), object)),
                Some(Err(())) => *tally.llc_crashed.entry(major).or_default() += 1,
                None => {}
            }
        }
        if let (Some(hod), Some(llc)) = (
            text_bytes(&sides[side][0].1),
            sides[side].iter().find(|(leg, _)| leg == "llc22").and_then(|(_, o)| text_bytes(o)),
        ) {
            tally.text.0 += hod;
            tally.text.1 += llc;
        }
    }
    let mut driver = String::from(DRIVER);
    for (seed, _) in &expected {
        let _ = writeln!(driver, "void t{seed}(void);");
    }
    driver.push_str("int main(void) {\n");
    for (seed, _) in &expected {
        let _ = writeln!(driver, "  puts(\"t{seed}\"); fflush(stdout); t{seed}();");
    }
    driver.push_str("  return 0;\n}\n");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let Some(linker) = toolchain.linkers.first() else { return };
    let c = toolchain.compile_c(&scratch, "driver", &driver, &["-fPIE"]);
    // `llc` 22 on both sides first: the reference where the interpreter
    // has none.
    let mut pairs: Vec<(&String, &PathBuf, &String, &PathBuf)> = Vec::new();
    for (caller_leg, caller) in &sides[0] {
        for (callee_leg, callee) in &sides[1] {
            let hod = caller_leg.starts_with("hod") || callee_leg.starts_with("hod");
            if hod {
                pairs.push((caller_leg, caller, callee_leg, callee));
            } else if caller_leg == "llc22" && callee_leg == "llc22" {
                pairs.insert(0, (caller_leg, caller, callee_leg, callee));
            }
        }
    }
    if !pairs.first().is_some_and(|p| p.0 == "llc22" && p.2 == "llc22") {
        // No `llc` 22: only what the interpreter ran is compared.
        expected.retain(|(_, lines)| !lines.is_empty());
    }
    for (caller_leg, caller, callee_leg, callee) in pairs {
        {
            let hod = caller_leg.starts_with("hod") || callee_leg.starts_with("hod");
            let leg = format!("{caller_leg}>{callee_leg}");
            let program = scratch.join(&leg);
            let lm = std::path::Path::new("-lm");
            let inputs = [c.as_path(), caller.as_path(), callee.as_path(), lm];
            toolchain.link_ok(linker, &["-pie"], &inputs, &program);
            let ran =
                run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(5));
            tally.legs.push(leg.clone());
            if hod && !ran.success() {
                tally.wrong.push(format!("{leg}: exit {:?}\n{}", ran.status, ran.stderr_text()));
            }
            let mut got: BTreeMap<u64, String> = BTreeMap::new();
            let mut current = None;
            for line in ran.stdout_text().lines() {
                if let Some(seed) = line.strip_prefix('t').and_then(|seed| seed.parse().ok()) {
                    current = Some(seed);
                    got.insert(seed, String::new());
                } else if let Some(seed) = current {
                    let entry = got.get_mut(&seed).expect("inserted");
                    entry.push_str(line);
                    entry.push('\n');
                }
            }
            if !hod {
                for (seed, lines) in &mut expected {
                    if lines.is_empty() {
                        *lines = got.get(seed).cloned().unwrap_or_default();
                    }
                }
            }
            for (seed, lines) in &expected {
                let actual = got.get(seed).map_or("(not run)\n", String::as_str);
                if matches(lines, actual) {
                    continue;
                }
                if hod {
                    let case = cases.iter().find(|c| c.seed == *seed).expect("a case");
                    tally.wrong.push(format!(
                        "{leg}: t{seed}:\n{}{}interpreter:\n{lines}got:\n{actual}",
                        case.caller, case.callee
                    ));
                } else {
                    *tally.llc_wrong.entry(22).or_default() += 1;
                }
            }
        }
    }
}
