//! `llc`'s `LowerConstantIntrinsics` for a function that calls
//! `llvm.is.constant` or `llvm.objectsize` (`spec/codegen.md` §5.6.8
//! "Folded constants and the blocks they remove", WP-261), and the
//! instructions `SelectionDAG` deletes afterwards (§5.5.1 "WP-084").
//!
//! `llc` runs the IR pass at every optimization level before instruction
//! selection. hod repeats what it decides, in its order:
//!
//! 1. Each call, in reverse post-order of the blocks reachable from the
//!    entry, becomes its constant; the users of a replaced value are
//!    simplified recursively, as LLVM's `InstSimplify` simplifies them
//!    ([`Lci::simplify`]: the subset hod models).
//! 2. A conditional `br` whose condition became `true` or `false` becomes
//!    an unconditional one (only `br`; a `switch` stays). The block it no
//!    longer goes to loses its `phi` entries for the edge.
//! 3. If such a block lost its last predecessor, LLVM's
//!    `removeUnreachableBlocks` walks the function from the entry
//!    (`markAliveBlocks`): it ends a block at a `noreturn` call, at a call
//!    through `undef`, and at a store to `undef`; folds a terminator on a
//!    constant (`br`, `switch`); and deletes every block it did not reach.
//!    Otherwise `llc`'s later pass deletes the blocks no longer reachable.
//!    The walk's other rewrites (a store to or a call/`invoke` through
//!    `null`, an `invoke` through `undef`, an `invoke` of a `nounwind` or
//!    `noreturn` callee, an `indirectbr` on a `blockaddress`) are not
//!    modelled: hod keeps the instruction as written with all its edges,
//!    so it emits a superset of `llc`'s blocks (WP-269). The folding
//!    never refuses a function.
//!
//! The result is per instruction an [`Action`], and the set of blocks
//! emitted. A function without those calls costs a look at its call
//! sites, and a function in which nothing folds a branch is emitted as
//! before (only the dead operands of the folded calls are left out).

use hod_ir::{
    BlockId, Body, CallSite, Callee, Const, FnAttrs, Inst, InstId, InstKind, IntPredicate,
    Intrinsic, Module, Opcode, SymbolKind, TailKind, TypeId, TypeKind, Value, ValueKind,
};

use crate::error::CgResult;

/// What the target does with an instruction of the function.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Action {
    /// Lowered as usual.
    #[default]
    Emit,
    /// Not emitted: `llc` deletes it.
    Drop,
    /// A terminator lowered as `br label %target` (its other edges are
    /// gone, with their `phi` stores).
    Jump(BlockId),
    /// A terminator lowered as `unreachable` (the block ends before it).
    Unreachable,
}

/// What hod knows of an instruction's replacement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lat {
    /// Not replaced.
    Kept,
    /// Replaced by this integer (bits above the width clear).
    Int(u64),
    /// Replaced by a constant hod does not model (`poison`, `undef`, an
    /// expression): nothing folds through it.
    Top,
    /// Replaced by another value.
    Alias(Value),
}

/// An operand as `InstSimplify` sees it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Opd {
    Int(u64),
    Top,
    /// A value that is not a constant integer (an instruction not
    /// replaced, an argument, an address), compared by identity.
    Val(Value),
}

/// Whether the call is one `LowerConstantIntrinsics` replaces.
fn folds(site: &CallSite<'_>) -> bool {
    matches!(site.intrinsic(), Some(Intrinsic::IsConstant | Intrinsic::ObjectSize))
}

/// Whether `inst` has no effect but its result (the driver's
/// `has_no_effect`): what `SelectionDAG` deletes when nothing uses it.
fn removable(inst: Inst<'_>) -> bool {
    let op = inst.opcode();
    if op.is_binary() || op.is_cast() {
        return true;
    }
    match op {
        Opcode::FNeg
        | Opcode::ICmp
        | Opcode::FCmp
        | Opcode::PtrAdd
        | Opcode::Select
        | Opcode::Freeze
        | Opcode::ExtractValue
        | Opcode::InsertValue
        | Opcode::ExtractElement
        | Opcode::InsertElement
        | Opcode::ShuffleVector => true,
        Opcode::Load => matches!(
            inst.kind(),
            InstKind::Load { mem, .. } if !mem.is_volatile() && mem.ordering().is_none()
        ),
        _ => false,
    }
}

