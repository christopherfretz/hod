//! Programs that are well formed and mostly wrong (`spec/interp.md` §1,
//! §2): whatever a body that `Body::verify` accepts does, the interpreter
//! answers with a result, a trap or "unsupported". It does not panic, and
//! it does not call the body invalid.
//!
//! The bodies here are random and unguarded: divisors may be zero, shift
//! amounts anything, pointers may be integers, addresses of functions or
//! null, values may be used where their definitions did not run, calls
//! may recurse without end or go through a pointer with the wrong type.

mod common;

use std::collections::BTreeMap;

use common::{DOUBLE, FLOAT, Fx, I1, I8, I16, I32, I64, I128, PTR, VOID, al, icall};
use hod_interp::{Config, Stop, Val};
use hod_ir::{
    BlockId, CallDesc, FloatPredicate, FunctionBuilder, IntPredicate, IrResult, Module, Opcode,
    SymId, TypeId, Value, VariableDesc,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

const INTS: [TypeId; 6] = [I1, I8, I16, I32, I64, I128];

/// What a body under construction can use: every value made so far, by
/// type, whether or not its definition will have run.
struct Wild<'a> {
    rng: &'a mut Rng,
    values: Vec<(TypeId, Value)>,
    /// The functions that may be called: symbol, return type, parameters.
    callees: &'a [(SymId, TypeId, Vec<TypeId>)],
    globals: &'a [SymId],
}

