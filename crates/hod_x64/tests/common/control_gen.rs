//! WP-124's copy of WP-037's graph generator (`hod_a64/tests/common/
//! control.rs`, whose generator part is target-neutral but which imports
//! `hod_a64`; WP-130 may move it to a shared place), with the check
//! rewritten for x86-64: random control-flow graphs whose only
//! instructions are `phi`s and terminators (`br`, `condbr`, `switch`,
//! `indirectbr`, `ret`), as `.ll` text, run in `hod_interp` and, compiled
//! by hod for x86-64 (and optionally by `llc` 19 and 22), linked with a C
//! driver by the cross toolchain and run in ONE emulated process per
//! object.
//!
//! Every generated function terminates: its blocks form a DAG in index
//! order except for loops of two fixed shapes whose exit depends on the
//! arguments only through the initial value of a state `phi` (see the
//! AArch64 file's documentation). What a function returns is one of its
//! values (mostly a `phi`), so the result names the path taken.

#![allow(
    dead_code,
    clippy::needless_range_loop,
    clippy::range_plus_one,
    clippy::struct_excessive_bools,
    reason = "graph code over block indexes reads clearer by index; the options are independent switches"
)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use hod_cg::Options;
use hod_interp::{Interp, Program, Val};
use hod_testutil::{Scratch, run_with};

use crate::linking::Toolchain;

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

    /// A number below `n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// `true` with probability `num / den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.next() % den < num
    }

    /// One of `items`.
    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// A first-class type of a generated function.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    /// `iN`.
    Int(u32),
    /// `ptr`.
    Ptr,
}

impl Ty {
    /// The `.ll` spelling.
    pub fn ll(self) -> String {
        match self {
            Ty::Int(bits) => format!("i{bits}"),
            Ty::Ptr => "ptr".to_owned(),
        }
    }

    /// The width in bits.
    pub fn bits(self) -> u32 {
        match self {
            Ty::Int(bits) => bits,
            Ty::Ptr => 64,
        }
    }

    /// The mask of the defined bits.
    pub fn mask(self) -> u64 {
        mask(self.bits())
    }
}

/// The low `bits` bits.
pub fn mask(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 }
}

/// The values at the edges of a `bits`-bit integer, and a random one.
pub fn edge_value(rng: &mut Rng, bits: u32) -> u64 {
    let m = mask(bits);
    let top = 1u64 << (bits - 1);
    let v = match rng.below(9) {
        0 => 0,
        1 => 1,
        2 => m,
        3 => m.wrapping_sub(1),
        4 => top,
        5 => top.wrapping_sub(1),
        6 => top.wrapping_add(1),
        7 => rng.below(40) as u64,
        _ => rng.next(),
    };
    v & m
}

/// An operand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// Argument `n`.
    Arg(usize),
    /// `phi` `k` of block `b`.
    Phi(usize, usize),
    /// An integer or pointer constant (a pointer: `null` or `inttoptr`).
    Const(Ty, u64),
    /// `undef` (or `poison`, by the flag) of the type.
    Undef(Ty, bool),
    /// `blockaddress` of block `b` of this function.
    Block(usize),
}

/// The successors of a block and what decides between them; operands are
/// chosen once the `phi`s exist.
#[derive(Clone, Debug)]
enum Shape {
    Ret,
    Br(usize),
    CondBr(usize, usize),
    Switch { ty: Ty, default: usize, cases: Vec<(u64, usize)> },
    IndirectBr { targets: Vec<usize> },
}

impl Shape {
    /// The successor edges in order, with multiplicity.
    fn edges(&self) -> Vec<usize> {
        match self {
            Shape::Ret => Vec::new(),
            Shape::Br(t) => vec![*t],
            Shape::CondBr(t, f) => vec![*t, *f],
            Shape::Switch { default, cases, .. } => {
                std::iter::once(*default).chain(cases.iter().map(|&(_, b)| b)).collect()
            }
            Shape::IndirectBr { targets } => targets.clone(),
        }
    }
}

/// A `phi`: its type, the incoming values fixed by a loop shape (by
/// predecessor), whether it holds block addresses, and its entries (one
/// per edge into the block, in the block's predecessor order).
#[derive(Clone, Debug)]
struct PhiDef {
    ty: Ty,
    fixed: Vec<(usize, Op)>,
    addr: bool,
    incoming: Vec<Op>,
}