/// Whether `llvm.is.constant` of `value` is true, as `llc`'s
/// `isManifestConstant` decides it: a constant that is data (a number,
/// `null`, a zero, `undef`, `poison`, `none`, a string of bytes), or an
/// aggregate, splat or constant expression all of whose operands are.
/// The address of a symbol or block, and so anything built on one, is
/// not.
pub fn is_manifest(module: &Module, value: Value) -> bool {
    let Some(id) = value.as_const() else { return false };
    let mut pending = Vec::new();
    let mut next = Some(id);
    while let Some(id) = next {
        match module.consts.get(id) {
            Const::Int { .. }
            | Const::WideInt { .. }
            | Const::Float { .. }
            | Const::WideFloat { .. }
            | Const::Null(_)
            | Const::Zero(_)
            | Const::Undef(_)
            | Const::Poison(_)
            | Const::NoneToken
            | Const::Bytes { .. }
            // `inttoptr` of an integer: an expression of data.
            | Const::AddrOfInt { .. } => {}
            Const::Aggregate { elems: args, .. } | Const::Expr { args, .. } => {
                pending.extend_from_slice(args);
            }
            Const::Splat { elem, .. } => pending.push(elem),
            _ => return false,
        }
        next = pending.pop();
    }
    true
}

/// The value hod gives `llvm.objectsize` (`min` true: `0`, else all
/// ones): the evaluator `llc` runs is not modelled (a known gap, §5.6.8).
pub fn object_size(module: &Module, min: Value) -> u64 {
    let min = min
        .as_const()
        .and_then(|id| match module.consts.get(id) {
            Const::Int { value, .. } => Some(value),
            _ => None,
        })
        .unwrap_or(0);
    if min & 1 == 1 { 0 } else { u64::MAX }
}

/// The width of an integer type of at most 64 bits.
fn int_bits(module: &Module, ty: TypeId) -> Option<u32> {
    match module.types.kind(ty) {
        TypeKind::Int(bits) if (1..=64).contains(&bits) => Some(bits),
        _ => None,
    }
}

fn mask(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 }
}

/// `value` of `bits` bits, sign-extended.
fn sext(value: u64, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((value << shift) as i64) >> shift
}

/// Whether `value` fits `bits` bits signed.
fn fits_signed(value: i128, bits: u32) -> bool {
    let half = 1i128 << (bits - 1);
    (-half..half).contains(&value)
}

/// What a callee or pointer operand is, for `markAliveBlocks`.
#[derive(PartialEq, Eq)]
enum Ptr {
    Null,
    Undef,
    Other,
}

fn ptr_kind(module: &Module, value: Value) -> Ptr {
    match value.as_const().map(|id| module.consts.get(id)) {
        Some(Const::Null(_)) => Ptr::Null,
        Some(Const::Undef(_) | Const::Poison(_)) => Ptr::Undef,
        _ => Ptr::Other,
    }
}

/// The callee's function attributes, for a direct call.
fn callee_has(module: &Module, site: &CallSite<'_>, flag: u32) -> bool {
    let Callee::Value(value) = site.callee() else { return false };
    let ValueKind::Sym(sym) = value.kind() else { return false };
    match module.symbol(sym).kind() {
        SymbolKind::Function { attrs, .. } => module.fn_attrs(attrs).has(flag),
        _ => false,
    }
}

/// `CallBase::doesNotReturn`: on the call, on the called function, or
/// one of the intrinsics LLVM declares `noreturn`.
fn no_return(module: &Module, site: &CallSite<'_>) -> bool {
    site.is_noreturn()
        || callee_has(module, site, FnAttrs::NORETURN)
        || matches!(
            site.intrinsic(),
            Some(Intrinsic::Trap | Intrinsic::UbsanTrap | Intrinsic::EhSjljLongjmp)
        )
}

/// The analysis of one function, and its buffers (part of [`crate::Scratch`]).
#[derive(Default, Debug)]
pub struct Lci {
    /// Whether a branch folded: [`Lci::action`] and [`Lci::reach`] apply.
    active: bool,
    /// Whether the driver's dead-chain rule drops an instruction.
    drops: bool,
    lat: Vec<Lat>,
    action: Vec<Action>,
    /// Per block: emitted. Meaningful when `active`.
    reach: Vec<bool>,
    block_of: Vec<u32>,
    user_start: Vec<u32>,
    users: Vec<u32>,
    /// `(x, y)`: `y` was replaced by `x`, whose users now include `y`'s.
    alias_from: Vec<(u32, u32)>,
    /// `(block, pred)`: a `phi` entry of `block` for `pred` was removed.
    dead_in: Vec<(u32, u32)>,
    preds: Vec<u32>,
    stamp: Vec<u32>,
    round: u32,
    list: Vec<u32>,
    unsimplified: Vec<u32>,
    calls: Vec<u32>,
    stack: Vec<(u32, u32)>,
    order: Vec<u32>,
    uses: Vec<u32>,
    bits: Vec<u64>,
}

/// The high bit of a use count: a use outside the block, or by a `phi`.
const EXPORTED: u32 = 1 << 31;

/// The result of [`analyse`], for tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folding {
    /// Per instruction, what the target does with it.
    pub actions: Vec<Action>,
    /// The blocks not emitted that a plain walk from the entry reaches;
    /// empty when no branch folds.
    pub removed: Vec<BlockId>,
}

