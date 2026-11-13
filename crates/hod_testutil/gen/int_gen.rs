//! The integer programs both code generators are checked with (WP-035,
//! WP-122; shared by WP-130): a systematic table of one-instruction
//! functions (every operation at every width, with register and constant
//! operands) and a generator of random straight-line integer functions of
//! their arguments and constants (with unconditional `br`s between
//! blocks). Both are `.ll` text without a module header, so the same
//! functions go to `hod_interp`, to the code generator and to `llc`.
//!
//! Target-neutral: each crate's `tests/common/int_gen.rs` includes this
//! file and adds what is its target's (the header, how an `i1` result is
//! compared, the driver, the runner).

#![allow(dead_code, reason = "each test uses part of it")]

use std::fmt::Write as _;

/// The widths every operation is checked at.
pub const WIDTHS: [u32; 17] = [1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 48, 63, 64];

/// The thirteen integer binary operations.
pub const BINARY: [&str; 13] = [
    "add", "sub", "mul", "udiv", "sdiv", "urem", "srem", "shl", "lshr", "ashr", "and", "or", "xor",
];

/// The ten predicates.
pub const PREDICATES: [&str; 10] =
    ["eq", "ne", "ugt", "uge", "ult", "ule", "sgt", "sge", "slt", "sle"];

/// A `Gpr`-class type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    /// `iN`.
    I(u32),
    /// `ptr`.
    Ptr,
}

impl Ty {
    /// The type in `.ll` text.
    pub fn ll(self) -> String {
        match self {
            Ty::I(bits) => format!("i{bits}"),
            Ty::Ptr => "ptr".to_string(),
        }
    }

    /// Its width.
    pub fn bits(self) -> u32 {
        match self {
            Ty::I(bits) => bits,
            Ty::Ptr => 64,
        }
    }
}

/// The low `bits` bits.
pub fn mask(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1 << bits) - 1 }
}

/// `value` (of `ty`) as an `.ll` literal: signed decimal, `true`/`false`
/// for `i1`, `null` for a pointer.
pub fn lit(ty: Ty, value: u64) -> String {
    match ty {
        Ty::I(1) => if value & 1 == 1 { "true" } else { "false" }.to_string(),
        Ty::I(bits) => {
            let v = value & mask(bits);
            if bits < 64 && v >> (bits - 1) == 1 {
                format!("-{}", (1u64 << bits) - v)
            } else if bits == 64 {
                format!("{}", v as i64)
            } else {
                format!("{v}")
            }
        }
        Ty::Ptr => "null".to_string(),
    }
}

/// A function of a suite.
#[derive(Clone, Debug)]
pub struct Func {
    /// Its name.
    pub name: String,
    /// Its parameters (at most three).
    pub params: Vec<Ty>,
    /// Its result.
    pub ret: Ty,
    /// Whether `llc`'s result must equal the interpreter's: not for a
    /// function whose result depends on what `freeze` makes of `poison`,
    /// which LLVM leaves arbitrary (hod and the interpreter make it 0).
    pub llc: bool,
    /// Where its text is in [`Suite::text`].
    pub text: std::ops::Range<usize>,
}

/// A function called with arguments: the values of the parameters, at
/// their width.
#[derive(Clone, Debug)]
pub struct Case {
    /// Index into [`Suite::funcs`].
    pub func: usize,
    /// The arguments, at their width.
    pub args: [u64; 3],
}

/// Functions as `.ll` text and the cases to run them with.
#[derive(Default, Debug)]
pub struct Suite {
    /// The functions' text (without the module header).
    pub text: String,
    /// The functions, in text order.
    pub funcs: Vec<Func>,
    /// The cases.
    pub cases: Vec<Case>,
}

impl Suite {
    /// Adds a function whose body is `body` (instructions, one per line)
    /// and returns its index.
    pub fn add(&mut self, name: String, params: &[Ty], ret: Ty, body: &str) -> usize {
        let list: Vec<String> =
            params.iter().enumerate().map(|(i, ty)| format!("{} %p{i}", ty.ll())).collect();
        let start = self.text.len();
        writeln!(self.text, "define {} @{name}({}) {{\n{body}}}\n", ret.ll(), list.join(", "))
            .unwrap();
        let text = start..self.text.len();
        self.funcs.push(Func { name, params: params.to_vec(), ret, llc: true, text });
        self.funcs.len() - 1
    }

