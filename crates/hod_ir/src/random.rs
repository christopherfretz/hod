//! Randomized tests of function bodies (`spec/ir.md` §5): a deterministic
//! generator of well-formed bodies, every one of which must verify, and
//! mutations of their tables, every one of which `verify` must name.
//!
//! This is a unit-test module rather than an integration test because the
//! mutations write the tables directly, which no public API can do.

// Random numbers are cut to size all over this file.
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]

use crate::attrs::{CallConv, ParamAttrs};
use crate::body::{BlockData, Body, Bundle, Clause, UNSET, UseFacts};
use crate::builder::{CallDesc, FunctionBuilder};
use crate::error::{IrErrorKind, IrResult};
use crate::ids::{BlockId, ConstId, DiId, LocId, SymId, TypeId};
use crate::inst::{
    AtomicOrdering, FloatPredicate, IntPredicate, MemFlags, Opcode, RmwOp, SyncScope, TailKind,
    word,
};
use crate::intrinsic::Intrinsic;
use crate::module::Module;
use crate::target::TargetSpec;
use crate::value::{InstId, SlotId, Value, ValueKind};

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

    /// A number below `bound`.
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn small(&mut self, bound: usize) -> u32 {
        self.below(bound) as u32
    }

    /// True once in `n` times.
    fn chance(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            items.swap(index, self.below(index + 1));
        }
    }
}

// The classes of value the generator keeps: one type each.
const BOOL: usize = 0;
const INT: usize = 1;
const LONG: usize = 2;
const FLT: usize = 3;
const DBL: usize = 4;
const PTR: usize = 5;
const AGG: usize = 6;
const VEC: usize = 7;
const CLASSES: usize = 8;

/// How a planned block ends; the numbers index the block list.
enum Term {
    Ret,
    Unreachable,
    Resume,
    Br(usize),
    CondBr(usize, usize),
    Switch(usize, Vec<usize>),
    IndirectBr(Vec<usize>),
    Invoke(usize, usize),
    CallBr(usize, Vec<usize>),
}

impl Term {
    fn successors(&self) -> Vec<usize> {
        match self {
            Term::Ret | Term::Unreachable | Term::Resume => Vec::new(),
            Term::Br(to) => vec![*to],
            Term::CondBr(yes, no) => vec![*yes, *no],
            Term::Invoke(normal, unwind) => vec![*normal, *unwind],
            Term::IndirectBr(targets) => targets.clone(),
            Term::Switch(first, rest) | Term::CallBr(first, rest) => {
                let mut all = vec![*first];
                all.extend(rest);
                all
            }
        }
    }

    fn plan(rng: &mut Rng, is_pad: bool, normals: &[usize], pads: &[usize]) -> Term {
        if is_pad && rng.chance(3) {
            return Term::Resume;
        }
        if normals.is_empty() {
            return if rng.chance(2) { Term::Ret } else { Term::Unreachable };
        }
        let list = |rng: &mut Rng, bound: usize| -> Vec<usize> {
            (0..rng.below(bound)).map(|_| rng.pick(normals)).collect()
        };
        let to = rng.pick(normals);
        match rng.below(12) {
            0 | 1 => Term::Ret,
            2 => Term::Unreachable,
            3 | 4 => Term::Br(to),
            5..=7 => Term::CondBr(to, rng.pick(normals)),
            8 => Term::Switch(to, list(rng, 5)),
            9 => Term::IndirectBr(list(rng, 4)),
            10 => Term::CallBr(to, list(rng, 3)),
            _ if pads.is_empty() => Term::Br(to),
            _ => Term::Invoke(to, rng.pick(pads)),
        }
    }
}