/// The decisions for `body`, as [`compile_function`](crate::compile_function)
/// takes them.
///
/// # Errors
/// None: the folding never refuses a function (WP-269); the `Result`
/// stays for the callers.
pub fn analyse(module: &Module, body: &Body) -> CgResult<Folding> {
    let mut lci = Lci::default();
    lci.plan(module, body);
    let actions = body.insts().map(|inst| lci.action(inst.id())).collect();
    let mut plain = vec![false; body.block_count()];
    let mut work = vec![body.entry()];
    plain[body.entry().index()] = true;
    while let Some(b) = work.pop() {
        for s in body.block(b).successors() {
            if !plain[s.index()] {
                plain[s.index()] = true;
                work.push(s);
            }
        }
    }
    let removed = (0..body.block_count())
        .filter(|&b| plain[b] && !lci.reached(BlockId::new(b as u32)))
        .map(|b| BlockId::new(b as u32))
        .collect();
    Ok(Folding { actions, removed })
}

impl Lci {
    /// The heap bytes held.
    pub(crate) fn allocated_bytes(&self) -> usize {
        fn bytes<T>(v: &Vec<T>) -> usize {
            v.capacity() * size_of::<T>()
        }
        bytes(&self.lat)
            + bytes(&self.action)
            + bytes(&self.reach)
            + bytes(&self.block_of)
            + bytes(&self.user_start)
            + bytes(&self.users)
            + bytes(&self.alias_from)
            + bytes(&self.dead_in)
            + bytes(&self.preds)
            + bytes(&self.stamp)
            + bytes(&self.list)
            + bytes(&self.unsimplified)
            + bytes(&self.calls)
            + bytes(&self.stack)
            + bytes(&self.order)
            + bytes(&self.uses)
            + bytes(&self.bits)
    }

    /// Whether a branch folded, so that blocks may be left out.
    pub(crate) fn active(&self) -> bool {
        self.active
    }

    /// Whether `block` is emitted; true for all when not [`Lci::active`].
    pub(crate) fn reached(&self, block: BlockId) -> bool {
        !self.active || self.reach.get(block.index()).copied().unwrap_or(false)
    }

    /// What the target does with `inst` of the function last planned.
    pub fn action(&self, inst: InstId) -> Action {
        let i = inst.index();
        if self.drops {
            let words = self.bits.len() / 2;
            if self.bits.get(words + i / 64).is_some_and(|w| w >> (i % 64) & 1 != 0) {
                return Action::Drop;
            }
        }
        if self.active { self.action.get(i).copied().unwrap_or_default() } else { Action::Emit }
    }

    /// Decides everything for `body`; never refuses (WP-269).
    pub(crate) fn plan(&mut self, module: &Module, body: &Body) {
        self.active = false;
        self.drops = false;
        if !body.calls().any(|site| folds(&site)) {
            return;
        }
        let n = body.inst_count();
        self.lat.clear();
        self.lat.resize(n, Lat::Kept);
        self.action.clear();
        self.action.resize(n, Action::Emit);
        self.alias_from.clear();
        self.dead_in.clear();
        self.index(body);
        let has_dead = self.fold(module, body);
        self.active = self.action.iter().any(|&a| a != Action::Emit);
        if self.active {
            if has_dead {
                self.mark_alive(module, body);
            } else {
                self.walk(body);
            }
        }
        self.drops = self.decide(body);
    }

    /// Users, the block of each instruction, predecessor counts.
    fn index(&mut self, body: &Body) {
        let n = body.inst_count();
        self.block_of.clear();
        self.block_of.resize(n, 0);
        self.preds.clear();
        self.preds.resize(body.block_count(), 0);
        self.user_start.clear();
        self.user_start.resize(n + 1, 0);
        for block in body.blocks() {
            for inst in block.insts() {
                self.block_of[inst.id().index()] = block.id().raw();
                for &op in inst.operands() {
                    if let ValueKind::Inst(x) = op.kind() {
                        self.user_start[x.index() + 1] += 1;
                    }
                }
            }
            for succ in block.successors() {
                self.preds[succ.index()] += 1;
            }
        }
        for i in 0..n {
            self.user_start[i + 1] += self.user_start[i];
        }
        self.users.clear();
        self.users.resize(self.user_start[n] as usize, 0);
        let mut fill: Vec<u32> = self.user_start[..n].to_vec();
        for inst in body.insts() {
            for &op in inst.operands() {
                if let ValueKind::Inst(x) = op.kind() {
                    let at = &mut fill[x.index()];
                    self.users[*at as usize] = inst.id().raw();
                    *at += 1;
                }
            }
        }
        self.stamp.clear();
        self.stamp.resize(n, 0);
        self.round = 0;
    }