    /// Adds a case of function `func`.
    pub fn case(&mut self, func: usize, args: [u64; 3]) {
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

    /// True with probability `percent` / 100.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// One element of `items`.
    pub fn pick<'t, T>(&mut self, items: &'t [T]) -> &'t T {
        &items[self.below(items.len() as u64) as usize]
    }
}

/// The values at the edges of `bits`: 0, 1, 2, -1, -2, the smallest and
/// largest signed values and their neighbours, the shift amounts around
/// the width, the immediate boundaries of both targets (AArch64's `add`/`cmp`:
/// 4095, 4096, 0xfff000, 2^24; x86's `imm8` and `imm32`) and their
/// negations, bitmask patterns; deduplicated, in a fixed order.
pub fn edges(bits: u32) -> Vec<u64> {
    let m = mask(bits);
    let min = 1u64 << (bits - 1);
    let mut out = Vec::new();
    for v in [
        0,
        1,
        2,
        m,
        m.wrapping_sub(1),
        min,
        min.wrapping_sub(1),
        min.wrapping_add(1),
        u64::from(bits) - 1,
        u64::from(bits),
        u64::from(bits) + 1,
        4095,
        4096,
        0x00ff_f000,
        0x0100_0000,
        4095u64.wrapping_neg(),
        4096u64.wrapping_neg(),
        0x00ff_00ff_00ff_00ff,
        0x5555_5555_5555_5555,
        // x86's immediates: `imm8` and `imm32`, sign-extended.
        127,
        128,
        128u64.wrapping_neg(),
        129u64.wrapping_neg(),
        0x7fff_ffff,
        0x8000_0000,
        0xffff_ffff,
        0x1_0000_0000,
        0x8000_0000u64.wrapping_neg(),
        0x8000_0001u64.wrapping_neg(),
    ] {
        let v = v & m;
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// A value of `ty`: an edge (60%) or random bits.
pub fn value(rng: &mut Rng, ty: Ty) -> u64 {
    let bits = ty.bits();
    if rng.chance(60) { *rng.pick(&edges(bits)) } else { rng.next() & mask(bits) }
}

// ---- the systematic table ----

/// Every binary operation at every width: with two register operands
/// (edge pairs and random ones), with each of a set of constants on the
/// right, and with a constant on the left.
pub fn binary_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    for &bits in &WIDTHS {
        let ty = Ty::I(bits);
        let t = ty.ll();
        let e = edges(bits);
        for op in BINARY {
            let f = s.add(
                format!("b_{op}_{bits}"),
                &[ty, ty],
                ty,
                &format!("  %r = {op} {t} %p0, %p1\n  ret {t} %r\n"),
            );
            for &a in &e {
                for &b in &e {
                    if rng.chance(25) {
                        s.case(f, [a, b, 0]);
                    }
                }
            }
            for _ in 0..12 {
                s.case(f, [value(rng, ty), value(rng, ty), 0]);
            }
            for (k, &c) in e.iter().enumerate() {
                let f = s.add(
                    format!("bc_{op}_{bits}_{k}"),
                    &[ty],
                    ty,
                    &format!("  %r = {op} {t} %p0, {}\n  ret {t} %r\n", lit(ty, c)),
                );
                for &a in &e {
                    if rng.chance(50) {
                        s.case(f, [a, 0, 0]);
                    }
                }
                s.case(f, [value(rng, ty), 0, 0]);
            }
            for (k, &c) in e.iter().enumerate().take(6) {
                let f = s.add(
                    format!("bl_{op}_{bits}_{k}"),
                    &[ty],
                    ty,
                    &format!("  %r = {op} {t} {}, %p0\n  ret {t} %r\n", lit(ty, c)),
                );
                for &a in &e {
                    if rng.chance(50) {
                        s.case(f, [a, 0, 0]);
                    }
                }
            }
        }
    }
    s
}

/// Every predicate at every width and on pointers: as a value, against
/// constants, and fused into `select`, `zext` and `sext`; `select` on a
/// plain `i1`; `freeze`.
pub fn compare_table(rng: &mut Rng) -> Suite {
    let mut s = Suite::default();
    let mut types: Vec<Ty> = WIDTHS.iter().map(|&b| Ty::I(b)).collect();
    types.push(Ty::Ptr);
    for &ty in &types {
        let bits = ty.bits();
        let t = ty.ll();
        let e = edges(bits);
        let pairs = |rng: &mut Rng, s: &mut Suite, f: usize| {
            for &a in &e {
                for &b in &e {
                    if rng.chance(20) {
                        s.case(f, [a, b, 0]);
                    }
                }
            }
            for &a in &e {
                s.case(f, [a, a, 0]);
            }
        };
        for p in PREDICATES {
            let f = s.add(
                format!("c_{p}_{bits}_{t}"),
                &[ty, ty],
                Ty::I(1),
                &format!("  %r = icmp {p} {t} %p0, %p1\n  ret i1 %r\n"),
            );
            pairs(rng, &mut s, f);
            let f = s.add(
                format!("cs_{p}_{bits}_{t}"),
                &[ty, ty],
                ty,
                &format!(
                    "  %c = icmp {p} {t} %p0, %p1\n  %r = select i1 %c, {t} %p0, {t} %p1\n  ret {t} %r\n"
                ),
            );
            pairs(rng, &mut s, f);
            for (ext, to) in [("zext", 32), ("sext", 64), ("zext", 7), ("sext", 33)] {
                let f = s.add(
                    format!("cx_{p}_{bits}_{t}_{ext}{to}"),
                    &[ty, ty],
                    Ty::I(to),
                    &format!(
                        "  %c = icmp {p} {t} %p0, %p1\n  %r = {ext} i1 %c to i{to}\n  ret i{to} %r\n"
                    ),
                );
                pairs(rng, &mut s, f);
            }
            if ty == Ty::Ptr {
                continue;
            }
            for (k, &c) in e.iter().enumerate() {
                let f = s.add(
                    format!("cc_{p}_{bits}_{k}"),
                    &[ty, ty],
                    ty,
                    &format!(
                        "  %c = icmp {p} {t} %p0, {}\n  %r = select i1 %c, {t} %p1, {t} 0\n  ret {t} %r\n",
                        lit(ty, c)
                    ),
                );
                for &a in &e {
                    s.case(f, [a, 1, 0]);
                }
                let f = s.add(
                    format!("cl_{p}_{bits}_{k}"),
                    &[ty],
                    Ty::I(1),
                    &format!("  %r = icmp {p} {t} {}, %p0\n  ret i1 %r\n", lit(ty, c)),
                );
                for &a in &e {
                    s.case(f, [a, 0, 0]);
                }
            }
        }
        let f = s.add(
            format!("s_{bits}_{t}"),
            &[Ty::I(1), ty, ty],
            ty,
            &format!("  %r = select i1 %p0, {t} %p1, {t} %p2\n  ret {t} %r\n"),
        );
        for _ in 0..8 {
            let (a, b) = (value(rng, ty), value(rng, ty));
            s.case(f, [0, a, b]);
            s.case(f, [1, a, b]);
        }
        let other = if ty == Ty::Ptr { "null".to_string() } else { lit(ty, 5) };
        let f = s.add(
            format!("sk_{bits}_{t}"),
            &[Ty::I(1), ty],
            ty,
            &format!(
                "  %a = select i1 %p0, {t} %p1, {t} {other}\n  %b = select i1 %p0, {t} {other}, {t} %p1\n  %c = select i1 %p0, {t} %a, {t} %b\n  ret {t} %c\n"
            ),
        );
        for _ in 0..4 {
            let a = value(rng, ty);
            s.case(f, [0, a, 0]);
            s.case(f, [1, a, 0]);
        }
        let f = s.add(
            format!("z_{bits}_{t}"),
            &[Ty::I(1)],
            ty,
            &format!("  %r = select i1 %p0, {t} {other}, {t} {}\n  ret {t} %r\n", lit(ty, 0)),
        );
        s.case(f, [0, 0, 0]);
        s.case(f, [1, 0, 0]);
        let f = s.add(
            format!("f_{bits}_{t}"),
            &[ty],
            ty,
            &format!("  %r = freeze {t} %p0\n  ret {t} %r\n"),
        );
        s.case(f, [value(rng, ty), 0, 0]);
        if ty != Ty::Ptr {
            let f = s.add(
                format!("fu_{bits}"),
                &[ty],
                ty,
                &format!("  %u = freeze {t} poison\n  %r = add {t} %u, %p0\n  ret {t} %r\n"),
            );
            s.funcs[f].llc = false;
            s.case(f, [value(rng, ty), 0, 0]);
        }
    }
    s
}

/// Every cast between the widths (`trunc`, `zext`, `sext`; a `bitcast`
/// of a type to itself), and between integers and pointers
/// (`ptrtoint`, `inttoptr`; `ptrtoaddr` when `llc` knows it).
pub fn cast_table(rng: &mut Rng, ptrtoaddr: bool) -> Suite {
    let mut s = Suite::default();
    let mut one = |s: &mut Suite, name: String, from: Ty, to: Ty, op: &str| {
        let f = s.add(
            name,
            &[from],
            to,
            &format!("  %r = {op} {} %p0 to {}\n  ret {} %r\n", from.ll(), to.ll(), to.ll()),
        );
        for &a in &edges(from.bits()) {
            s.case(f, [a, 0, 0]);
        }
        for _ in 0..4 {
            s.case(f, [value(rng, from), 0, 0]);
        }
    };
    for &a in &WIDTHS {
        for &b in &WIDTHS {
            let (from, to) = (Ty::I(a), Ty::I(b));
            match a.cmp(&b) {
                std::cmp::Ordering::Greater => {
                    one(&mut s, format!("t_{a}_{b}"), from, to, "trunc");
                }
                std::cmp::Ordering::Less => {
                    one(&mut s, format!("zx_{a}_{b}"), from, to, "zext");
                    one(&mut s, format!("sx_{a}_{b}"), from, to, "sext");
                }
                std::cmp::Ordering::Equal => {
                    one(&mut s, format!("bc_{a}"), from, to, "bitcast");
                }
            }
        }
        one(&mut s, format!("pi_{a}"), Ty::Ptr, Ty::I(a), "ptrtoint");
        one(&mut s, format!("ip_{a}"), Ty::I(a), Ty::Ptr, "inttoptr");
        if ptrtoaddr && a == 64 {
            one(&mut s, format!("pa_{a}"), Ty::Ptr, Ty::I(a), "ptrtoaddr");
        }
    }
    one(&mut s, "pp".to_string(), Ty::Ptr, Ty::Ptr, "bitcast");
    s
}
// ---- random straight-line functions ----

/// A function being generated.
struct Gen<'r> {
    rng: &'r mut Rng,
    body: String,
    /// The values defined so far, in order, and their types.
    pool: Vec<(String, Ty)>,
    next: u32,
    blocks: u32,
    /// The function's two widths.
    widths: [u32; 2],
    insts: usize,
}