impl Wild<'_> {
    /// A value of `ty`: one made earlier, or a constant.
    fn value(&mut self, m: &mut Module, ty: TypeId) -> IrResult<Value> {
        let of_type: Vec<Value> =
            self.values.iter().filter(|(t, _)| *t == ty).map(|&(_, value)| value).collect();
        if !of_type.is_empty() && self.rng.below(4) != 0 {
            return Ok(self.rng.pick(&of_type));
        }
        let edge = self.rng.pick(&[0u64, 1, 2, 7, 8, 63, 64, 127, 128, u64::MAX, 1 << 63]);
        let constant = if ty == PTR {
            match self.rng.below(4) {
                0 => m.consts.null(&m.types, PTR)?,
                1 => m.consts.addr(self.rng.pick(self.globals), edge as i64 % 40)?,
                2 => m.consts.addr(self.callees[self.rng.below(self.callees.len())].0, 0)?,
                _ => return Value::sym(self.rng.pick(self.globals)),
            }
        } else if ty == DOUBLE {
            m.consts.f64(self.rng.pick(&[
                0.0,
                -0.0,
                1.5,
                -3.0e300,
                f64::INFINITY,
                f64::NAN,
                1e19,
            ]))?
        } else if ty == FLOAT {
            m.consts.f32(self.rng.pick(&[0.0, -1.0, 3.0e38, f32::INFINITY, f32::NAN, 5e9]))?
        } else {
            m.consts.int_from_words(&m.types, ty, &[edge, self.rng.next() & edge])?
        };
        Value::constant(constant)
    }

    fn keep(&mut self, ty: TypeId, value: Value) {
        self.values.push((ty, value));
    }

    /// One random instruction that is not a terminator.
    #[allow(clippy::too_many_lines)]
    fn inst(&mut self, m: &mut Module, b: &mut FunctionBuilder) -> IrResult<()> {
        let int = self.rng.pick(&INTS);
        let float = self.rng.pick(&[FLOAT, DOUBLE]);
        match self.rng.below(22) {
            0..4 => {
                let op = self.rng.pick(&[
                    Opcode::Add,
                    Opcode::Sub,
                    Opcode::Mul,
                    Opcode::UDiv,
                    Opcode::SDiv,
                    Opcode::URem,
                    Opcode::SRem,
                    Opcode::Shl,
                    Opcode::LShr,
                    Opcode::AShr,
                    Opcode::And,
                    Opcode::Or,
                    Opcode::Xor,
                ]);
                let (lhs, rhs) = (self.value(m, int)?, self.value(m, int)?);
                let value = b.binary(op, int, lhs, rhs)?;
                self.keep(int, value);
            }
            4 => {
                let op = self.rng.pick(&[
                    Opcode::FAdd,
                    Opcode::FSub,
                    Opcode::FMul,
                    Opcode::FDiv,
                    Opcode::FRem,
                ]);
                let (lhs, rhs) = (self.value(m, float)?, self.value(m, float)?);
                let value = b.binary(op, float, lhs, rhs)?;
                self.keep(float, value);
            }
            5 => {
                let (lhs, rhs) = (self.value(m, int)?, self.value(m, int)?);
                let value = b.icmp(self.rng.pick(IntPredicate::ALL), I1, lhs, rhs)?;
                self.keep(I1, value);
            }
            6 => {
                let (lhs, rhs) = (self.value(m, float)?, self.value(m, float)?);
                let value = b.fcmp(self.rng.pick(FloatPredicate::ALL), I1, lhs, rhs)?;
                self.keep(I1, value);
            }
            7 => {
                // Any integer to any other.
                let to = self.rng.pick(&INTS);
                let (from_bits, to_bits) = (m.types.size_of(int), m.types.size_of(to));
                let arg = self.value(m, int)?;
                if int != to && (int == I1 || to == I1 || from_bits != to_bits) {
                    let wider = to == I128
                        || (int != I128 && (int == I1 || (to != I1 && to_bits > from_bits)));
                    let op = if !wider {
                        Opcode::Trunc
                    } else if self.rng.below(2) == 0 {
                        Opcode::ZExt
                    } else {
                        Opcode::SExt
                    };
                    let value = b.cast(op, to, arg)?;
                    self.keep(to, value);
                }
            }
            8 => {
                // Floats to integers and back, in range or not.
                let arg = self.value(m, float)?;
                let op = self.rng.pick(&[Opcode::FPToSI, Opcode::FPToUI]);
                let value = b.cast(op, int, arg)?;
                self.keep(int, value);
                let arg = self.value(m, int)?;
                let op = self.rng.pick(&[Opcode::SIToFP, Opcode::UIToFP]);
                let value = b.cast(op, float, arg)?;
                self.keep(float, value);
            }
            9 => {
                let arg = self.value(m, PTR)?;
                let value = b.cast(Opcode::PtrToInt, int, arg)?;
                self.keep(int, value);
            }
            10 => {
                let arg = self.value(m, int)?;
                let value = b.cast(Opcode::IntToPtr, PTR, arg)?;
                self.keep(PTR, value);
            }
            11 | 12 => {
                let ty = self.rng.pick(&[I1, I8, I16, I32, I64, I128, FLOAT, DOUBLE, PTR]);
                let ptr = self.value(m, PTR)?;
                let value = b.load(ty, ptr, al(self.rng.pick(&[1, 1, 2, 4, 8, 16])))?;
                self.keep(ty, value);
            }
            13 | 14 => {
                let ty = self.rng.pick(&[I1, I8, I16, I32, I64, I128, FLOAT, DOUBLE, PTR]);
                let (value, ptr) = (self.value(m, ty)?, self.value(m, PTR)?);
                b.store(value, ptr, al(self.rng.pick(&[1, 1, 2, 4, 8, 16])))?;
            }
            15 => {
                let base = self.value(m, PTR)?;
                let index = self.value(m, int)?;
                let offset = self.rng.pick(&[0i64, 1, 8, 15, 16, -8, 4096]);
                let indices = if int == I128 || self.rng.below(2) == 0 {
                    vec![]
                } else {
                    vec![(index, self.rng.pick(&[1u64, 4, 8, 1 << 40]))]
                };
                let value = b.ptr_add(PTR, base, offset, &indices)?;
                self.keep(PTR, value);
            }
            16 => {
                let ty = self.rng.pick(&[I64, PTR, DOUBLE]);
                let cond = self.value(m, I1)?;
                let (yes, no) = (self.value(m, ty)?, self.value(m, ty)?);
                let value = b.select(ty, cond, yes, no)?;
                self.keep(ty, value);
            }
            17 => {
                let count = self.value(m, I64)?;
                let value = b.alloca(PTR, count, self.rng.pick(&[1, 8, 1 << 20]), 8)?;
                self.keep(PTR, value);
            }
            18 => {
                // A direct call, or one through any pointer at all.
                let (sym, ret, params) = self.callees[self.rng.below(self.callees.len())].clone();
                let fn_ty = m.types.function(ret, &params, false)?;
                let mut args = Vec::new();
                for &param in &params {
                    args.push(self.value(m, param)?);
                }
                let callee =
                    if self.rng.below(3) == 0 { self.value(m, PTR)? } else { Value::sym(sym)? };
                let value = b.call(ret, &CallDesc::new(callee, fn_ty, &args))?;
                if ret != VOID {
                    self.keep(ret, value);
                }
            }
            19 => {
                let ty = self.rng.pick(&[I8, I16, I32, I64]);
                let bits = m.types.size_of(ty) * 8;
                let arg = self.value(m, ty)?;
                // The flag is an immediate argument: a constant.
                let flag = Value::constant(m.consts.int(&m.types, I1, self.rng.next() & 1)?)?;
                let name = self.rng.pick(&["ctlz", "cttz", "abs"]);
                let name = format!("llvm.{name}.i{bits}");
                let value = icall(m, b, &name, ty, &[ty, I1], &[arg, flag])?;
                self.keep(ty, value);
            }
            20 => {
                let (dst, src) = (self.value(m, PTR)?, self.value(m, PTR)?);
                let len = self.value(m, I64)?;
                let no = Value::constant(m.consts.int(&m.types, I1, self.rng.next() & 1)?)?;
                let name = self.rng.pick(&["llvm.memcpy.p0.p0.i64", "llvm.memmove.p0.p0.i64"]);
                icall(m, b, name, VOID, &[PTR, PTR, I64, I1], &[dst, src, len, no])?;
            }
            _ => {
                let arg = self.value(m, int)?;
                let value = b.freeze(int, arg)?;
                self.keep(int, value);
            }
        }
        Ok(())
    }
}