    /// Steps 1 and 2; whether a block lost its last predecessor.
    fn fold(&mut self, module: &Module, body: &Body) -> bool {
        // `ReversePostOrderTraversal` from the entry: successors in order.
        self.order.clear();
        self.stack.clear();
        let mut seen = vec![false; body.block_count()];
        seen[body.entry().index()] = true;
        self.stack.push((body.entry().raw(), 0));
        while let Some(top) = self.stack.last_mut() {
            let (b, next) = *top;
            top.1 += 1;
            let succ = body.block(BlockId::new(b)).successors().nth(next as usize);
            match succ {
                Some(s) if !seen[s.index()] => {
                    seen[s.index()] = true;
                    self.stack.push((s.raw(), 0));
                }
                Some(_) => {}
                None => {
                    self.order.push(b);
                    self.stack.pop();
                }
            }
        }
        self.calls.clear();
        for &b in self.order.iter().rev() {
            for inst in body.block(BlockId::new(b)).insts() {
                if inst.call().is_some_and(|site| folds(&site)) && inst.opcode() == Opcode::Call {
                    self.calls.push(inst.id().raw());
                }
            }
        }
        let mut has_dead = false;
        for k in 0..self.calls.len() {
            let c = self.calls[k];
            if self.lat[c as usize] != Lat::Kept {
                continue;
            }
            let inst = body.inst(InstId::new(c));
            let Some(site) = inst.call() else { continue };
            let args = site.args();
            let value = match (site.intrinsic(), args) {
                (Some(Intrinsic::IsConstant), &[x]) => Lat::Int(u64::from(is_manifest(module, x))),
                (Some(Intrinsic::ObjectSize), &[_, min, ..]) => match int_bits(module, inst.ty()) {
                    Some(bits) => Lat::Int(object_size(module, min) & mask(bits)),
                    None => Lat::Top,
                },
                _ => Lat::Top,
            };
            has_dead |= self.replace(module, body, c, value);
        }
        has_dead
    }

    /// `replaceConditionalBranchesOnConstant` for one call.
    fn replace(&mut self, module: &Module, body: &Body, call: u32, value: Lat) -> bool {
        self.round += 1;
        self.list.clear();
        self.unsimplified.clear();
        self.lat[call as usize] = value;
        self.push_users(call);
        let mut at = 0;
        while at < self.list.len() {
            let i = self.list[at];
            at += 1;
            match self.simplify(module, body, body.inst(InstId::new(i))) {
                None => self.unsimplified.push(i),
                Some(lat) => {
                    self.set(i, lat);
                    self.push_users(i);
                }
            }
        }
        let mut has_dead = false;
        for k in 0..self.unsimplified.len() {
            let i = self.unsimplified[k];
            let inst = body.inst(InstId::new(i));
            let InstKind::CondBr { cond, if_true, if_false } = inst.kind() else { continue };
            if self.action[i as usize] != Action::Emit {
                continue;
            }
            let (target, other) = match self.opd(module, cond) {
                Opd::Int(0) => (if_false, if_true),
                Opd::Int(1) => (if_true, if_false),
                _ => continue,
            };
            if target == other {
                continue;
            }
            let source = self.block_of[i as usize];
            self.remove_pred(module, body, other, source);
            self.action[i as usize] = Action::Jump(target);
            let p = &mut self.preds[other.index()];
            *p = p.saturating_sub(1);
            has_dead |= *p == 0;
        }
        has_dead
    }

    fn set(&mut self, i: u32, lat: Lat) {
        self.lat[i as usize] = lat;
        if let Lat::Alias(v) = lat
            && let Some(x) = v.as_inst()
        {
            self.alias_from.push((x.raw(), i));
        }
    }

    /// Adds the users of `i` (and of what was replaced by it) not yet in
    /// this round's list and not replaced.
    fn push_users(&mut self, i: u32) {
        let mut pending = vec![i];
        while let Some(x) = pending.pop() {
            let (start, end) =
                (self.user_start[x as usize] as usize, self.user_start[x as usize + 1] as usize);
            for k in start..end {
                let u = self.users[k];
                if self.stamp[u as usize] != self.round && self.lat[u as usize] == Lat::Kept {
                    self.stamp[u as usize] = self.round;
                    self.list.push(u);
                }
            }
            for &(to, from) in &self.alias_from {
                if to == x {
                    pending.push(from);
                }
            }
        }
    }

    /// An operand as the IR has it now.
    fn opd(&self, module: &Module, mut value: Value) -> Opd {
        loop {
            match value.kind() {
                ValueKind::Inst(x) => match self.lat.get(x.index()).copied().unwrap_or(Lat::Kept) {
                    Lat::Kept => return Opd::Val(value),
                    Lat::Int(k) => return Opd::Int(k),
                    Lat::Top => return Opd::Top,
                    Lat::Alias(v) => value = v,
                },
                ValueKind::Const(id) => {
                    return match module.consts.get(id) {
                        Const::Int { value, .. } => Opd::Int(value),
                        Const::Zero(ty) if int_bits(module, ty).is_some() => Opd::Int(0),
                        Const::Null(_) | Const::Aggregate { .. } | Const::Bytes { .. } => {
                            Opd::Val(value)
                        }
                        _ => Opd::Top,
                    };
                }
                _ => return Opd::Val(value),
            }
        }
    }