impl Gen<'_> {
    fn fresh(&mut self) -> String {
        self.next += 1;
        format!("%v{}", self.next)
    }

    /// Emits `%v = <text>` of `ty` and adds it to the pool.
    fn def(&mut self, ty: Ty, text: &str) -> String {
        let name = self.fresh();
        writeln!(self.body, "  {name} = {text}").unwrap();
        self.pool.push((name.clone(), ty));
        self.insts += 1;
        name
    }

    /// A value of `ty` from the pool, preferring recent ones.
    fn existing(&mut self, ty: Ty) -> Option<String> {
        let all: Vec<&String> =
            self.pool.iter().filter(|(_, t)| *t == ty).map(|(name, _)| name).collect();
        if all.is_empty() {
            return None;
        }
        let from = if self.rng.chance(70) { all.len().saturating_sub(4) } else { 0 };
        let span = (all.len() - from) as u64;
        Some(all[from + self.rng.below(span) as usize].clone())
    }

    /// A value of `ty`: from the pool, or cast from an integer of
    /// another width, or a constant.
    fn get(&mut self, ty: Ty) -> String {
        if let Some(name) = self.existing(ty) {
            return name;
        }
        let ints: Vec<(String, Ty)> =
            self.pool.iter().filter(|(_, t)| matches!(t, Ty::I(_))).cloned().collect();
        if let Some((name, from)) = ints.last().cloned() {
            return self.convert(&name, from, ty);
        }
        lit(ty, value(self.rng, ty))
    }

    /// `name` of type `from` as a value of `to`.
    fn convert(&mut self, name: &str, from: Ty, to: Ty) -> String {
        if from == to {
            return name.to_string();
        }
        let (f, t) = (from.ll(), to.ll());
        let text = match (from, to) {
            (Ty::Ptr, _) => format!("ptrtoint ptr {name} to {t}"),
            (_, Ty::Ptr) => format!("inttoptr {f} {name} to ptr"),
            (Ty::I(a), Ty::I(b)) if a > b => format!("trunc {f} {name} to {t}"),
            _ => {
                let op = if self.rng.chance(50) { "zext" } else { "sext" };
                format!("{op} {f} {name} to {t}")
            }
        };
        self.def(to, &text)
    }

    /// An integer type of the function: one of its widths, or `i1`.
    fn int_ty(&mut self) -> Ty {
        match self.rng.below(10) {
            0..=5 => Ty::I(self.widths[0]),
            6..=8 => Ty::I(self.widths[1]),
            _ => Ty::I(1),
        }
    }

    /// A value of `ty` or (with `percent` chance) a constant.
    fn operand(&mut self, ty: Ty, percent: u64) -> String {
        if self.rng.chance(percent) { lit(ty, value(self.rng, ty)) } else { self.get(ty) }
    }

    fn binary(&mut self) {
        let ty = self.int_ty();
        let bits = ty.bits();
        let t = ty.ll();
        let mut op = *self.rng.pick(&BINARY);
        if bits == 1 && matches!(op, "sdiv" | "srem") {
            op = "udiv";
        }
        let lhs = self.operand(ty, 8);
        let rhs = match op {
            "udiv" | "urem" | "sdiv" | "srem" => {
                if self.rng.chance(25) {
                    let mut c = value(self.rng, ty) | 1;
                    if op.starts_with('s') && bits > 1 {
                        c &= mask(bits - 1);
                        c |= 1;
                    }
                    lit(ty, c)
                } else {
                    let mut x = self.get(ty);
                    if op.starts_with('s') && self.rng.chance(60) {
                        x = self.def(ty, &format!("and {t} {x}, {}", lit(ty, mask(bits - 1))));
                    }
                    self.def(ty, &format!("or {t} {x}, 1"))
                }
            }
            "shl" | "lshr" | "ashr" => match self.rng.below(10) {
                0..=3 => {
                    let b = u64::from(bits);
                    let c = *self.rng.pick(&[0, 1, b - 1, b / 2, self.rng.clone().below(b)]);
                    lit(ty, c)
                }
                4..=8 => {
                    let k = bits.ilog2();
                    let x = self.get(ty);
                    self.def(ty, &format!("and {t} {x}, {}", lit(ty, mask(k))))
                }
                _ => self.get(ty),
            },
            _ => self.operand(ty, 45),
        };
        self.def(ty, &format!("{op} {t} {lhs}, {rhs}"));
    }

    fn compare(&mut self) -> String {
        let ptrs = self.pool.iter().any(|(_, t)| *t == Ty::Ptr);
        let ty = if ptrs && self.rng.chance(20) { Ty::Ptr } else { self.int_ty() };
        let p = *self.rng.pick(&PREDICATES);
        let lhs = self.operand(ty, 5);
        let rhs = self.operand(ty, 40);
        self.def(Ty::I(1), &format!("icmp {p} {} {lhs}, {rhs}", ty.ll()))
    }

    fn select(&mut self, cond: Option<String>) {
        let cond = cond.unwrap_or_else(|| self.get(Ty::I(1)));
        let ptrs = self.pool.iter().any(|(_, t)| *t == Ty::Ptr);
        let ty = if ptrs && self.rng.chance(20) { Ty::Ptr } else { self.int_ty() };
        let a = self.operand(ty, 20);
        let b = self.operand(ty, 20);
        let t = ty.ll();
        self.def(ty, &format!("select i1 {cond}, {t} {a}, {t} {b}"));
    }

    fn cast(&mut self) {
        let ptrs: Vec<String> =
            self.pool.iter().filter(|(_, t)| *t == Ty::Ptr).map(|(n, _)| n.clone()).collect();
        let to = match self.rng.below(10) {
            0 => Ty::Ptr,
            1 => Ty::I(*self.rng.pick(&[8, 32, 64])),
            _ => self.int_ty(),
        };
        if !ptrs.is_empty() && to != Ty::Ptr && self.rng.chance(15) {
            let p = self.rng.pick(&ptrs).clone();
            self.convert(&p, Ty::Ptr, to);
            return;
        }
        let from = self.int_ty();
        let x = self.get(from);
        if from == to {
            let t = to.ll();
            self.def(to, &format!("bitcast {t} {x} to {t}"));
        } else {
            self.convert(&x, from, to);
        }
    }

    fn step(&mut self) {
        match self.rng.below(100) {
            0..=39 => self.binary(),
            40..=51 => {
                let c = self.compare();
                match self.rng.below(4) {
                    0 => self.select(Some(c)),
                    1 => {
                        let to = self.int_ty();
                        let op = if self.rng.chance(50) { "zext" } else { "sext" };
                        if to.bits() > 1 {
                            self.def(to, &format!("{op} i1 {c} to {}", to.ll()));
                        }
                    }
                    _ => {}
                }
            }
            52..=63 => self.select(None),
            64..=85 => self.cast(),
            86..=91 => {
                let ty = if self.rng.chance(80) { self.int_ty() } else { Ty::Ptr };
                let x = self.operand(ty, 10);
                self.def(ty, &format!("freeze {} {x}", ty.ll()));
            }
            _ => {
                self.blocks += 1;
                let b = self.blocks;
                writeln!(self.body, "  br label %b{b}\nb{b}:").unwrap();
                self.insts += 1;
            }
        }
    }
}