impl PhiDef {
    fn new(ty: Ty) -> PhiDef {
        PhiDef { ty, fixed: Vec::new(), addr: false, incoming: Vec::new() }
    }
}

/// What a generated function may contain.
#[derive(Clone, Copy, Debug)]
pub struct GenOptions {
    /// `undef` and `poison` as incoming values (the interpreter's zero;
    /// not for the `llc` comparison, where they are anything).
    pub undef: bool,
    /// `indirectbr` on a `blockaddress` constant.
    pub indirect: bool,
    /// `indirectbr` on a `phi` of `blockaddress` constants (the constant
    /// is materialized by the glue: needs `common.rs` to route
    /// `Const::BlockAddr` to `block_address`).
    pub addr_phis: bool,
    /// The loop shapes.
    pub loops: bool,
    /// The most blocks of the DAG part.
    pub max_blocks: usize,
    /// The most cases of a `switch`.
    pub max_cases: usize,
}

impl Default for GenOptions {
    fn default() -> GenOptions {
        GenOptions {
            undef: true,
            indirect: true,
            addr_phis: false,
            loops: true,
            max_blocks: 14,
            max_cases: 24,
        }
    }
}

/// A function as `.ll` text, with its signature and the values worth
/// passing as its arguments.
#[derive(Clone, Debug)]
pub struct Func {
    /// The symbol name.
    pub name: String,
    /// The parameter types.
    pub params: Vec<Ty>,
    /// The return type.
    pub ret: Ty,
    /// The definition.
    pub text: String,
    /// Values the function compares with, by width.
    pub interesting: Vec<(u32, u64)>,
}

impl Func {
    /// `count` argument lists for the function: values it compares with,
    /// their neighbours, and edges.
    pub fn arg_sets(&self, rng: &mut Rng, count: usize) -> Vec<Vec<u64>> {
        (0..count)
            .map(|_| {
                self.params
                    .iter()
                    .map(|&ty| {
                        let bits = ty.bits();
                        let known: Vec<u64> = self
                            .interesting
                            .iter()
                            .filter(|&&(b, _)| b == bits)
                            .map(|&(_, v)| v)
                            .collect();
                        let v = if !known.is_empty() && rng.chance(3, 4) {
                            let v = rng.pick(&known);
                            match rng.below(6) {
                                0 => v.wrapping_add(1),
                                1 => v.wrapping_sub(1),
                                _ => v,
                            }
                        } else {
                            edge_value(rng, bits)
                        };
                        v & ty.mask()
                    })
                    .collect()
            })
            .collect()
    }
}

const WIDTHS: [u32; 12] = [1, 1, 1, 7, 8, 16, 17, 32, 33, 63, 64, 64];