    fn lat_of(opd: Opd) -> Lat {
        match opd {
            Opd::Int(k) => Lat::Int(k),
            Opd::Top => Lat::Top,
            Opd::Val(v) => Lat::Alias(v),
        }
    }

    /// What `InstSimplify` makes of `inst` (the subset hod models; `None`
    /// is "not simplified").
    fn simplify(&self, module: &Module, body: &Body, inst: Inst<'_>) -> Option<Lat> {
        let op = inst.opcode();
        match inst.kind() {
            InstKind::Binary { lhs, rhs } => {
                let bits = int_bits(module, inst.ty())?;
                binary(op, bits, self.opd(module, lhs), self.opd(module, rhs))
            }
            InstKind::Cast { arg } if matches!(op, Opcode::ZExt | Opcode::SExt | Opcode::Trunc) => {
                let bits = int_bits(module, inst.ty())?;
                let from = int_bits(module, body.value_type(module, arg))?;
                match self.opd(module, arg) {
                    Opd::Int(k) => Some(Lat::Int(match op {
                        Opcode::SExt => sext(k, from) as u64 & mask(bits),
                        _ => k & mask(bits),
                    })),
                    Opd::Top => Some(Lat::Top),
                    Opd::Val(_) => None,
                }
            }
            InstKind::ICmp { pred, lhs, rhs } => {
                let bits = int_bits(module, body.value_type(module, lhs))?;
                icmp(pred, bits, self.opd(module, lhs), self.opd(module, rhs))
            }
            InstKind::Select { cond, if_true, if_false } => match self.opd(module, cond) {
                Opd::Int(k) => {
                    let arm = if k & 1 == 1 { if_true } else { if_false };
                    Some(Self::lat_of(self.opd(module, arm)))
                }
                Opd::Top => Some(Lat::Top),
                Opd::Val(_) => {
                    let (a, b) = (self.opd(module, if_true), self.opd(module, if_false));
                    (a == b && a != Opd::Top).then(|| Self::lat_of(a))
                }
            },
            InstKind::Unary { arg } if op == Opcode::Freeze => match self.opd(module, arg) {
                Opd::Int(k) => Some(Lat::Int(k)),
                Opd::Top => Some(Lat::Top),
                Opd::Val(_) => None,
            },
            InstKind::Phi(phi) => {
                let block = self.block_of[inst.id().index()];
                let mut common: Option<Opd> = None;
                let mut undef = false;
                let mut skipped = self.dead_preds(block);
                for (value, pred) in phi.incoming() {
                    if let Some(at) = skipped.iter().position(|&p| p == pred.raw()) {
                        skipped.swap_remove(at);
                        continue;
                    }
                    if value.as_inst() == Some(inst.id()) {
                        continue;
                    }
                    if value.as_const().is_some_and(|id| {
                        matches!(module.consts.get(id), Const::Undef(_) | Const::Poison(_))
                    }) {
                        undef = true;
                        continue;
                    }
                    let opd = self.opd(module, value);
                    if opd == Opd::Top || common.is_some_and(|c| c != opd) {
                        return None;
                    }
                    common = Some(opd);
                }
                match common? {
                    Opd::Int(k) => Some(Lat::Int(k)),
                    // A value with `undef` beside it only if it dominates
                    // the `phi`: not modelled.
                    Opd::Val(v) if !undef => Some(Lat::Alias(v)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The predecessors whose `phi` entries in `block` were removed.
    fn dead_preds(&self, block: u32) -> Vec<u32> {
        self.dead_in.iter().filter(|&&(b, _)| b == block).map(|&(_, p)| p).collect()
    }

    /// `BasicBlock::removePredecessor`: one `phi` entry for `pred` goes; a
    /// `phi` left with one value (`hasConstantValue`) is replaced by it.
    fn remove_pred(&mut self, module: &Module, body: &Body, block: BlockId, pred: u32) {
        self.dead_in.push((block.raw(), pred));
        for phi in body.block(block).phis() {
            let i = phi.id().raw();
            if self.lat[i as usize] != Lat::Kept {
                continue;
            }
            let InstKind::Phi(p) = phi.kind() else { continue };
            let mut skipped = self.dead_preds(block.raw());
            let mut common: Option<Opd> = None;
            let mut same = true;
            for (value, from) in p.incoming() {
                if let Some(at) = skipped.iter().position(|&q| q == from.raw()) {
                    skipped.swap_remove(at);
                    continue;
                }
                if value.as_inst() == Some(phi.id()) {
                    continue;
                }
                let opd = self.opd(module, value);
                if opd == Opd::Top || common.is_some_and(|c| c != opd) {
                    same = false;
                    break;
                }
                common = Some(opd);
            }
            if same && let Some(opd) = common {
                self.set(i, Self::lat_of(opd));
            }
        }
    }

    /// The successors of `block` after the decisions so far.
    fn succs(&self, body: &Body, block: BlockId) -> Vec<BlockId> {
        let term = body.block(block).terminator();
        match self.action[term.id().index()] {
            Action::Jump(target) => vec![target],
            Action::Unreachable => Vec::new(),
            _ => term.successors().collect(),
        }
    }

    /// The blocks reachable over the decided terminators (`llc`'s
    /// `UnreachableBlockElim`).
    fn walk(&mut self, body: &Body) {
        self.reach.clear();
        self.reach.resize(body.block_count(), false);
        self.reach[body.entry().index()] = true;
        let mut work = vec![body.entry()];
        while let Some(b) = work.pop() {
            for s in self.succs(body, b) {
                if !self.reach[s.index()] {
                    self.reach[s.index()] = true;
                    work.push(s);
                }
            }
        }
    }

    /// Ends the block before `stop` (`changeToUnreachable`).
    fn end_block(&mut self, module: &Module, body: &Body, block: BlockId, stop: InstId) {
        for s in self.succs(body, block) {
            self.remove_pred(module, body, s, block.raw());
        }
        let term = body.block(block).terminator().id();
        for i in stop.raw()..term.raw() {
            self.action[i as usize] = Action::Drop;
        }
        self.action[term.index()] = Action::Unreachable;
    }

    /// Step 3: `markAliveBlocks`, in its order.
    fn mark_alive(&mut self, module: &Module, body: &Body) {
        self.reach.clear();
        self.reach.resize(body.block_count(), false);
        self.reach[body.entry().index()] = true;
        let mut work = vec![body.entry()];
        while let Some(b) = work.pop() {
            let block = body.block(b);
            let mut stop = None;
            for inst in block.insts() {
                match inst.kind() {
                    InstKind::Call(site) if inst.opcode() == Opcode::Call => {
                        // A call through `null` is kept as written
                        // (WP-269); `llc` would end the block here.
                        if let Callee::Value(callee) = site.callee()
                            && ptr_kind(module, callee) == Ptr::Undef
                        {
                            stop = Some(inst.id());
                            break;
                        }
                        if no_return(module, &site) && site.tail() != TailKind::MustTail {
                            let next = InstId::new(inst.id().raw() + 1);
                            if body.inst(next).opcode() != Opcode::Unreachable {
                                stop = Some(next);
                            }
                            break;
                        }
                    }
                    // A store to `null` is kept as written (WP-269).
                    InstKind::Store { ptr, mem, .. }
                        if !mem.is_volatile() && ptr_kind(module, ptr) == Ptr::Undef =>
                    {
                        stop = Some(inst.id());
                        break;
                    }
                    _ => {}
                }
            }
            if let Some(stop) = stop {
                self.end_block(module, body, b, stop);
                continue;
            }
            self.alive_terminator(module, body, b);
            for s in self.succs(body, b) {
                if !self.reach[s.index()] {
                    self.reach[s.index()] = true;
                    work.push(s);
                }
            }
        }
    }

    /// Step 3 at the terminator of `b`: `ConstantFoldTerminator`. An
    /// `invoke` (through `null` or `undef`, of a `nounwind` or `noreturn`
    /// callee) and an `indirectbr` on a `blockaddress` are kept as
    /// written with all their edges (WP-269): `llc` rewrites them, hod
    /// emits them as it does outside the folding.
    fn alive_terminator(&mut self, module: &Module, body: &Body, b: BlockId) {
        let term = body.block(b).terminator();
        let t = term.id().index();
        match term.kind() {
            // `ConstantFoldTerminator`.
            InstKind::CondBr { cond, if_true, if_false } if self.action[t] == Action::Emit => {
                if if_true == if_false {
                    self.remove_pred(module, body, if_true, b.raw());
                    self.action[t] = Action::Jump(if_true);
                } else {
                    let (target, other) = match self.opd(module, cond) {
                        Opd::Int(0) => (Some(if_false), if_true),
                        Opd::Int(1) => (Some(if_true), if_false),
                        _ => (None, if_true),
                    };
                    if let Some(target) = target {
                        self.remove_pred(module, body, other, b.raw());
                        self.action[t] = Action::Jump(target);
                    }
                }
            }
            InstKind::Switch(switch) => {
                if let Opd::Int(k) = self.opd(module, switch.cond) {
                    let dest = switch
                            .cases()
                            .find(|&(c, _)| {
                                matches!(module.consts.get(c), Const::Int { value, .. } if value == k)
                            })
                            .map_or(switch.default, |(_, d)| d);
                    let mut kept = false;
                    for s in term.successors() {
                        if s == dest && !kept {
                            kept = true;
                        } else {
                            self.remove_pred(module, body, s, b.raw());
                        }
                    }
                    self.action[t] = Action::Jump(dest);
                }
            }
            _ => {}
        }
    }

    /// Whether the edge `pred -> block` is taken in the emitted code.
    fn edge_live(&self, body: &Body, pred: BlockId, block: BlockId) -> bool {
        self.reached(pred) && (!self.active || self.succs(body, pred).contains(&block))
    }

    /// The instructions `SelectionDAG` deletes (§5.5.1 "WP-084"): no
    /// effect, no use but by a folded call or by an instruction not
    /// emitted, not used outside its block or by a `phi`. Once a branch
    /// folded, uses in blocks not emitted, by instructions not emitted,
    /// by a folded terminator's condition and by `phi` entries of edges
    /// gone do not count, and a folded call may go too. Whether any is.
    fn decide(&mut self, body: &Body) -> bool {
        let n = body.inst_count();
        let words = n.div_ceil(64);
        let active = self.active;
        self.uses.clear();
        self.uses.resize(n, 0);
        self.bits.clear();
        self.bits.resize(2 * words, 0);
        let set = |bits: &mut [u64], base: usize, i: usize| bits[base + i / 64] |= 1 << (i % 64);
        let get = |bits: &[u64], base: usize, i: usize| bits[base + i / 64] >> (i % 64) & 1 != 0;
        for site in body.calls() {
            if folds(&site) {
                set(&mut self.bits, 0, site.inst().index());
            }
        }
        for block in body.blocks() {
            if active && !self.reached(block.id()) {
                continue;
            }
            let range = block.inst_range();
            for inst in block.insts() {
                let i = inst.id().index();
                if get(&self.bits, 0, i) || (active && self.action[i] != Action::Emit) {
                    continue;
                }
                let phi = inst.opcode() == Opcode::Phi;
                let count = |uses: &mut Vec<u32>, op: Value| {
                    if let ValueKind::Inst(x) = op.kind() {
                        let u = &mut uses[x.index()];
                        *u = (*u & EXPORTED) | ((*u & !EXPORTED) + 1).min(!EXPORTED);
                        if phi || !range.contains(&x.raw()) {
                            *u |= EXPORTED;
                        }
                    }
                };
                if let (true, InstKind::Phi(p)) = (active, inst.kind()) {
                    for (value, pred) in p.incoming() {
                        if self.edge_live(body, pred, block.id()) {
                            count(&mut self.uses, value);
                        }
                    }
                } else {
                    for &op in inst.operands() {
                        count(&mut self.uses, op);
                    }
                }
            }
        }
        let mut any = false;
        for block in body.blocks() {
            if active && !self.reached(block.id()) {
                continue;
            }
            for inst in block.insts().rev() {
                let i = inst.id().index();
                let folded = get(&self.bits, 0, i);
                if self.uses[i] != 0
                    || inst.ty() == TypeId::VOID
                    || (active && self.action[i] != Action::Emit)
                    || !(if folded { active } else { removable(inst) })
                {
                    continue;
                }
                set(&mut self.bits, words, i);
                any = true;
                for &op in inst.operands() {
                    if let ValueKind::Inst(x) = op.kind() {
                        let u = &mut self.uses[x.index()];
                        if *u & !EXPORTED != 0 {
                            *u -= 1;
                        }
                    }
                }
            }
        }
        any
    }
}

/// `InstSimplify` on an integer binary operation of `bits` bits.
fn binary(op: Opcode, bits: u32, lhs: Opd, rhs: Opd) -> Option<Lat> {
    let ones = mask(bits);
    match (lhs, rhs) {
        (Opd::Top, _) | (_, Opd::Top) => Some(Lat::Top),
        (Opd::Int(x), Opd::Int(y)) => Some(fold_binary(op, bits, x, y).map_or(Lat::Top, Lat::Int)),
        (Opd::Val(x), Opd::Val(y)) if x == y => match op {
            Opcode::Xor | Opcode::Sub => Some(Lat::Int(0)),
            Opcode::And | Opcode::Or => Some(Lat::Alias(x)),
            _ => None,
        },
        (Opd::Val(_), Opd::Val(_)) => None,
        (Opd::Int(k), Opd::Val(v)) | (Opd::Val(v), Opd::Int(k)) => {
            let left = matches!(lhs, Opd::Int(_));
            match op {
                Opcode::And | Opcode::Mul if k == 0 => Some(Lat::Int(0)),
                Opcode::And if k == ones => Some(Lat::Alias(v)),
                Opcode::Or if k == ones => Some(Lat::Int(ones)),
                Opcode::Or | Opcode::Xor | Opcode::Add if k == 0 => Some(Lat::Alias(v)),
                Opcode::Mul if k == 1 => Some(Lat::Alias(v)),
                Opcode::Sub | Opcode::Shl | Opcode::LShr | Opcode::AShr if !left && k == 0 => {
                    Some(Lat::Alias(v))
                }
                Opcode::Shl | Opcode::LShr | Opcode::AShr if !left && k >= u64::from(bits) => {
                    Some(Lat::Top)
                }
                Opcode::Shl | Opcode::LShr | Opcode::AShr if left && k == 0 => Some(Lat::Int(0)),
                Opcode::AShr if left && k == ones => Some(Lat::Int(ones)),
                _ => None,
            }
        }
    }
}

/// `x op y` of `bits` bits; `None` where the result may be `poison` (a
/// wrap that `nsw`/`nuw` would make poison, a shift out of range, bits
/// an `exact` shift would lose): hod's IR does not keep those flags.
fn fold_binary(op: Opcode, bits: u32, x: u64, y: u64) -> Option<u64> {
    let m = mask(bits);
    let (sx, sy) = (i128::from(sext(x, bits)), i128::from(sext(y, bits)));
    let (ux, uy) = (u128::from(x), u128::from(y));
    let r = match op {
        Opcode::Add => {
            if ux + uy > u128::from(m) || !fits_signed(sx + sy, bits) {
                return None;
            }
            x.wrapping_add(y)
        }
        Opcode::Sub => {
            if x < y || !fits_signed(sx - sy, bits) {
                return None;
            }
            x.wrapping_sub(y)
        }
        Opcode::Mul => {
            if ux * uy > u128::from(m) || !fits_signed(sx * sy, bits) {
                return None;
            }
            x.wrapping_mul(y)
        }
        Opcode::And => x & y,
        Opcode::Or => x | y,
        Opcode::Xor => x ^ y,
        Opcode::Shl | Opcode::LShr | Opcode::AShr => {
            if y >= u64::from(bits) {
                return None;
            }
            match op {
                Opcode::Shl => {
                    let r = (x << y) & m;
                    if r >> y != x || sext(r, bits) >> y != sext(x, bits) {
                        return None;
                    }
                    r
                }
                Opcode::LShr => {
                    if x & ((1u64 << y) - 1) != 0 {
                        return None;
                    }
                    x >> y
                }
                _ => {
                    if x & ((1u64 << y) - 1) != 0 {
                        return None;
                    }
                    (sext(x, bits) >> y) as u64
                }
            }
        }
        _ => return None,
    };
    Some(r & m)
}

/// `InstSimplify` on an integer compare of `bits` bits.
fn icmp(pred: IntPredicate, bits: u32, lhs: Opd, rhs: Opd) -> Option<Lat> {
    use IntPredicate as P;
    let ones = mask(bits);
    let truth = |t: bool| Some(Lat::Int(u64::from(t)));
    match (lhs, rhs) {
        (Opd::Top, _) | (_, Opd::Top) => Some(Lat::Top),
        (Opd::Int(x), Opd::Int(y)) => {
            let (sx, sy) = (sext(x, bits), sext(y, bits));
            truth(match pred {
                P::Eq => x == y,
                P::Ne => x != y,
                P::Ugt => x > y,
                P::Uge => x >= y,
                P::Ult => x < y,
                P::Ule => x <= y,
                P::Sgt => sx > sy,
                P::Sge => sx >= sy,
                P::Slt => sx < sy,
                P::Sle => sx <= sy,
            })
        }
        (Opd::Val(x), Opd::Val(y)) if x == y => {
            truth(matches!(pred, P::Eq | P::Uge | P::Ule | P::Sge | P::Sle))
        }
        (Opd::Val(_), Opd::Val(_)) => None,
        (Opd::Int(k), Opd::Val(v)) | (Opd::Val(v), Opd::Int(k)) => {
            // As `value pred k`.
            let pred = if matches!(lhs, Opd::Int(_)) { swapped(pred) } else { pred };
            let (smin, smax) = (1u64 << (bits - 1), ones >> 1);
            match pred {
                P::Ult if k == 0 => truth(false),
                P::Uge if k == 0 => truth(true),
                P::Ugt if k == ones => truth(false),
                P::Ule if k == ones => truth(true),
                P::Slt if k == smin => truth(false),
                P::Sge if k == smin => truth(true),
                P::Sgt if k == smax => truth(false),
                P::Sle if k == smax => truth(true),
                P::Eq if bits == 1 && k == 1 => Some(Lat::Alias(v)),
                P::Ne if bits == 1 && k == 0 => Some(Lat::Alias(v)),
                _ => None,
            }
        }
    }
}

fn swapped(pred: IntPredicate) -> IntPredicate {
    use IntPredicate as P;
    match pred {
        P::Ugt => P::Ult,
        P::Uge => P::Ule,
        P::Ult => P::Ugt,
        P::Ule => P::Uge,
        P::Sgt => P::Slt,
        P::Sge => P::Sle,
        P::Slt => P::Sgt,
        P::Sle => P::Sge,
        same => same,
    }
}