/// `count` random functions `r<n>`, each with four arguments-dependent
/// cases (edge and random values).
pub fn random_functions(rng: &mut Rng, count: usize) -> Suite {
    let mut s = Suite::default();
    for n in 0..count {
        let widths = [*rng.pick(&WIDTHS), *rng.pick(&WIDTHS)];
        let mut params = Vec::new();
        for _ in 0..=rng.below(3) {
            params.push(match rng.below(10) {
                0 => Ty::Ptr,
                1 | 2 => Ty::I(widths[1]),
                3 => Ty::I(1),
                _ => Ty::I(widths[0]),
            });
        }
        let pool = params.iter().enumerate().map(|(i, &t)| (format!("%p{i}"), t)).collect();
        let mut g = Gen { rng, body: String::new(), pool, next: 0, blocks: 0, widths, insts: 0 };
        let len = 3 + g.rng.below(18);
        while (g.insts as u64) < len {
            g.step();
        }
        // The result: the last few values, at the first width, xor'ed.
        let ret = Ty::I(widths[0]);
        let t = ret.ll();
        let tail: Vec<(String, Ty)> = g.pool.iter().rev().take(4).cloned().collect();
        let mut acc: Option<String> = None;
        for (name, ty) in tail {
            let v = g.convert(&name, ty, ret);
            acc = Some(match acc {
                None => v,
                Some(a) => g.def(ret, &format!("xor {t} {a}, {v}")),
            });
        }
        let acc = acc.unwrap_or_else(|| lit(ret, 0));
        writeln!(g.body, "  ret {t} {acc}").unwrap();
        let body = std::mem::take(&mut g.body);
        let f = s.add(format!("r{n}"), &params, ret, &body);
        for _ in 0..4 {
            let mut args = [0; 3];
            for (i, &ty) in params.iter().enumerate() {
                args[i] = value(rng, ty);
            }
            s.case(f, args);
        }
    }
    s
}