/// Where a block goes when it ends.
#[derive(Clone)]
enum End {
    Br(usize),
    CondBr(usize, usize),
    Switch(usize, Vec<(u64, usize)>),
    Ret,
    Unreachable,
}

/// A random body for the function open in `b`, whose entry block is open.
fn wild_body(
    m: &mut Module,
    b: &mut FunctionBuilder,
    wild: &mut Wild<'_>,
    ret: TypeId,
) -> IrResult<()> {
    let count = 1 + wild.rng.below(5);
    let mut blocks: Vec<BlockId> = vec![b.current_block().expect("the entry block is open")];
    for _ in 1..count {
        blocks.push(b.create_block()?);
    }
    // No block branches to the entry block.
    let ends: Vec<End> = (0..count)
        .map(|_| {
            let rng = &mut *wild.rng;
            let mut to = || if count == 1 { None } else { Some(1 + rng.below(count - 1)) };
            match (to(), to(), to()) {
                (Some(first), Some(second), Some(third)) => match rng.below(8) {
                    0..3 => End::Br(first),
                    3..5 => End::CondBr(first, second),
                    5 => End::Switch(first, vec![(0, second), (1, third), (7, second)]),
                    6 => End::Unreachable,
                    _ => End::Ret,
                },
                _ => End::Ret,
            }
        })
        .collect();
    // The values of the entry block, which dominates every other: a
    // block uses those and its own, so that each definition dominates
    // its uses (`spec/ir.md` §5.8).
    let mut from_entry = wild.values.len();
    for (at, end) in ends.iter().enumerate() {
        if at != 0 {
            wild.values.truncate(from_entry);
            b.begin_block(blocks[at])?;
            // One `phi` entry for each edge that comes here.
            let mut edges = Vec::new();
            for (from, end) in ends.iter().enumerate() {
                let targets: Vec<usize> = match end {
                    End::Br(to) => vec![*to],
                    End::CondBr(yes, no) => vec![*yes, *no],
                    End::Switch(default, cases) => {
                        std::iter::once(*default).chain(cases.iter().map(|case| case.1)).collect()
                    }
                    End::Ret | End::Unreachable => vec![],
                };
                edges.extend(targets.into_iter().filter(|&to| to == at).map(|_| from));
            }
            if !edges.is_empty() && wild.rng.below(2) == 0 {
                let ty = wild.rng.pick(&[I64, PTR, I1]);
                // Edges from one block carry one value.
                let mut of_block = BTreeMap::new();
                let mut incoming = Vec::new();
                for from in edges {
                    let value = match of_block.get(&from) {
                        Some(&value) => value,
                        None => wild.value(m, ty)?,
                    };
                    of_block.insert(from, value);
                    incoming.push((value, blocks[from]));
                }
                let value = b.phi(ty, &incoming)?;
                wild.keep(ty, value);
            }
        }
        for _ in 0..wild.rng.below(8) {
            wild.inst(m, b)?;
        }
        if at == 0 {
            from_entry = wild.values.len();
        }
        match end {
            End::Br(to) => {
                b.br(blocks[*to])?;
            }
            End::CondBr(yes, no) => {
                let cond = wild.value(m, I1)?;
                b.cond_br(cond, blocks[*yes], blocks[*no])?;
            }
            End::Switch(default, cases) => {
                let on = wild.value(m, I32)?;
                let mut list = Vec::new();
                for &(key, to) in cases {
                    list.push((m.consts.int(&m.types, I32, key)?, blocks[to]));
                }
                b.switch(on, blocks[*default], &list)?;
            }
            End::Ret if ret == VOID => {
                b.ret(None)?;
            }
            End::Ret => {
                let value = wild.value(m, ret)?;
                b.ret(Some(value))?;
            }
            End::Unreachable => {
                b.unreachable()?;
            }
        }
    }
    Ok(())
}