/// A function type the generated calls use: its id, the class of its
/// result, the classes of its parameters, whether it is variadic.
type Callee = (TypeId, Option<usize>, &'static [usize], bool);

struct Hole {
    value: Value,
    class: usize,
    resolved: bool,
}

struct Generator {
    rng: Rng,
    b: FunctionBuilder,
    tys: [TypeId; CLASSES],
    /// What a value of each class can be: constants, arguments, slots,
    /// symbols, and every instruction result so far. Nothing here checks
    /// dominance, as nothing in `verify` does.
    pool: [Vec<Value>; CLASSES],
    holes: Vec<Hole>,
    /// Phi entries to add once every block is built.
    deferred: Vec<(Value, Value, BlockId)>,
    ret: Option<usize>,
    locs: bool,
    callees: [Callee; 4],
    memcpy_ty: TypeId,
    void_fn: TypeId,
    exn: TypeId,
    pair: TypeId,
    far_ptr: TypeId,
    cases: [[ConstId; 4]; 2],
    catch: ConstId,
    filter: ConstId,
}

impl Generator {
    /// A value of `class`: an existing one, or one reserved now and defined
    /// later (§5.3).
    fn value(&mut self, class: usize) -> Value {
        match self.rng.below(16) {
            0 | 1 => {
                let value = self.b.reserve(self.tys[class]).unwrap();
                self.holes.push(Hole { value, class, resolved: false });
                value
            }
            2 => {
                let old: Vec<Value> =
                    self.holes.iter().filter(|h| h.class == class).map(|h| h.value).collect();
                if old.is_empty() { self.rng.pick(&self.pool[class]) } else { self.rng.pick(&old) }
            }
            _ => self.rng.pick(&self.pool[class]),
        }
    }

    /// Records a new value of `class`, sometimes as the definition of a
    /// reserved one.
    fn define(&mut self, class: usize, result: IrResult<Value>) {
        let value = result.unwrap();
        self.pool[class].push(value);
        if self.rng.chance(6)
            && let Some(hole) = self.holes.iter_mut().find(|h| h.class == class && !h.resolved)
        {
            self.b.resolve(hole.value, value).unwrap();
            hole.resolved = true;
        }
    }

    fn mem(&mut self, atomic: Option<fn(AtomicOrdering) -> bool>) -> MemFlags {
        let mut mem = MemFlags::aligned(1 << self.rng.below(5)).unwrap();
        if self.rng.chance(4) {
            mem = mem.with_volatile();
        }
        if let Some(valid) = atomic {
            let orderings: Vec<AtomicOrdering> =
                AtomicOrdering::ALL.iter().copied().filter(|&o| valid(o)).collect();
            let scope =
                if self.rng.chance(3) { SyncScope::SingleThread } else { SyncScope::System };
            mem = mem.with_atomic(self.rng.pick(&orderings), scope);
        }
        mem
    }

    fn call(&mut self, invoke: Option<(BlockId, BlockId)>, callbr: Option<(BlockId, &[BlockId])>) {
        let emit = |b: &mut FunctionBuilder, ty: TypeId, desc: &CallDesc<'_>| match (invoke, callbr)
        {
            (Some((normal, unwind)), _) => b.invoke(ty, desc, normal, unwind),
            (None, Some((fallthrough, indirect))) => b.callbr(ty, desc, fallthrough, indirect),
            (None, None) => b.call(ty, desc),
        };
        let which = self.rng.below(6);
        if which == 4 {
            let args = [self.value(PTR), self.value(PTR), self.value(LONG), self.value(BOOL)];
            let overloads = [TypeId::PTR, TypeId::PTR, TypeId::I64];
            let memcpy = CallDesc::intrinsic(Intrinsic::Memcpy, &overloads, self.memcpy_ty, &args);
            let desc = CallDesc { nounwind: true, ..memcpy };
            emit(&mut self.b, TypeId::VOID, &desc).unwrap();
            return;
        }
        if which == 5 {
            let name = self.rng.pick(&["llvm.hod.first", "llvm.hod.second.i32"]);
            let desc = CallDesc::intrinsic(Intrinsic::Other(name), &[], self.void_fn, &[]);
            emit(&mut self.b, TypeId::VOID, &desc).unwrap();
            return;
        }
        let (fn_ty, ret, params, variadic) = self.callees[which];
        let mut args: Vec<Value> = params.iter().map(|&class| self.value(class)).collect();
        if variadic {
            for _ in 0..self.rng.below(3) {
                let class = self.rng.below(CLASSES);
                args.push(self.value(class));
            }
        }
        let callee = if self.rng.chance(3) {
            self.value(PTR)
        } else {
            Value::sym(SymId::new(self.rng.small(50))).unwrap()
        };
        let mut inputs = Vec::new();
        let bundle_count = if self.rng.chance(4) { 1 + self.rng.below(2) } else { 0 };
        if bundle_count > 0 {
            for _ in 0..self.rng.below(3) {
                let class = self.rng.below(CLASSES);
                inputs.push(self.value(class));
            }
        }
        let bundles =
            [Bundle { tag: "deopt", inputs: &inputs }, Bundle { tag: "kcfi", inputs: &[] }];
        let signext = ParamAttrs::NONE.with(ParamAttrs::SIGNEXT).unwrap();
        let attrs: Vec<ParamAttrs> = if self.rng.chance(3) {
            args.iter()
                .map(|_| if self.rng.chance(2) { signext } else { ParamAttrs::NONE })
                .collect()
        } else {
            Vec::new()
        };
        let desc = CallDesc {
            cc: self.rng.pick(&[CallConv::C, CallConv::FAST, CallConv::COLD]),
            tail: self.rng.pick(TailKind::ALL),
            noreturn: self.rng.chance(8),
            nounwind: self.rng.chance(2),
            returns_twice: self.rng.chance(16),
            ret_attrs: if self.rng.chance(4) { signext } else { ParamAttrs::NONE },
            arg_attrs: &attrs,
            bundles: &bundles[..bundle_count],
            ..CallDesc::new(callee, fn_ty, &args)
        };
        let ty = ret.map_or(TypeId::VOID, |class| self.tys[class]);
        let result = emit(&mut self.b, ty, &desc);
        match ret {
            Some(class) => self.define(class, result),
            None => {
                result.unwrap();
            }
        }
    }

    /// One instruction that is neither a phi nor a terminator.
    #[allow(clippy::too_many_lines)]
    fn instruction(&mut self) {
        if self.locs && self.rng.chance(3) {
            let loc = if self.rng.chance(5) { LocId::NONE } else { LocId::new(self.rng.small(40)) };
            self.b.set_loc(loc);
        }
        let tys = self.tys;
        let wide = if self.rng.chance(2) { LONG } else { INT };
        match self.rng.below(25) {
            0 => {
                let ops: Vec<Opcode> =
                    Opcode::ALL.iter().copied().filter(|op| op.is_int_binary()).collect();
                let (lhs, rhs) = (self.value(wide), self.value(wide));
                let result = self.b.binary(self.rng.pick(&ops), tys[wide], lhs, rhs);
                self.define(wide, result);
            }
            1 => {
                let ops: Vec<Opcode> =
                    Opcode::ALL.iter().copied().filter(|op| op.is_float_binary()).collect();
                let class = if self.rng.chance(2) { FLT } else { DBL };
                let (lhs, rhs) = (self.value(class), self.value(class));
                let result = self.b.binary(self.rng.pick(&ops), tys[class], lhs, rhs);
                self.define(class, result);
            }
            2 => {
                let class = self.rng.pick(&[INT, LONG, PTR, BOOL]);
                let (lhs, rhs) = (self.value(class), self.value(class));
                let pred = self.rng.pick(IntPredicate::ALL);
                let result = self.b.icmp(pred, TypeId::I1, lhs, rhs);
                self.define(BOOL, result);
            }
            3 => {
                let (lhs, rhs) = (self.value(FLT), self.value(FLT));
                let pred = self.rng.pick(FloatPredicate::ALL);
                let result = self.b.fcmp(pred, TypeId::I1, lhs, rhs);
                self.define(BOOL, result);
            }
            4 | 5 => {
                let (op, from, to) = self.rng.pick(&[
                    (Opcode::Trunc, LONG, INT),
                    (Opcode::Trunc, INT, BOOL),
                    (Opcode::ZExt, INT, LONG),
                    (Opcode::ZExt, BOOL, INT),
                    (Opcode::SExt, INT, LONG),
                    (Opcode::FPTrunc, DBL, FLT),
                    (Opcode::FPExt, FLT, DBL),
                    (Opcode::FPToUI, FLT, LONG),
                    (Opcode::FPToSI, DBL, INT),
                    (Opcode::UIToFP, INT, DBL),
                    (Opcode::SIToFP, LONG, FLT),
                    (Opcode::PtrToInt, PTR, INT),
                    (Opcode::PtrToAddr, PTR, LONG),
                    (Opcode::IntToPtr, LONG, PTR),
                    (Opcode::BitCast, INT, FLT),
                    (Opcode::BitCast, DBL, LONG),
                ]);
                let arg = self.value(from);
                let result = self.b.cast(op, tys[to], arg);
                self.define(to, result);
            }
            6 => {
                let arg = self.value(PTR);
                self.b.cast(Opcode::AddrSpaceCast, self.far_ptr, arg).unwrap();
            }
            7 | 8 => {
                let class = self.rng.below(CLASSES);
                let atomic = matches!(class, INT | LONG | PTR | FLT) && self.rng.chance(4);
                let mem = self.mem(atomic.then_some(AtomicOrdering::valid_for_load));
                let ptr = self.value(PTR);
                let result = self.b.load(tys[class], ptr, mem);
                self.define(class, result);
            }
            9 | 10 => {
                let class = self.rng.below(CLASSES);
                let atomic = matches!(class, INT | LONG | PTR | DBL) && self.rng.chance(4);
                let mem = self.mem(atomic.then_some(AtomicOrdering::valid_for_store));
                let (value, ptr) = (self.value(class), self.value(PTR));
                self.b.store(value, ptr, mem).unwrap();
            }
            11 => {
                let base = self.value(PTR);
                let offset = self.rng.next() as i64 >> self.rng.below(64);
                let result = self.b.ptr_add(TypeId::PTR, base, offset, &[]);
                self.define(PTR, result);
            }
            12 => {
                let base = self.value(PTR);
                let mut indices = Vec::new();
                for _ in 0..=self.rng.below(3) {
                    let class = if self.rng.chance(2) { LONG } else { INT };
                    indices.push((self.value(class), self.rng.next() >> self.rng.below(64)));
                }
                let result = self.b.ptr_add(TypeId::PTR, base, -16, &indices);
                self.define(PTR, result);
            }
            13 => {
                let agg = self.value(AGG);
                let (class, offset) = if self.rng.chance(2) { (INT, 0) } else { (LONG, 8) };
                let result = self.b.extract_value(tys[class], agg, offset);
                self.define(class, result);
            }
            14 => {
                let agg = self.value(AGG);
                let (class, offset) = if self.rng.chance(2) { (INT, 0) } else { (LONG, 8) };
                let field = self.value(class);
                let result = self.b.insert_value(tys[AGG], agg, field, offset);
                self.define(AGG, result);
            }
            15 => {
                let (vector, index) = (self.value(VEC), self.value(wide));
                let result = self.b.extract_element(TypeId::I32, vector, index);
                self.define(INT, result);
            }
            16 => {
                let (vector, element, index) = (self.value(VEC), self.value(INT), self.value(wide));
                let result = self.b.insert_element(tys[VEC], vector, element, index);
                self.define(VEC, result);
            }
            17 => {
                let (first, second) = (self.value(VEC), self.value(VEC));
                let mut mask = [0u32; 4];
                for entry in &mut mask {
                    *entry =
                        if self.rng.chance(6) { crate::SHUFFLE_UNDEF } else { self.rng.small(8) };
                }
                let result = self.b.shuffle_vector(tys[VEC], first, second, &mask);
                self.define(VEC, result);
            }
            18 => {
                let class = self.rng.below(CLASSES);
                let (cond, yes, no) = (self.value(BOOL), self.value(class), self.value(class));
                let result = self.b.select(tys[class], cond, yes, no);
                self.define(class, result);
            }
            19 => {
                let count = self.value(wide);
                let size = self.rng.next() >> self.rng.below(64);
                let result = self.b.alloca(TypeId::PTR, count, size, 1 << self.rng.below(8));
                self.define(PTR, result);
            }
            20 => {
                let class = self.rng.pick(&[INT, LONG, FLT, DBL, PTR]);
                let ops: Vec<RmwOp> = RmwOp::ALL
                    .iter()
                    .copied()
                    .filter(|&op| match class {
                        INT | LONG => !op.is_float(),
                        PTR => op == RmwOp::Xchg,
                        _ => op.is_float() || op == RmwOp::Xchg,
                    })
                    .collect();
                let mem = self.mem(Some(AtomicOrdering::valid_for_rmw));
                let (ptr, operand) = (self.value(PTR), self.value(class));
                let result = self.b.atomic_rmw(self.rng.pick(&ops), tys[class], ptr, operand, mem);
                self.define(class, result);
            }
            21 => {
                let mem = self.mem(Some(AtomicOrdering::valid_for_rmw));
                let failures: Vec<AtomicOrdering> = AtomicOrdering::ALL
                    .iter()
                    .copied()
                    .filter(|o| o.valid_for_cmpxchg_failure())
                    .collect();
                let (ptr, old, new) = (self.value(PTR), self.value(INT), self.value(INT));
                let failure = self.rng.pick(&failures);
                let weak = self.rng.chance(2);
                let both = self.b.cmpxchg(self.pair, ptr, old, new, mem, failure, weak).unwrap();
                let result = self.b.extract_value(TypeId::I1, both, 4);
                self.define(BOOL, result);
            }
            22 => {
                let class = self.rng.below(CLASSES);
                let arg = self.value(class);
                let result = if matches!(class, FLT | DBL) && self.rng.chance(2) {
                    self.b.fneg(tys[class], arg)
                } else {
                    self.b.freeze(tys[class], arg)
                };
                self.define(class, result);
            }
            23 => {
                if self.rng.chance(2) {
                    let orderings: Vec<AtomicOrdering> = AtomicOrdering::ALL
                        .iter()
                        .copied()
                        .filter(|o| o.valid_for_fence())
                        .collect();
                    self.b.fence(self.rng.pick(&orderings), SyncScope::System).unwrap();
                } else {
                    let class = self.rng.below(CLASSES);
                    let list = self.value(PTR);
                    let result = self.b.va_arg(tys[class], list);
                    self.define(class, result);
                }
            }
            _ => self.call(None, None),
        }
    }

    fn block(&mut self, index: usize, term: &Term, preds: &[usize], pad: bool, ids: &[BlockId]) {
        self.b.begin_block(ids[index]).unwrap();
        let phis = if preds.is_empty() { 0 } else { self.rng.below(3) };
        for _ in 0..phis {
            let class = self.rng.below(CLASSES);
            // One value for each predecessor, however many edges it has.
            let mut distinct = preds.to_vec();
            distinct.sort_unstable();
            distinct.dedup();
            let values: Vec<Value> = distinct.iter().map(|_| self.value(class)).collect();
            let mut list: Vec<(Value, BlockId)> = preds
                .iter()
                .map(|from| (values[distinct.binary_search(from).unwrap()], ids[*from]))
                .collect();
            self.rng.shuffle(&mut list);
            let now = self.rng.below(list.len() + 1);
            let result = self.b.phi(self.tys[class], &list[..now]);
            let phi = *result.as_ref().unwrap();
            self.deferred.extend(list[now..].iter().map(|&(value, from)| (phi, value, from)));
            self.define(class, result);
        }
        let caught = pad.then(|| {
            let all = [Clause::Catch(self.catch), Clause::Filter(self.filter)];
            let clauses = &all[..self.rng.below(3)];
            let cleanup = clauses.is_empty() || self.rng.chance(2);
            self.b.landing_pad(self.exn, cleanup, clauses).unwrap()
        });
        for _ in 0..self.rng.below(10) {
            self.instruction();
        }
        let blocks = |list: &[usize]| list.iter().map(|&to| ids[to]).collect::<Vec<BlockId>>();
        match term {
            Term::Ret => {
                let value = self.ret.map(|class| self.value(class));
                self.b.ret(value).unwrap();
            }
            Term::Unreachable => {
                self.b.unreachable().unwrap();
            }
            Term::Resume => {
                self.b.resume(caught.unwrap()).unwrap();
            }
            Term::Br(to) => {
                self.b.br(ids[*to]).unwrap();
            }
            Term::CondBr(yes, no) => {
                let cond = self.value(BOOL);
                self.b.cond_br(cond, ids[*yes], ids[*no]).unwrap();
            }
            Term::Switch(default, targets) => {
                let wide = self.rng.chance(2);
                let cond = self.value(if wide { LONG } else { INT });
                let values = self.cases[usize::from(wide)];
                let cases: Vec<(ConstId, BlockId)> =
                    targets.iter().zip(values).map(|(&to, value)| (value, ids[to])).collect();
                self.b.switch(cond, ids[*default], &cases).unwrap();
            }
            Term::IndirectBr(targets) => {
                let addr = self.value(PTR);
                self.b.indirect_br(addr, &blocks(targets)).unwrap();
            }
            Term::Invoke(normal, unwind) => self.call(Some((ids[*normal], ids[*unwind])), None),
            Term::CallBr(fallthrough, indirect) => {
                self.call(None, Some((ids[*fallthrough], &blocks(indirect))));
            }
        }
    }
}

/// The body seed `seed` describes, built in `storage` if there is some,
/// and its module. With `complete` false, one reserved value is left
/// undefined.
#[allow(clippy::too_many_lines)]
pub(crate) fn generate(
    seed: u64,
    storage: Option<Body>,
    complete: bool,
) -> (Module, IrResult<Body>) {
    let mut rng = Rng(seed);
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let (i1, i32, i64, ptr) = (TypeId::I1, TypeId::I32, TypeId::I64, TypeId::PTR);
    let agg = m.types.struct_type(&[i32, i64], false).unwrap();
    let vec = m.types.vector(i32, 4).unwrap();
    let tys = [i1, i32, i64, TypeId::FLOAT, TypeId::DOUBLE, ptr, agg, vec];
    let mut function = |ret, params: &[TypeId], variadic| m.types.function(ret, params, variadic);
    let callees: [Callee; 4] = [
        (function(i32, &[i32, ptr], false).unwrap(), Some(INT), &[INT, PTR], false),
        (function(TypeId::VOID, &[], false).unwrap(), None, &[], false),
        (function(i64, &[i64], true).unwrap(), Some(LONG), &[LONG], true),
        (function(agg, &[vec, TypeId::DOUBLE], false).unwrap(), Some(AGG), &[VEC, DBL], false),
    ];
    let memcpy_ty = function(TypeId::VOID, &[ptr, ptr, i64, i1], false).unwrap();
    let void_fn = callees[1].0;

    let params: Vec<usize> = (0..rng.below(7)).map(|_| rng.below(CLASSES)).collect();
    let param_tys: Vec<TypeId> = params.iter().map(|&class| tys[class]).collect();
    let ret = if rng.chance(2) { None } else { Some(rng.below(CLASSES)) };
    let ret_ty = ret.map_or(TypeId::VOID, |class| tys[class]);
    let fn_ty = m.types.function(ret_ty, &param_tys, rng.chance(4)).unwrap();

    let mut pool: [Vec<Value>; CLASSES] = Default::default();
    for (class, values) in pool.iter_mut().enumerate() {
        let zero = m.consts.zero(&m.types, tys[class]).unwrap();
        let undef = m.consts.undef(&m.types, tys[class]).unwrap();
        values.extend([zero, undef].map(|constant| Value::constant(constant).unwrap()));
    }
    let mut cases = [[ConstId::new(0); 4]; 2];
    for (wide, row) in cases.iter_mut().enumerate() {
        let class = if wide == 1 { LONG } else { INT };
        for (index, case) in row.iter_mut().enumerate() {
            *case = m.consts.int(&m.types, tys[class], 10 + index as u64).unwrap();
            pool[class].push(Value::constant(*case).unwrap());
        }
    }
    pool[PTR].push(Value::sym(SymId::new(rng.small(100))).unwrap());
    let filter_ty = m.types.array(ptr, 2).unwrap();

    let sym = SymId::new(seed as u32 & 0xffff);
    let mut b = match storage {
        Some(storage) => FunctionBuilder::reusing(&m, sym, fn_ty, storage),
        None => FunctionBuilder::new(&m, sym, fn_ty),
    }
    .unwrap();
    for (index, &class) in params.iter().enumerate() {
        pool[class].push(b.arg(index as u32).unwrap());
    }
    for _ in 0..rng.below(4) {
        let slot = b.add_slot(rng.next() >> rng.below(64), 1 << rng.below(6)).unwrap();
        pool[PTR].push(b.slot(slot).unwrap());
        if rng.chance(2) {
            b.set_slot_var(slot, DiId::new(rng.small(9)), LocId::new(rng.small(9))).unwrap();
        }
    }

    // The control-flow graph first: phis need their predecessors. Block 0
    // is the entry and nothing branches to it; a landing-pad block is
    // reached only by unwinding.
    let count = 1 + rng.below(8);
    let ids: Vec<BlockId> = (0..count).map(|_| b.create_block().unwrap()).collect();
    let is_pad: Vec<bool> = (0..count).map(|index| index > 0 && rng.chance(5)).collect();
    let normals: Vec<usize> = (1..count).filter(|&index| !is_pad[index]).collect();
    let pads: Vec<usize> = (1..count).filter(|&index| is_pad[index]).collect();
    let terms: Vec<Term> =
        (0..count).map(|index| Term::plan(&mut rng, is_pad[index], &normals, &pads)).collect();
    let mut preds = vec![Vec::new(); count];
    for (from, term) in terms.iter().enumerate() {
        for to in term.successors() {
            preds[to].push(from);
        }
    }
    let mut order: Vec<usize> = (1..count).collect();
    rng.shuffle(&mut order);
    order.insert(0, 0);

    let mut g = Generator {
        locs: rng.chance(2),
        catch: m.consts.null(&m.types, ptr).unwrap(),
        filter: m.consts.zero(&m.types, filter_ty).unwrap(),
        exn: m.types.struct_type(&[ptr, i32], false).unwrap(),
        pair: m.types.struct_type(&[i32, i1], false).unwrap(),
        far_ptr: m.types.ptr(1).unwrap(),
        rng,
        b,
        tys,
        pool,
        holes: Vec::new(),
        deferred: Vec::new(),
        ret,
        callees,
        memcpy_ty,
        void_fn,
        cases,
    };
    for index in order {
        g.block(index, &terms[index], &preds[index], is_pad[index], &ids);
    }
    let mut deferred = std::mem::take(&mut g.deferred);
    g.rng.shuffle(&mut deferred);
    for (phi, value, from) in deferred {
        g.b.add_incoming(phi, value, from).unwrap();
    }
    if !complete && g.holes.iter().all(|hole| hole.resolved) {
        let value = g.b.reserve(TypeId::I32).unwrap();
        g.holes.push(Hole { value, class: INT, resolved: false });
    }
    let unresolved: Vec<usize> =
        (0..g.holes.len()).filter(|&index| !g.holes[index].resolved).collect();
    let skipped = if complete { None } else { Some(g.rng.pick(&unresolved)) };
    for index in unresolved {
        if skipped == Some(index) {
            continue;
        }
        // To a real value, or to a reserved value made earlier, which
        // cannot make a cycle.
        let class = g.holes[index].class;
        let earlier: Vec<Value> =
            g.holes[..index].iter().filter(|h| h.class == class).map(|h| h.value).collect();
        let target = if !earlier.is_empty() && g.rng.chance(4) {
            g.rng.pick(&earlier)
        } else {
            g.rng.pick(&g.pool[class])
        };
        g.b.resolve(g.holes[index].value, target).unwrap();
    }
    (m, g.b.finish())
}

const SEEDS: u64 = 1500;

/// §5.1, §5.3, §5.6: every generated body verifies, which includes that
/// the use facts the builder kept through every forward reference are the
/// ones `verify` recomputes; and the generator reaches every opcode but
/// the funclet ones.
#[test]
fn random_bodies_verify() {
    let mut seen = vec![0usize; Opcode::ALL.len()];
    let (mut insts, mut outside, mut next, mut arg_outside) = (0, 0, 0, 0);
    for seed in 0..SEEDS {
        let (m, body) = generate(seed, None, true);
        let body = body.unwrap();
        if let Err(error) = body.verify(&m) {
            panic!("seed {seed}: {error}\n{body:#?}");
        }
        insts += body.inst_count();
        for inst in body.insts() {
            seen[inst.opcode() as usize] += 1;
            outside += usize::from(inst.uses().used_outside_block());
            next += usize::from(inst.uses().only_use_is_next());
        }
        arg_outside += (0..body.arg_count())
            .filter(|&index| body.arg_uses(index).used_outside_block())
            .count();
    }
    for &op in Opcode::ALL {
        assert_eq!(seen[op as usize] == 0, op.is_funclet(), "{op:?}");
    }
    assert!(insts > 20 * SEEDS as usize, "{insts} instructions");
    assert!(outside > 1000 && next > 1000 && arg_outside > 100, "{outside} {next} {arg_outside}");
    eprintln!(
        "random bodies: {SEEDS} bodies, {insts} instructions; {outside} results used outside \
         their block, {next} only by the next instruction; {arg_outside} arguments used \
         outside the entry block"
    );
}

/// §5.3: a reserved value left undefined fails `finish` with
/// `UndefinedValue`, wherever it is and whether or not it is used.
#[test]
fn random_bodies_with_an_undefined_value_do_not_finish() {
    for seed in 0..SEEDS / 5 {
        let (_, body) = generate(seed, None, false);
        assert_eq!(body.unwrap_err().kind, IrErrorKind::UndefinedValue, "seed {seed}");
    }
}

/// §5.1: a body built in recycled storage equals the one built in fresh
/// storage, and the storage only ever grows.
#[test]
fn random_bodies_in_recycled_storage_are_the_same() {
    let mut storage: Option<Body> = None;
    let mut allocated = 0;
    for seed in 0..SEEDS / 3 {
        let fresh = generate(seed, None, true).1.unwrap();
        let recycled = generate(seed, storage.take(), true).1.unwrap();
        assert!(fresh == recycled, "seed {seed}");
        assert!(recycled.allocated_bytes() >= allocated, "seed {seed}");
        assert!(recycled.table_bytes() <= recycled.allocated_bytes());
        allocated = recycled.allocated_bytes();
        storage = Some(recycled);
    }
    // After many bodies the storage has stopped growing: one more pass
    // over the same seeds allocates nothing.
    for seed in 0..SEEDS / 3 {
        let recycled = generate(seed, storage.take(), true).1.unwrap();
        assert_eq!(recycled.allocated_bytes(), allocated, "seed {seed}");
        storage = Some(recycled);
    }
}

/// A random instruction of which `wanted` holds.
fn find(body: &Body, rng: &mut Rng, wanted: impl Fn(usize) -> bool) -> Option<usize> {
    let all: Vec<usize> = (0..body.insts.len()).filter(|&index| wanted(index)).collect();
    if all.is_empty() { None } else { Some(rng.pick(&all)) }
}

const MUTATIONS: usize = 26;

/// Damages `body` in way `which` and says what `verify` must report, or
/// returns `None` if the body has nothing of that kind to damage.
#[allow(clippy::too_many_lines)]
fn mutate(which: usize, body: &mut Body, m: &mut Module, rng: &mut Rng) -> Option<IrErrorKind> {
    use IrErrorKind::{BadBlock, InvalidOperand, UseFactsMismatch};
    let count = body.insts.len();
    let past_end = Value::inst(InstId::new(count as u32)).unwrap();
    let binary = |body: &Body, rng: &mut Rng| find(body, rng, |i| body.insts[i].op.is_binary());
    let block = rng.below(body.blocks.len());
    match which {
        // §5.6: the recorded facts are exactly the recomputed ones.
        0 => {
            let index = rng.below(count);
            let raw = body.uses[index].raw() ^ (1 << rng.below(32));
            body.uses[index] = UseFacts::from_raw(raw);
            Some(UseFactsMismatch)
        }
        1 => {
            let index = rng.below(body.arg_uses.len().max(1));
            let facts = body.arg_uses.get_mut(index)?;
            *facts = UseFacts::from_raw(facts.raw() ^ (1 << rng.below(31)));
            Some(UseFactsMismatch)
        }
        2 => {
            body.uses.pop();
            Some(UseFactsMismatch)
        }
        3 => {
            body.arg_uses.push(UseFacts::NONE);
            Some(UseFactsMismatch)
        }
        // An operand replaced by another value of its type: well typed,
        // and the facts of the old operand no longer hold.
        4 => {
            let index = find(body, rng, |i| {
                let inst = &body.insts[i];
                let named = matches!(inst.ops[0].kind(), ValueKind::Inst(_) | ValueKind::Arg(_));
                inst.op.is_binary() && named
            })?;
            let zero = m.consts.zero(&m.types, body.insts[index].ty).unwrap();
            body.insts[index].ops[0] = Value::constant(zero).unwrap();
            Some(UseFactsMismatch)
        }
        // §5.3: an operand names something that exists.
        5 => {
            let index = binary(body, rng)?;
            body.insts[index].ops[rng.below(2)] = past_end;
            Some(InvalidOperand)
        }
        6 => {
            let index = binary(body, rng)?;
            body.insts[index].ops[rng.below(2)] = Value::arg(body.arg_count).unwrap();
            Some(InvalidOperand)
        }
        7 => {
            let index = binary(body, rng)?;
            body.insts[index].ops[rng.below(2)] = Value::NONE;
            Some(InvalidOperand)
        }
        8 => {
            let index = rng.below(body.vals.len().max(1));
            *body.vals.get_mut(index)? = past_end;
            Some(InvalidOperand)
        }
        9 => {
            let index = binary(body, rng)?;
            body.insts[index].ops[0] = Value::slot(SlotId::new(body.slots.len() as u32)).unwrap();
            Some(InvalidOperand)
        }
        // §5.1: blocks tile the instructions, phis first, one terminator
        // last, in layout order.
        10 => {
            let last = body.blocks[block].end - 1;
            body.insts[last as usize].op = Opcode::Add;
            Some(BadBlock)
        }
        11 => {
            let data = body.blocks[block];
            if data.end - data.first < 2 {
                return None;
            }
            let index = data.first as usize + rng.below((data.end - data.first - 1) as usize);
            body.insts[index].op = Opcode::Unreachable;
            Some(BadBlock)
        }
        12 => {
            let other = rng.below(body.layout.len());
            if other == block {
                return None;
            }
            body.layout.swap(block, other);
            Some(BadBlock)
        }
        13 => {
            body.blocks[block].phis += 1;
            Some(BadBlock)
        }
        14 => {
            body.blocks[block].phis = body.blocks[block].phis.checked_sub(1)?;
            Some(BadBlock)
        }
        15 => {
            body.blocks[block].end -= 1;
            Some(BadBlock)
        }
        16 => {
            body.blocks.push(BlockData { first: UNSET, end: UNSET, phis: 0 });
            Some(BadBlock)
        }
        // §5.4: a phi has one entry for each edge into its block.
        17 => {
            let index = find(body, rng, |i| {
                body.insts[i].op == Opcode::Phi && body.insts[i].ops[1].bits() > 0
            })?;
            let entries = body.insts[index].ops[1].bits();
            body.insts[index].ops[1] = word(entries - 1);
            Some(BadBlock)
        }
        18 => {
            let index = find(body, rng, |i| {
                body.insts[i].op == Opcode::Phi && body.insts[i].ops[1].bits() > 0
            })?;
            let [_, entries, blocks] = body.insts[index].ops;
            let at = blocks.bits() as usize + rng.below(entries.bits() as usize);
            // A block with a phi has a predecessor, so there are two blocks.
            body.words[at] = (body.words[at] + 1) % body.blocks.len() as u32;
            Some(BadBlock)
        }
        // Ids are in range. Instruction 0 is checked before anything
        // that could use it.
        19 => {
            body.insts[0].ty = TypeId::new(m.types.len() as u32);
            Some(InvalidOperand)
        }
        20 => {
            let index = find(body, rng, |i| body.insts[i].op == Opcode::Br)?;
            body.insts[index].ops[0] = word(body.blocks.len() as u32);
            Some(InvalidOperand)
        }
        // §5.4: the side tables are the instructions that point at them.
        21 => {
            let index = rng.below(body.calls.len().max(1));
            body.calls.get_mut(index)?.nargs += 1;
            Some(InvalidOperand)
        }
        22 => {
            let index = rng.below(body.calls.len().max(1));
            body.calls.get_mut(index)?.inst += 1;
            Some(InvalidOperand)
        }
        23 => {
            if body.calls.len() < 2 {
                return None;
            }
            let last = body.calls.len() - 1;
            body.calls.swap(0, last);
            Some(InvalidOperand)
        }
        24 => {
            let index = rng.below(body.pads.len().max(1));
            body.pads.get_mut(index)?.inst += 1;
            Some(InvalidOperand)
        }
        // §5.4: switch cases are distinct.
        _ => {
            let index = find(body, rng, |i| {
                body.insts[i].op == Opcode::Switch && body.insts[i].ops[2].bits() >= 2
            })?;
            let [_, list, cases] = body.insts[index].ops;
            let values = (list.bits() + 1 + cases.bits()) as usize;
            body.words[values + 1] = body.words[values];
            Some(InvalidOperand)
        }
    }
}

/// §5.1: `verify` names each kind of damage to a body's tables: use
/// facts (`UseFactsMismatch`), block structure (`BadBlock`), operands,
/// ids and side tables (`InvalidOperand`).
#[test]
fn damaged_bodies_do_not_verify() {
    let mut applied = [0usize; MUTATIONS];
    for seed in 0..SEEDS / 3 {
        let (mut m, body) = generate(seed, None, true);
        let body = body.unwrap();
        let mut rng = Rng(!seed);
        for (which, applied) in applied.iter_mut().enumerate() {
            let mut damaged = body.clone();
            let Some(expected) = mutate(which, &mut damaged, &mut m, &mut rng) else { continue };
            *applied += 1;
            let found = damaged.verify(&m).map_err(|error| error.kind);
            assert_eq!(found, Err(expected), "seed {seed}, mutation {which}");
        }
        // The module only gained constants: the body itself still verifies.
        body.verify(&m).unwrap();
    }
    assert!(applied.iter().all(|&times| times >= 25), "{applied:?}");
}