/// A random function named `name`.
#[allow(clippy::too_many_lines, reason = "one generator, in its steps")]
pub fn function(rng: &mut Rng, name: &str, o: &GenOptions) -> Func {
    // The signature: an `i1` first, then integers of every width and
    // pointers.
    let mut params = vec![Ty::Int(1)];
    for _ in 0..1 + rng.below(5) {
        params.push(if rng.chance(1, 8) { Ty::Ptr } else { Ty::Int(rng.pick(&WIDTHS)) });
    }
    let ret = rng.pick(&[
        Ty::Int(64),
        Ty::Int(64),
        Ty::Int(32),
        Ty::Int(8),
        Ty::Int(1),
        Ty::Int(17),
        Ty::Ptr,
    ]);
    let mut types: Vec<Ty> = params.clone();
    types.push(ret);
    types.push(Ty::Int(1));
    types.push(Ty::Int(64));
    let mut interesting: Vec<(u32, u64)> = Vec::new();

    let n = 2 + rng.below(o.max_blocks.max(2) - 1);
    let mut shapes: Vec<Option<Shape>> = vec![None; n];
    let mut phis: Vec<Vec<PhiDef>> = vec![Vec::new(); n];
    // Blocks only their loop header may enter.
    let mut closed = vec![false; n];
    // Blocks that must be entered from the block before them.
    let mut wanted = vec![false; n];

    // The loops, placed first so that the DAG goes around them.
    if o.loops {
        // Where they go (a header and its case blocks, or a self loop),
        // then what they are, their exits avoiding every case block.
        let mut placed: Vec<(usize, usize)> = Vec::new();
        let mut at = 1;
        while at + 2 < n {
            if rng.chance(1, 3) {
                let room = n - at - 1;
                let k = if room >= 3 && rng.chance(1, 2) {
                    1 + rng.below((room - 2).min(3))
                } else {
                    0
                };
                placed.push((at, k));
                for c in at + 1..=at + k {
                    closed[c] = true;
                }
                wanted[at] = true;
                at += k + 2;
                continue;
            }
            at += 1;
        }
        for (at, k) in placed {
            let exits: Vec<usize> = (at + k + 1..n).filter(|&t| !closed[t]).collect();
            if k > 0 {
                header_loop(rng, at, k, &exits, &mut shapes, &mut phis, &mut interesting);
            } else {
                self_loop(rng, at, &exits, &mut shapes, &mut phis, &mut interesting);
            }
        }
    }

    // The DAG: every other block goes forward, never into a closed block.
    for b in 0..n {
        if shapes[b].is_some() {
            continue;
        }
        let open: Vec<usize> = (b + 1..n).filter(|&t| !closed[t]).collect();
        if open.is_empty() {
            shapes[b] = Some(Shape::Ret);
            continue;
        }
        let target = |rng: &mut Rng| {
            if b + 1 < n && wanted[b + 1] && rng.chance(2, 3) { b + 1 } else { rng.pick(&open) }
        };
        let shape = match rng.below(if o.indirect { 10 } else { 9 }) {
            0 => Shape::Ret,
            1 | 2 => Shape::Br(target(rng)),
            3..=5 => {
                let t = target(rng);
                let f = if rng.chance(1, 8) { t } else { target(rng) };
                Shape::CondBr(t, f)
            }
            6..=8 => {
                let ty = Ty::Int(rng.pick(&WIDTHS));
                let default = target(rng);
                let cases = switch_cases(rng, ty.bits(), o.max_cases, &open, default);
                for &(v, _) in &cases {
                    interesting.push((ty.bits(), v));
                }
                Shape::Switch { ty, default, cases }
            }
            _ => {
                let count = 1 + rng.below(4);
                let mut targets: Vec<usize> = Vec::new();
                for _ in 0..count {
                    let t = target(rng);
                    if !targets.contains(&t) {
                        targets.push(t);
                    }
                }
                Shape::IndirectBr { targets }
            }
        };
        shapes[b] = Some(shape);
    }
    let shapes: Vec<Shape> = shapes.into_iter().map(|s| s.expect("every block shaped")).collect();

    // Predecessor edges, reachability, dominators.
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (b, shape) in shapes.iter().enumerate() {
        for t in shape.edges() {
            preds[t].push(b);
        }
    }
    let mut reachable = vec![false; n];
    let mut stack = vec![0];
    while let Some(b) = stack.pop() {
        if !std::mem::replace(&mut reachable[b], true) {
            stack.extend(shapes[b].edges());
        }
    }
    let all = mask(n as u32);
    let mut dom: Vec<u64> =
        (0..n).map(|b| if b == 0 || !reachable[b] { 1 << b } else { all }).collect();
    loop {
        let mut changed = false;
        for b in 1..n {
            if !reachable[b] {
                continue;
            }
            let mut d = all;
            for &p in &preds[b] {
                if reachable[p] {
                    d &= dom[p];
                }
            }
            d |= 1 << b;
            if d != dom[b] {
                dom[b] = d;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // The other `phi`s, and the address `phi` of an `indirectbr` that
    // has predecessors.
    let mut via_phi = vec![false; n];
    for b in 1..n {
        if preds[b].is_empty() {
            phis[b].clear();
            continue;
        }
        for _ in 0..rng.below(4) {
            phis[b].push(PhiDef::new(rng.pick(&types)));
        }
        if o.addr_phis && matches!(shapes[b], Shape::IndirectBr { .. }) && rng.chance(2, 3) {
            let mut phi = PhiDef::new(Ty::Ptr);
            phi.addr = true;
            phis[b].push(phi);
            via_phi[b] = true;
        }
    }

    // Values available at the end of block `b` of type `ty`.
    let available = |phis: &[Vec<PhiDef>], b: usize, ty: Ty| -> Vec<Op> {
        let mut out: Vec<Op> = Vec::new();
        for d in 0..n {
            if dom[b] & (1 << d) != 0 {
                for (k, phi) in phis[d].iter().enumerate() {
                    if phi.ty == ty && !phi.addr {
                        out.push(Op::Phi(d, k));
                    }
                }
            }
        }
        out
    };
    let args_of = |ty: Ty| -> Vec<Op> {
        params.iter().enumerate().filter(|&(_, &t)| t == ty).map(|(i, _)| Op::Arg(i)).collect()
    };
    let constant = |rng: &mut Rng, ty: Ty| -> Op {
        match ty {
            Ty::Int(bits) => Op::Const(ty, edge_value(rng, bits)),
            Ty::Ptr => Op::Const(ty, if rng.chance(1, 2) { 0 } else { rng.next() & 0xffff_fff0 }),
        }
    };
    let choose = |rng: &mut Rng, phis: &[Vec<PhiDef>], b: usize, ty: Ty, undef: bool| -> Op {
        let from_phis = available(phis, b, ty);
        let from_args = args_of(ty);
        let roll = rng.below(20);
        if undef && roll == 0 {
            return Op::Undef(ty, rng.chance(1, 2));
        }
        if !from_phis.is_empty() && roll < 13 {
            return rng.pick(&from_phis);
        }
        if !from_args.is_empty() && roll < 18 {
            return rng.pick(&from_args);
        }
        constant(rng, ty)
    };

    // Incoming values: one per edge, equal for the edges of one
    // predecessor.
    for b in 1..n {
        for k in 0..phis[b].len() {
            let mut incoming: Vec<Op> = Vec::with_capacity(preds[b].len());
            for (e, &p) in preds[b].iter().enumerate() {
                let first = preds[b][..e].iter().position(|&q| q == p);
                let op = if let Some(first) = first {
                    incoming[first]
                } else if let Some(&(_, op)) = phis[b][k].fixed.iter().find(|&&(q, _)| q == p) {
                    op
                } else if phis[b][k].addr {
                    let Shape::IndirectBr { targets } = &shapes[b] else { unreachable!() };
                    Op::Block(rng.pick(targets))
                } else {
                    choose(rng, &phis, p, phis[b][k].ty, o.undef)
                };
                incoming.push(op);
            }
            phis[b][k].incoming = incoming;
        }
    }

    // The text, the entry first and the other blocks shuffled.
    let mut order: Vec<usize> = (1..n).collect();
    for i in (1..order.len()).rev() {
        order.swap(i, rng.below(i + 1));
    }
    order.insert(0, 0);
    let mut text = String::new();
    let plist: Vec<String> =
        params.iter().enumerate().map(|(i, t)| format!("{} %a{i}", t.ll())).collect();
    writeln!(text, "define {} @{name}({}) {{", ret.ll(), plist.join(", ")).unwrap();
    let op_text = |op: Op| -> String {
        match op {
            Op::Arg(i) => format!("%a{i}"),
            Op::Phi(b, k) => format!("%p{b}_{k}"),
            Op::Const(Ty::Int(1), v) => (if v & 1 != 0 { "true" } else { "false" }).to_owned(),
            Op::Const(Ty::Int(bits), v) => signed(bits, v).to_string(),
            Op::Const(Ty::Ptr, 0) => "null".to_owned(),
            Op::Const(Ty::Ptr, v) => format!("inttoptr (i64 {v} to ptr)"),
            Op::Undef(_, false) => "undef".to_owned(),
            Op::Undef(_, true) => "poison".to_owned(),
            Op::Block(t) => format!("blockaddress(@{name}, %b{t})"),
        }
    };
    for &b in &order {
        writeln!(text, "b{b}:").unwrap();
        for (k, phi) in phis[b].iter().enumerate() {
            let entries: Vec<String> = phi
                .incoming
                .iter()
                .zip(&preds[b])
                .map(|(&op, &p)| format!("[ {}, %b{p} ]", op_text(op)))
                .collect();
            writeln!(text, "  %p{b}_{k} = phi {} {}", phi.ty.ll(), entries.join(", ")).unwrap();
        }
        let line = match &shapes[b] {
            Shape::Ret => {
                let mut op = choose(rng, &phis, b, ret, false);
                // Mostly a `phi`, so that the result names the path.
                for _ in 0..3 {
                    if matches!(op, Op::Phi(..)) {
                        break;
                    }
                    op = choose(rng, &phis, b, ret, false);
                }
                format!("ret {} {}", ret.ll(), op_text(op))
            }
            Shape::Br(t) => format!("br label %b{t}"),
            Shape::CondBr(t, f) => {
                let op = if let Some(k) = loop_flag(&phis[b], b, *t, *f) {
                    Op::Phi(b, k)
                } else if rng.chance(1, 12) {
                    Op::Const(Ty::Int(1), rng.next() & 1)
                } else {
                    let mut op = choose(rng, &phis, b, Ty::Int(1), false);
                    if let Op::Const(..) = op {
                        op = Op::Arg(0);
                    }
                    op
                };
                format!("br i1 {}, label %b{t}, label %b{f}", op_text(op))
            }
            Shape::Switch { ty, default, cases } => {
                // A loop's switch reads its state `phi`, which is `phi` 0.
                let op = if phis[b].first().is_some_and(|p| !p.fixed.is_empty() && p.ty == *ty) {
                    Op::Phi(b, 0)
                } else if rng.chance(1, 16) {
                    let pool: Vec<u64> = cases.iter().map(|&(v, _)| v).collect();
                    let v = if pool.is_empty() || rng.chance(1, 3) {
                        edge_value(rng, ty.bits())
                    } else {
                        rng.pick(&pool)
                    };
                    Op::Const(*ty, v)
                } else {
                    let mut op = choose(rng, &phis, b, *ty, false);
                    if let Op::Const(..) = op {
                        op = args_of(*ty).first().copied().unwrap_or(op);
                    }
                    op
                };
                let list: Vec<String> = cases
                    .iter()
                    .map(|&(v, t)| {
                        format!("{} {}, label %b{t}", ty.ll(), op_text(Op::Const(*ty, v)))
                    })
                    .collect();
                format!(
                    "switch {} {}, label %b{default} [ {} ]",
                    ty.ll(),
                    op_text(op),
                    list.join(" ")
                )
            }
            Shape::IndirectBr { targets } => {
                let op = if via_phi[b] {
                    Op::Phi(b, phis[b].len() - 1)
                } else {
                    Op::Block(rng.pick(targets))
                };
                let labels: Vec<String> = targets.iter().map(|t| format!("label %b{t}")).collect();
                format!("indirectbr ptr {}, [ {} ]", op_text(op), labels.join(", "))
            }
        };
        writeln!(text, "  {line}").unwrap();
    }
    text.push_str("}\n");
    Func { name: name.to_owned(), params, ret, text, interesting }
}

/// The `i1` `phi` a self loop's `condbr` reads, if this is one.
fn loop_flag(phis: &[PhiDef], b: usize, t: usize, f: usize) -> Option<usize> {
    if t != b && f != b {
        return None;
    }
    phis.first().filter(|p| p.ty == Ty::Int(1) && !p.fixed.is_empty()).map(|_| 0)
}

/// `value` of `bits` bits as a signed decimal.
pub fn signed(bits: u32, value: u64) -> i64 {
    let shift = 64 - bits;
    ((value << shift) as i64) >> shift
}

/// Distinct case values of `bits` bits, some to `default`, several to
/// one block, dense or sparse.
fn switch_cases(
    rng: &mut Rng,
    bits: u32,
    max: usize,
    open: &[usize],
    default: usize,
) -> Vec<(u64, usize)> {
    let room = if bits >= 12 { max } else { max.min((1usize << bits) - 1) };
    let count = match rng.below(5) {
        0 => 0,
        1 => 1,
        2 => 2 + rng.below(3),
        _ => rng.below(room + 1),
    }
    .min(room);
    let dense = rng.chance(1, 3);
    let base = edge_value(rng, bits);
    let mut values: Vec<u64> = Vec::with_capacity(count);
    let mut tries = 0;
    while values.len() < count && tries < 8 * count + 8 {
        tries += 1;
        let v = if dense { base.wrapping_add(values.len() as u64) } else { edge_value(rng, bits) }
            & mask(bits);
        if !values.contains(&v) {
            values.push(v);
        }
    }
    let mut blocks: Vec<usize> = Vec::new();
    values
        .into_iter()
        .map(|v| {
            let b = match rng.below(6) {
                0 => default,
                1 | 2 if !blocks.is_empty() => rng.pick(&blocks),
                _ => rng.pick(open),
            };
            blocks.push(b);
            (v, b)
        })
        .collect()
}

/// A header loop at `h` with `k` case blocks after it.
fn header_loop(
    rng: &mut Rng,
    h: usize,
    k: usize,
    exits: &[usize],
    shapes: &mut [Option<Shape>],
    phis: &mut [Vec<PhiDef>],
    interesting: &mut Vec<(u32, u64)>,
) {
    let width = rng.pick(&[8, 32, 64, 3, 17]);
    let ty = Ty::Int(width);
    let default = rng.pick(exits);
    // Case values per case block, in rank order.
    let mut values: Vec<u64> = Vec::new();
    let mut cases: Vec<(u64, usize)> = Vec::new();
    for c in h + 1..=h + k {
        for _ in 0..1 + rng.below(2) {
            if let Some(v) = fresh(rng, width, &values) {
                values.push(v);
                cases.push((v, c));
            }
        }
    }
    // A case to the default block, and one straight out.
    for target in [default, rng.pick(exits)] {
        if rng.chance(1, 2)
            && let Some(v) = fresh(rng, width, &values)
        {
            values.push(v);
            cases.push((v, target));
        }
    }
    // A value no case takes: the way out (`fresh` keeps one free).
    let leave = (0..=mask(width)).find(|v| !values.contains(v)).expect("a free value");
    for &(v, _) in &cases {
        interesting.push((width, v));
    }
    let mut state = PhiDef::new(ty);
    for c in h + 1..=h + k {
        // The next state: a later case block's, or the way out.
        let later: Vec<u64> =
            cases.iter().filter(|&&(_, b)| b > c && b <= h + k).map(|&(v, _)| v).collect();
        let next = if !later.is_empty() && rng.chance(2, 3) { rng.pick(&later) } else { leave };
        state.fixed.push((c, Op::Const(ty, next)));
        let back = if rng.chance(1, 3) {
            let out = rng.pick(exits);
            if rng.chance(1, 2) { Shape::CondBr(h, out) } else { Shape::CondBr(out, h) }
        } else {
            Shape::Br(h)
        };
        shapes[c] = Some(back);
    }
    phis[h].push(state);
    // The rotation: each reads the next on every back edge.
    let rot_ty = rng.pick(&[Ty::Int(64), Ty::Int(32), Ty::Int(1), Ty::Int(17), Ty::Ptr]);
    let m = 2 + rng.below(2);
    for i in 0..m {
        let mut phi = PhiDef::new(rot_ty);
        for c in h + 1..=h + k {
            phi.fixed.push((c, Op::Phi(h, 1 + (i + 1) % m)));
        }
        phis[h].push(phi);
    }
    shapes[h] = Some(Shape::Switch { ty, default, cases });
}

/// A `width`-bit value not in `values`, if one stays free after it.
fn fresh(rng: &mut Rng, width: u32, values: &[u64]) -> Option<u64> {
    if width < 8 && values.len() as u64 + 2 > mask(width) + 1 {
        return None;
    }
    (0..64).map(|_| edge_value(rng, width)).find(|v| !values.contains(v))
}

/// A self loop at `s`.
fn self_loop(
    rng: &mut Rng,
    s: usize,
    exits: &[usize],
    shapes: &mut [Option<Shape>],
    phis: &mut [Vec<PhiDef>],
    interesting: &mut Vec<(u32, u64)>,
) {
    let out = rng.pick(exits);
    if rng.chance(1, 2) {
        let mut flag = PhiDef::new(Ty::Int(1));
        flag.fixed.push((s, Op::Const(Ty::Int(1), 0)));
        phis[s].push(flag);
        shapes[s] =
            Some(if rng.chance(1, 2) { Shape::CondBr(s, out) } else { Shape::CondBr(out, s) });
        // `condbr %flag, s, out` loops while the flag is true; the
        // inverted form loops while it is false: then the back edge
        // makes it false again, so it must loop at most once another
        // way. Keep the inverted form leaving at once on the back edge.
        if let Some(Shape::CondBr(t, _)) = &shapes[s]
            && *t != s
        {
            phis[s][0].fixed[0].1 = Op::Const(Ty::Int(1), 1);
        }
    } else {
        let width = rng.pick(&[1, 8, 33, 64]);
        let ty = Ty::Int(width);
        let v = edge_value(rng, width);
        let c = v ^ 1;
        let mut state = PhiDef::new(ty);
        state.fixed.push((s, Op::Const(ty, c)));
        phis[s].push(state);
        let mut cases = vec![(v, s)];
        if width > 1 && rng.chance(1, 2) {
            let w = edge_value(rng, width);
            if w != v {
                cases.push((w, rng.pick(exits)));
            }
        }
        interesting.push((width, v));
        shapes[s] = Some(Shape::Switch { ty, default: out, cases });
    }
    // A pair that swaps on the back edge.
    let ty = rng.pick(&[Ty::Int(64), Ty::Int(8), Ty::Int(1), Ty::Ptr]);
    let first = phis[s].len();
    for i in 0..2 {
        let mut phi = PhiDef::new(ty);
        phi.fixed.push((s, Op::Phi(s, first + 1 - i)));
        phis[s].push(phi);
    }
}

// ---- the check (x86-64) ----

/// The module header for x86-64 Linux.
pub fn ll_header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// What clang writes for `-fPIE`: `PIC Level` 2, `PIE Level` 2.
pub const PIE_FLAGS: &str = "\n!llvm.module.flags = !{!900, !901}\n\
    !900 = !{i32 8, !\"PIC Level\", i32 2}\n!901 = !{i32 7, !\"PIE Level\", i32 2}\n";

/// A module and its bodies from `.ll` text (the header is added).
pub fn parse(functions: &str) -> (hod_ir::Module, Vec<hod_ir::Body>) {
    let text = format!("{}{functions}{PIE_FLAGS}", ll_header());
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

/// `llc -O0 -fast-isel=0` of LLVM `major` for x86-64 PIC, of the `.ll`
/// functions `functions`, to an object; `None` when that `llc` is not
/// installed.
pub fn llc(dir: &Scratch, major: u32, name: &str, functions: &str) -> Option<PathBuf> {
    let llc = hod_testutil::find_tool(&format!("llc-{major}"))?;
    let input = dir.write(&format!("{name}.ll"), format!("{}{functions}", ll_header()));
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-fast-isel=0", "-mtriple=x86_64-unknown-linux-gnu"])
        .args(["-relocation-model=pic", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc-{major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

/// One call: the function's index and its arguments (defined bits only).
#[derive(Clone, Debug)]
pub struct Call {
    /// Index into the batch's functions.
    pub func: usize,
    /// The arguments.
    pub args: Vec<u64>,
}

/// How a batch is compiled.
#[derive(Clone, Copy, Debug)]
pub struct Mode {
    /// Its name in reports.
    pub name: &'static str,
    /// The options.
    pub options: Options,
}

/// The result of `call` in the interpreter, as the driver prints it.
fn interpret(
    program: &Program<'_>,
    module: &hod_ir::Module,
    funcs: &[Func],
    call: &Call,
) -> String {
    let func = &funcs[call.func];
    let sym = module.find_symbol(func.name.as_bytes()).expect("the function");
    let args: Vec<Val> = func
        .params
        .iter()
        .zip(&call.args)
        .map(|(&ty, &v)| match ty {
            Ty::Int(bits) => Val::int(bits, u128::from(v)),
            Ty::Ptr => Val::Ptr(v),
        })
        .collect();
    let outcome = Interp::new(program).run(sym, &args);
    match outcome.result {
        Ok(Some(Val::Int { value, .. })) => format!("{}", value as u64),
        Ok(Some(Val::Ptr(v))) => format!("{v}"),
        other => panic!("{}: {other:?}\n{}", func.name, func.text),
    }
}

/// The C driver: a table of calls through a pointer of six `uint64_t`
/// parameters (SysV passes them in `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`
/// whatever the callee declares), the arguments with random bits above
/// their width (§2.2: the callee may not rely on them; an `i1` gets them
/// from bit 1 on), each result masked to its width.
fn driver(funcs: &[Func], calls: &[Call], rng: &mut Rng) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         typedef uint64_t (*fn6)(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t);\n",
    );
    for f in funcs {
        writeln!(c, "uint64_t {}(void);", f.name).unwrap();
    }
    c.push_str("static const struct { fn6 f; uint64_t m; uint64_t a[6]; } calls[] = {\n");
    for call in calls {
        let f = &funcs[call.func];
        let mut args = [0u64; 6];
        for (i, (&ty, &v)) in f.params.iter().zip(&call.args).enumerate() {
            let junk = rng.next();
            args[i] = match ty {
                Ty::Int(bits) if bits < 64 => v | (junk << bits),
                _ => v,
            };
        }
        let list: Vec<String> = args.iter().map(|a| format!("{a}ULL")).collect();
        writeln!(c, "  {{ (fn6){}, {}ULL, {{ {} }} }},", f.name, f.ret.mask(), list.join(", "))
            .unwrap();
    }
    c.push_str(
        "};\nint main(void) {\n\
           for (unsigned i = 0; i < sizeof calls / sizeof calls[0]; i++) {\n\
             const uint64_t *a = calls[i].a;\n\
             printf(\"%llu\\n\", (unsigned long long)(calls[i].f(a[0], a[1], a[2], a[3], a[4], a[5]) & calls[i].m));\n\
           }\n  return 0;\n}\n",
    );
    c
}

/// A disagreement: the call, the function, what each side printed.
#[derive(Debug)]
pub struct Wrong {
    /// The mode (`plain`, `stress`, `llc19`, ...).
    pub mode: &'static str,
    /// The call's index.
    pub call: usize,
    /// The function's name.
    pub func: String,
    /// The interpreter's result.
    pub want: String,
    /// The native result.
    pub got: String,
}

/// Which `llc`s a check runs besides hod.
pub const LLC_MAJORS: [u32; 2] = [19, 22];

/// Runs every call of `calls` in the interpreter, and compiled for
/// x86-64 by hod in each of `modes` (and by `llc` 19 and 22 when
/// `with_llc`), each in one object, one driver and one emulated process.
/// Returns the disagreements; `None` without the tools.
pub fn check(
    label: &str,
    funcs: &[Func],
    calls: &[Call],
    modes: &[Mode],
    with_llc: bool,
    seed: u64,
) -> Option<Vec<Wrong>> {
    let body: String = funcs.iter().map(|f| f.text.as_str()).collect();
    let (module, bodies) = parse(&body);
    let program = Program::with_bodies(&module, &bodies).expect("bodies of the module");
    let expected: Vec<String> =
        calls.iter().map(|c| interpret(&program, &module, funcs, c)).collect();
    let scratch = Scratch::new(label);
    let toolchain = Toolchain::x86_64(&scratch)?;
    let linker = toolchain.linkers.first()?;
    let mut rng = Rng(seed ^ 0xd51e_57a6);
    let driver = toolchain.compile_c(&scratch, "driver", &driver(funcs, calls, &mut rng), &[]);
    let mut wrong = Vec::new();
    let mut objects: Vec<(&'static str, PathBuf)> = Vec::new();
    for mode in modes {
        let bytes = hod_x64::compile_to_object(&module, &bodies, &mode.options)
            .unwrap_or_else(|e| panic!("{}: {e}\n{body}", mode.name));
        objects.push((mode.name, scratch.write(&format!("{}.o", mode.name), bytes)));
    }
    if with_llc {
        for (major, name) in LLC_MAJORS.into_iter().zip(["llc19", "llc22"]) {
            if let Some(object) = llc(&scratch, major, name, &body) {
                objects.push((name, object));
            }
        }
    }
    for (name, object) in objects {
        let exe = scratch.join(name);
        toolchain.link_ok(linker, &[], &[&driver, &object], &exe);
        let ran = toolchain.run(linker, &exe);
        assert!(ran.success(), "{name}: {:?}\n{}", ran.status, ran.stderr_text());
        let out = ran.stdout_text();
        let got: Vec<&str> = out.lines().collect();
        assert_eq!(got.len(), expected.len(), "{name}: {out}");
        for (i, (want, got)) in expected.iter().zip(got).enumerate() {
            if want != got {
                wrong.push(Wrong {
                    mode: name,
                    call: i,
                    func: funcs[calls[i].func].name.clone(),
                    want: want.clone(),
                    got: got.to_owned(),
                });
            }
        }
    }
    Some(wrong)
}

/// The function named `name` among `funcs`, for a report.
pub fn text_of<'f>(funcs: &'f [Func], name: &str) -> &'f str {
    funcs.iter().find(|f| f.name == name).map_or("", |f| f.text.as_str())
}