/// §1, §2: a thousand unguarded programs, each run to whatever end it has.
#[test]
fn any_verified_body_gets_an_answer() {
    let mut ends: BTreeMap<String, u32> = BTreeMap::new();
    let mut refused = 0;
    for seed in 0..1000u64 {
        let mut rng = Rng(seed);
        let mut fx = Fx::new();
        let word = fx.m.add_variable(b"word", &VariableDesc::new(I64)).unwrap();
        let init = fx.m.consts.bytes(&mut fx.m.types, &[1, 2, 3, 4, 5, 6, 7, 8, 9]).unwrap();
        let ty = fx.m.consts.ty(init);
        let desc = VariableDesc { init: Some(init), constant: true, ..VariableDesc::new(ty) };
        let table = fx.m.add_variable(b"table", &desc).unwrap();
        let zero = fx.m.consts.zero(&fx.m.types, I64).unwrap();
        let desc = VariableDesc { init: Some(zero), ..VariableDesc::new(I64) };
        let cell = fx.m.add_variable(b"cell", &desc).unwrap();
        let globals = [word, table, cell];
        let signatures = [
            ("f0", I64, vec![I64, PTR, DOUBLE]),
            ("f1", I32, vec![I32]),
            ("f2", VOID, vec![PTR, I8]),
        ];
        let mut callees: Vec<(SymId, TypeId, Vec<TypeId>)> = signatures
            .iter()
            .map(|(name, ret, params)| (fx.declare(name, *ret, params), *ret, params.clone()))
            .collect();
        callees.push((fx.std.sink, VOID, vec![I64]));
        callees.push((fx.std.source, I64, vec![]));
        let mut verified = true;
        for (sym, ret, params) in callees[..3].iter().cloned() {
            let mut b = FunctionBuilder::new(&fx.m, sym, fx.m.symbol(sym).ty()).unwrap();
            let entry = b.create_block().unwrap();
            b.begin_block(entry).unwrap();
            let mut wild =
                Wild { rng: &mut rng, values: Vec::new(), callees: &callees, globals: &globals };
            for (index, &param) in params.iter().enumerate() {
                wild.keep(param, b.arg(index as u32).unwrap());
            }
            let slot = b.add_slot(24, 8).unwrap();
            wild.keep(PTR, b.slot(slot).unwrap());
            wild_body(&mut fx.m, &mut b, &mut wild, ret).unwrap();
            let body = b.finish().unwrap();
            verified &= body.verify(&fx.m).is_ok();
            fx.m.set_has_body(sym).unwrap();
            fx.bodies.push(body);
        }
        if !verified {
            refused += 1;
            continue;
        }
        let config =
            Config { fuel: 3000, max_call_depth: 40, max_memory: 1 << 22, ..Config::default() };
        let args = [Val::i64(rng.next() % 9), Val::Ptr(0), Val::f64(2.5)];
        let outcome = fx.run_with(callees[0].0, &args, config);
        let end = match &outcome.result {
            Ok(_) => "result".to_owned(),
            Err(Stop::Trap(trap)) => trap.name().to_owned(),
            Err(Stop::Unsupported(reason)) => format!("unsupported {reason}"),
            Err(Stop::Invalid(what)) => panic!("seed {seed}: a verified body is invalid: {what}"),
        };
        assert!(outcome.steps <= config.fuel);
        assert_eq!(outcome.stopped_at.is_some(), outcome.result.is_err(), "seed {seed}");
        *ends.entry(end).or_default() += 1;
    }
    println!("{ends:#?}\nrefused by verify: {refused}");
    // The programs are not all alike: many kinds of end occur.
    assert_eq!(refused, 0, "the bodies are meant to verify");
    assert!(ends.len() >= 12, "only {} kinds of end", ends.len());
    assert!(ends.get("result").copied().unwrap_or(0) >= 20);
}
