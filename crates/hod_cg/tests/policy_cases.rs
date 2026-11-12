//! `spec/codegen.md` §12: the cases the design was reviewed against,
//! one named test each, on hand-built bodies. Wherever the interpreter
//! can run the program it is the oracle, under every stress mode; the
//! rest check the records the case is about.

// Builder code names the module `m`, the builder `b`, operands `a`, `x`.
#![allow(clippy::many_single_char_names)]

mod common;

use std::sync::atomic::Ordering;

use common::exec;
use common::harness::{MODES, compile_all};
use common::isa;
use common::target::Tm;
use hod_cg::{
    CgError, Chunk, FuncRecord, LIMIT_FRAME_SIZE, LIMIT_FUNCTION_SIZE, Options, Scratch, Stress,
    compile_function,
};
use hod_interp::host::{self, Standard};
use hod_interp::{Interp, Program, Val};
use hod_ir::{
    BlockId, Body, CallDesc, FunctionBuilder, FunctionDesc, IntPredicate, IrResult, MemFlags,
    Module, Opcode, SymId, TailKind, TargetSpec, TypeId, Value,
};

const I1: TypeId = TypeId::I1;
const I64: TypeId = TypeId::I64;
const DOUBLE: TypeId = TypeId::DOUBLE;
const PTR: TypeId = TypeId::PTR;

/// A module with the standard host functions, and its bodies.
struct Fx {
    m: Module,
    bodies: Vec<Body>,
    std: Standard,
}

impl Fx {
    fn new() -> Fx {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let std = host::declare(&mut m).unwrap();
        Fx { m, bodies: Vec::new(), std }
    }

    fn declare(&mut self, name: &str, params: &[TypeId]) -> SymId {
        let ty = self.m.types.function(I64, params, false).unwrap();
        self.m.add_function(name.as_bytes(), &FunctionDesc::new(ty)).unwrap()
    }

    /// Gives `sym` a body: `build` is called with the entry block open.
    fn define(
        &mut self,
        sym: SymId,
        build: impl FnOnce(&mut Module, &Standard, &mut FunctionBuilder) -> IrResult<()>,
    ) {
        let ty = self.m.symbol(sym).ty();
        let mut b = FunctionBuilder::new(&self.m, sym, ty).unwrap();
        let entry = b.create_block().unwrap();
        b.begin_block(entry).unwrap();
        let std = self.std;
        build(&mut self.m, &std, &mut b).unwrap();
        let body = b.finish().unwrap();
        body.verify(&self.m).unwrap();
        self.m.set_has_body(sym).unwrap();
        self.bodies.push(body);
    }

    /// Declares and defines `i64 name(i64 × params)`.
    fn func(
        &mut self,
        name: &str,
        params: usize,
        build: impl FnOnce(&mut Module, &Standard, &mut FunctionBuilder) -> IrResult<()>,
    ) -> SymId {
        let sym = self.declare(name, &vec![I64; params]);
        self.define(sym, build);
        sym
    }

    /// Runs `entry` in the interpreter, then compiled under every stress
    /// mode, and asserts the results and host-call traces are equal.
    /// Returns the result.
    #[track_caller]
    fn check(&self, entry: SymId, args: &[u64]) -> u64 {
        let program = Program::with_bodies(&self.m, &self.bodies).unwrap();
        let mut interp = Interp::new(&program);
        host::register(&mut interp);
        let vals: Vec<Val> = args.iter().map(|&arg| Val::i64(arg)).collect();
        let outcome = interp.run(entry, &vals);
        let Ok(Some(Val::Int { bits: 64, value })) = outcome.result else {
            panic!("the interpreter did not return an i64: {:?}", outcome.result);
        };
        let expected = value as u64;
        for (name, stress) in MODES {
            let ran = self.run(entry, args, stress).unwrap_or_else(|what| panic!("{name}: {what}"));
            assert_eq!(ran.ret[0], expected, "mode {name}: the result");
            assert_eq!(ran.trace, outcome.trace, "mode {name}: the host calls");
        }
        expected
    }

    /// Compiles every body under `stress` and runs `entry`.
    fn run(&self, entry: SymId, args: &[u64], stress: Stress) -> Result<exec::Ran, String> {
        let chunk = self.compile(stress).map_err(|error| error.to_string())?;
        exec::run(&self.m, &chunk, entry, args)
    }

    fn compile(&self, stress: Stress) -> Result<Chunk, CgError> {
        let mut chunk = Chunk::new();
        compile_all(
            &Tm::default(),
            &self.m,
            &self.bodies,
            Options { stress, ..Options::default() },
            &mut Scratch::default(),
            &mut chunk,
        )?;
        Ok(chunk)
    }
}

fn int(m: &mut Module, value: u64) -> Value {
    Value::constant(m.consts.int(&m.types, I64, value).unwrap()).unwrap()
}

fn al8() -> MemFlags {
    MemFlags::aligned(8).unwrap()
}

/// A direct call of `callee` with `args`.
fn call(m: &Module, b: &mut FunctionBuilder, callee: SymId, args: &[Value]) -> IrResult<Value> {
    let fn_ty = m.symbol(callee).ty();
    let hod_ir::TypeKind::Function { ret, .. } = m.types.kind(fn_ty) else {
        unreachable!("a function symbol has a function type")
    };
    b.call(ret, &CallDesc::new(Value::sym(callee)?, fn_ty, args))
}

fn sink(m: &Module, std: &Standard, b: &mut FunctionBuilder, value: Value) -> IrResult<()> {
    call(m, b, std.sink, &[value]).map(|_| ())
}

/// The record of `chunk`'s function `index` and its rows, each table
/// sliced to the function's range: what must not depend on what else
/// the chunk holds.
#[derive(PartialEq, Debug)]
struct Rows<'c> {
    sym: SymId,
    align: u32,
    frame: hod_cg::FrameDesc,
    has_pads: bool,
    code: &'c [u8],
    relocs: &'c [hod_cg::Reloc],
    cfi: &'c [hod_cg::CfiRow],
    call_sites: &'c [hod_cg::CallSiteRow],
    pads: &'c [hod_cg::PadRow],
    lines: &'c [hod_cg::LineRow],
    islands: &'c [hod_cg::IslandRow],
    block_offsets: &'c [hod_cg::BlockOffset],
}

fn rows(chunk: &Chunk, index: usize) -> Rows<'_> {
    fn of<'t, T>(table: &'t [T], range: &std::ops::Range<u32>) -> &'t [T] {
        &table[range.start as usize..range.end as usize]
    }
    let record: &FuncRecord = &chunk.funcs[index];
    Rows {
        sym: record.sym,
        align: record.align,
        frame: record.frame,
        has_pads: record.has_pads,
        code: of(&chunk.code, &record.code),
        relocs: of(&chunk.relocs, &record.relocs),
        cfi: of(&chunk.cfi, &record.cfi),
        call_sites: of(&chunk.call_sites, &record.call_sites),
        pads: of(&chunk.pads, &record.pads),
        lines: of(&chunk.lines, &record.lines),
        islands: of(&chunk.islands, &record.islands),
        block_offsets: of(&chunk.block_offsets, &record.block_offsets),
    }
}

/// §4.2: instructions whose results have no use, and whose operands are
/// register values used nowhere else, are left out; the operands'
/// registers are not lost. More of them than the pool has registers.
#[test]
fn dead_instruction_with_register_operands() {
    let mut fx = Fx::new();
    let f = fx.func("f", 2, |m, std, b| {
        let (a, c) = (b.arg(0)?, b.arg(1)?);
        let mut keep = Vec::new();
        for k in 0..12 {
            let k = int(m, 3 + k);
            let x = b.binary(Opcode::Mul, I64, a, k)?;
            let y = b.binary(Opcode::Add, I64, c, k)?;
            // Dead: `y` has no other use; `x` has one later.
            b.binary(Opcode::Sub, I64, x, y)?;
            keep.push(x);
        }
        let mut sum = c;
        for x in keep {
            sum = b.binary(Opcode::Xor, I64, sum, x)?;
        }
        sink(m, std, b, sum)?;
        b.ret(Some(sum))?;
        Ok(())
    });
    fx.check(f, &[0x1234_5678, 99]);
}

/// §4.2: a dead instruction whose operand is a deferred compare, alone
/// and with a later live use of the compare; longer than a deferral run.
#[test]
fn dead_instruction_with_deferred_operand() {
    let mut fx = Fx::new();
    let f = fx.func("f", 2, |m, std, b| {
        let (a, c) = (b.arg(0)?, b.arg(1)?);
        for k in 0..12 {
            let k = int(m, k);
            let x = b.binary(Opcode::Add, I64, a, k)?;
            let cmp = b.icmp(IntPredicate::Slt, I1, x, c)?;
            // Dead: the compare's only use.
            b.select(I64, cmp, a, c)?;
        }
        let cmp = b.icmp(IntPredicate::Ult, I1, a, c)?;
        b.cast(Opcode::ZExt, I64, cmp)?;
        let (yes, no) = (b.create_block()?, b.create_block()?);
        b.cond_br(cmp, yes, no)?;
        b.begin_block(yes)?;
        sink(m, std, b, a)?;
        b.ret(Some(a))?;
        b.begin_block(no)?;
        sink(m, std, b, c)?;
        b.ret(Some(c))?;
        Ok(())
    });
    assert_eq!(fx.check(f, &[3, 10]), 3);
    assert_eq!(fx.check(f, &[10, 3]), 3);
}

/// §4.6: a deferred compare that is not the first operand of the
/// instruction that asks for it, itself the end of a deferred run, with
/// registers already taken by the other operands; under the minimum
/// pool. (Seed 14001094 of the option set `no-globals` found it: the
/// test target asked for the other operands first and ran out of
/// registers.)
#[test]
fn deferred_operand_that_is_not_the_first() {
    let mut fx = Fx::new();
    let f = fx.func("f", 2, |m, std, b| {
        let (a, c) = (b.arg(0)?, b.arg(1)?);
        let zero = int(m, 0);
        let first = b.icmp(IntPredicate::Ult, I1, a, c)?;
        let other = b.icmp(IntPredicate::Ne, I1, c, zero)?;
        sink(m, std, b, a)?;
        let narrow = b.cast(Opcode::Trunc, TypeId::I32, c)?;
        let wide = b.cast(Opcode::Trunc, TypeId::I32, a)?;
        let last = b.icmp(IntPredicate::Sgt, I1, wide, narrow)?;
        let picked = b.select(I1, other, first, last)?;
        let anded = b.icmp(IntPredicate::Eq, I1, a, c)?;
        let both = b.binary(Opcode::Or, I1, picked, anded)?;
        let r = b.cast(Opcode::ZExt, I64, both)?;
        b.ret(Some(r))?;
        Ok(())
    });
    assert_eq!(fx.check(f, &[5, 0]), 1);
    assert_eq!(fx.check(f, &[5, 3]), 0);
    assert_eq!(fx.check(f, &[3, 5]), 1);
    assert_eq!(fx.check(f, &[7, 7]), 1);
}

/// Builds a loop of `n` iterations whose `phi`s rotate `values` by one
/// each time around (`header` → `body` → `header`), and returns
/// `Σ value_i × 10^i` at the exit.
fn rotating_loop(
    m: &mut Module,
    std: &Standard,
    b: &mut FunctionBuilder,
    values: &[Value],
    n: u64,
) -> IrResult<()> {
    let entry = b.current_block().unwrap();
    let (header, body, exit) = (b.create_block()?, b.create_block()?, b.create_block()?);
    let (zero, one, limit) = (int(m, 0), int(m, 1), int(m, n));
    b.br(header)?;
    b.begin_block(header)?;
    let i = b.phi(I64, &[(zero, entry)])?;
    let phis: Vec<Value> =
        values.iter().map(|&value| b.phi(I64, &[(value, entry)])).collect::<IrResult<_>>()?;
    let more = b.icmp(IntPredicate::Ult, I1, i, limit)?;
    b.cond_br(more, body, exit)?;
    b.begin_block(body)?;
    sink(m, std, b, phis[0])?;
    let next = b.binary(Opcode::Add, I64, i, one)?;
    b.add_incoming(i, next, body)?;
    for (index, &phi) in phis.iter().enumerate() {
        b.add_incoming(phi, phis[(index + 1) % phis.len()], body)?;
    }
    b.br(header)?;
    b.begin_block(exit)?;
    let mut sum = zero;
    let mut scale = 1;
    for &phi in &phis {
        let k = int(m, scale);
        let term = b.binary(Opcode::Mul, I64, phi, k)?;
        sum = b.binary(Opcode::Add, I64, sum, term)?;
        scale *= 10;
    }
    b.ret(Some(sum))?;
    Ok(())
}

/// §4.5: two `phi`s that swap each other, and three that rotate: the
/// incoming copies form a cycle.
#[test]
fn phi_swap() {
    let mut fx = Fx::new();
    let two = fx.func("two", 2, |m, std, b| {
        let values = [b.arg(0)?, b.arg(1)?];
        rotating_loop(m, std, b, &values, 3)
    });
    let three = fx.func("three", 3, |m, std, b| {
        let values = [b.arg(0)?, b.arg(1)?, b.arg(2)?];
        rotating_loop(m, std, b, &values, 4)
    });
    assert_eq!(fx.check(two, &[1, 2]), 12);
    assert_eq!(fx.check(three, &[1, 2, 3]), 132);
}

/// §4.5: a block that is its own predecessor, with a `phi` whose value
/// on the back edge is itself and one that swaps with another.
#[test]
fn phi_self_loop() {
    let mut fx = Fx::new();
    let f = fx.func("f", 2, |m, std, b| {
        let entry = b.current_block().unwrap();
        let (looping, exit) = (b.create_block()?, b.create_block()?);
        let (zero, one, five) = (int(m, 0), int(m, 1), int(m, 5));
        b.br(looping)?;
        b.begin_block(looping)?;
        let i = b.phi(I64, &[(zero, entry)])?;
        let same = b.phi(I64, &[(b.arg(0)?, entry)])?;
        let x = b.phi(I64, &[(b.arg(1)?, entry)])?;
        let y = b.phi(I64, &[(five, entry)])?;
        sink(m, std, b, x)?;
        let next = b.binary(Opcode::Add, I64, i, one)?;
        b.add_incoming(i, next, looping)?;
        b.add_incoming(same, same, looping)?;
        b.add_incoming(x, y, looping)?;
        b.add_incoming(y, x, looping)?;
        let more = b.icmp(IntPredicate::Ult, I1, next, five)?;
        b.cond_br(more, looping, exit)?;
        b.begin_block(exit)?;
        let k = int(m, 1000);
        let scaled = b.binary(Opcode::Mul, I64, same, k)?;
        let sum = b.binary(Opcode::Add, I64, scaled, x)?;
        let sum = b.binary(Opcode::Sub, I64, sum, y)?;
        b.ret(Some(sum))?;
        Ok(())
    });
    fx.check(f, &[7, 11]);
}

/// A function whose entry branches over `n` additions: its code is
/// longer than a near branch reaches once `n` is in the hundreds.
fn chain(fx: &mut Fx, name: &str, n: u64) -> SymId {
    fx.func(name, 1, |m, _, b| {
        let entry = b.current_block().unwrap();
        let (long, exit) = (b.create_block()?, b.create_block()?);
        let zero = int(m, 0);
        let a = b.arg(0)?;
        let skip = b.icmp(IntPredicate::Eq, I1, a, zero)?;
        b.cond_br(skip, exit, long)?;
        b.begin_block(long)?;
        let mut v = a;
        for index in 0..n {
            let k = int(m, index.wrapping_mul(0x9E37_79B9) | 1);
            let op = if index % 2 == 0 { Opcode::Add } else { Opcode::Xor };
            v = b.binary(op, I64, v, k)?;
        }
        b.br(exit)?;
        b.begin_block(exit)?;
        let r = b.phi(I64, &[(a, entry), (v, long)])?;
        b.ret(Some(r))?;
        Ok(())
    })
}

/// §6.3, §10: a function whose short branches do not reach is compiled
/// again in far mode, and the first attempt leaves no trace: the chunk
/// is the one far mode gives from the start.
#[test]
fn far_branch_retry_leaves_no_trace() {
    let mut fx = Fx::new();
    let small = fx.func("small", 1, |m, _, b| {
        let one = int(m, 1);
        let r = b.binary(Opcode::Add, I64, b.arg(0)?, one)?;
        b.ret(Some(r)).map(|_| ())
    });
    let big = chain(&mut fx, "big", 400);
    let tm = Tm::default();
    let mut scratch = Scratch::default();
    let mut retried = Chunk::new();
    for body in &fx.bodies {
        compile_function(&tm, &fx.m, body, &Options::default(), &mut scratch, &mut retried)
            .unwrap();
    }
    // `small` once, `big` twice.
    assert_eq!(tm.attempts.load(Ordering::Relaxed), 3);
    assert_eq!(tm.far_attempts.load(Ordering::Relaxed), 1);
    assert!(rows(&retried, 1).code.len() > isa::NEAR_REACH as usize);
    let far = fx.compile(Stress { far: true, ..Stress::default() }).unwrap();
    assert_eq!(rows(&retried, 1), rows(&far, 1));
    // `small`, compiled in near mode before it, is untouched.
    let near = fx.compile(Stress::default()).unwrap();
    assert_eq!(rows(&retried, 0), rows(&near, 0));
    assert_eq!(retried.funcs.len(), 2);
    assert_eq!(retried.code.len(), near.code.len());
    fx.check(small, &[4]);
    fx.check(big, &[0]);
    fx.check(big, &[5]);
}

/// §6.1: blocks no path from the entry reaches, one of them a
/// predecessor of a reachable `phi` and one its own predecessor, are not
/// emitted.
#[test]
fn unreachable_blocks() {
    let mut fx = Fx::new();
    let f = fx.func("f", 1, |m, std, b| {
        let entry = b.current_block().unwrap();
        let (dead, spin, join) = (b.create_block()?, b.create_block()?, b.create_block()?);
        let a = b.arg(0)?;
        b.br(join)?;
        b.begin_block(dead)?;
        let one = int(m, 1);
        let x = b.binary(Opcode::Add, I64, a, one)?;
        b.br(join)?;
        b.begin_block(spin)?;
        sink(m, std, b, a)?;
        b.br(spin)?;
        b.begin_block(join)?;
        let p = b.phi(I64, &[(a, entry), (x, dead)])?;
        sink(m, std, b, p)?;
        b.ret(Some(p))?;
        Ok(())
    });
    assert_eq!(fx.check(f, &[41]), 41);
    let chunk = fx.compile(Stress::default()).unwrap();
    // No `indirectbr`: no block offsets (§9.6).
    assert!(rows(&chunk, 0).block_offsets.is_empty());
}

/// §6.1, §9.6: blocks whose addresses are taken, one of them listed by
/// no `indirectbr` and unreachable. The interpreter does not run
/// `blockaddress`; the results are the IR's by hand.
#[test]
fn address_taken_blocks() {
    let mut fx = Fx::new();
    let sym = fx.declare("f", &[I64]);
    let mut blocks = [BlockId::new(0); 3];
    fx.define(sym, |m, _, b| {
        let (one, two, three) = (b.create_block()?, b.create_block()?, b.create_block()?);
        blocks = [one, two, three];
        let addr = |m: &mut Module, block| {
            Value::constant(m.consts.block_addr(sym, block).unwrap()).unwrap()
        };
        let (to_one, to_two, to_three) = (addr(m, one), addr(m, two), addr(m, three));
        let slot = b.add_slot(8, 8)?;
        let slot = b.slot(slot)?;
        // `three`'s address escapes, but no `indirectbr` lists it.
        b.store(to_three, slot, al8())?;
        let zero = int(m, 0);
        let pick = b.icmp(IntPredicate::Ne, I1, b.arg(0)?, zero)?;
        let to = b.select(PTR, pick, to_one, to_two)?;
        b.indirect_br(to, &[one, two])?;
        for (block, value) in [(one, 11), (two, 22), (three, 33)] {
            b.begin_block(block)?;
            let value = int(m, value);
            b.ret(Some(value))?;
        }
        Ok(())
    });
    for (name, stress) in MODES {
        assert_eq!(fx.run(sym, &[1], stress).unwrap().ret[0], 11, "{name}");
        assert_eq!(fx.run(sym, &[0], stress).unwrap().ret[0], 22, "{name}");
        let chunk = fx.compile(stress).unwrap();
        let rows = rows(&chunk, 0);
        let offset_of = |block| {
            rows.block_offsets.iter().find(|row| row.block == block).map(|row| row.offset as usize)
        };
        let [one, two, three] = blocks.map(|block| offset_of(block).expect("a block offset"));
        assert!(one != two && one < rows.code.len() && two < rows.code.len(), "{name}");
        // The unreachable block stands for the shared trap.
        assert_eq!(rows.code[three], isa::TRAP, "{name}");
    }
}

/// §7.1: a `musttail` call that passes its two stack arguments to the
/// callee swapped: the outgoing area is the incoming one.
#[test]
fn musttail_swapping_stack_arguments() {
    let mut fx = Fx::new();
    let g = fx.func("g", 5, |m, std, b| {
        let mut sum = int(m, 0);
        for (index, k) in [1, 10, 100, 1000, 10000].into_iter().enumerate() {
            let k = int(m, k);
            let term = b.binary(Opcode::Mul, I64, b.arg(index as u32)?, k)?;
            sum = b.binary(Opcode::Add, I64, sum, term)?;
        }
        sink(m, std, b, b.arg(3)?)?;
        b.ret(Some(sum))?;
        Ok(())
    });
    let f = fx.func("f", 5, |m, _, b| {
        let args = [b.arg(0)?, b.arg(1)?, b.arg(2)?, b.arg(4)?, b.arg(3)?];
        let fn_ty = m.symbol(g).ty();
        let desc =
            CallDesc { tail: TailKind::MustTail, ..CallDesc::new(Value::sym(g)?, fn_ty, &args) };
        let r = b.call(I64, &desc)?;
        b.ret(Some(r))?;
        Ok(())
    });
    let h = fx.func("h", 1, |m, _, b| {
        let x = b.arg(0)?;
        let mut args = vec![x];
        for k in 1..5 {
            let k = int(m, k);
            args.push(b.binary(Opcode::Add, I64, x, k)?);
        }
        let r = call(m, b, f, &args)?;
        b.ret(Some(r))?;
        Ok(())
    });
    // g(1, 2, 3, 5, 4) = 1 + 20 + 300 + 5000 + 40000.
    assert_eq!(fx.check(h, &[1]), 45_321);
}

/// §4.4: values live across calls, more of them than the pool holds,
/// one of them a float.
#[test]
fn values_held_across_a_call() {
    let mut fx = Fx::new();
    let f = fx.func("f", 2, |m, std, b| {
        let (a, c) = (b.arg(0)?, b.arg(1)?);
        let mut held = Vec::new();
        for k in 1..10 {
            let k = int(m, k);
            held.push(b.binary(Opcode::Mul, I64, a, k)?);
        }
        let float = b.cast(Opcode::SIToFP, DOUBLE, c)?;
        sink(m, std, b, a)?;
        let got = call(m, b, std.source, &[])?;
        sink(m, std, b, got)?;
        let back = b.cast(Opcode::FPToSI, I64, float)?;
        let mut sum = b.binary(Opcode::Add, I64, got, back)?;
        for (index, value) in held.into_iter().enumerate() {
            let op = if index % 2 == 0 { Opcode::Add } else { Opcode::Xor };
            sum = b.binary(op, I64, sum, value)?;
        }
        b.ret(Some(sum))?;
        Ok(())
    });
    fx.check(f, &[0x77, 1234]);
}

/// §12 (unit): a function compiled alone and among others gives the
/// same bytes and records.
#[test]
fn determinism_alone_and_among_others() {
    let mut fx = Fx::new();
    let target = fx.func("target", 1, |m, std, b| {
        sink(m, std, b, b.arg(0)?)?;
        let got = call(m, b, std.source, &[])?;
        let float = b.cast(Opcode::SIToFP, DOUBLE, got)?;
        let half = Value::constant(m.consts.f64(1.5).unwrap()).unwrap();
        let sum = b.binary(Opcode::FAdd, DOUBLE, float, half)?;
        let r = b.cast(Opcode::FPToSI, I64, sum)?;
        b.ret(Some(r))?;
        Ok(())
    });
    chain(&mut fx, "other", 300);
    fx.func("third", 2, |_, _, b| {
        let r = b.binary(Opcode::Mul, I64, b.arg(0)?, b.arg(1)?)?;
        b.ret(Some(r)).map(|_| ())
    });
    let tm = Tm::default();
    for (name, stress) in MODES {
        let options = Options { stress, ..Options::default() };
        let mut scratch = Scratch::default();
        let mut alone = Chunk::new();
        compile_function(&tm, &fx.m, &fx.bodies[0], &options, &mut scratch, &mut alone).unwrap();
        // The same scratch, reused after other functions, and the target
        // last in a chunk that holds them.
        let mut among = Chunk::new();
        for index in [1, 2, 0] {
            let body = &fx.bodies[index];
            compile_function(&tm, &fx.m, body, &options, &mut scratch, &mut among).unwrap();
        }
        assert_eq!(rows(&alone, 0), rows(&among, 2), "{name}");
    }
    fx.check(target, &[3]);
}

/// §1, §10: a failed compile leaves the chunk as it was, and the next
/// function compiles as if the failure had not happened. The limits a
/// compile can hit: the core's frame size, the target's frame size, the
/// reach of a far branch.
#[test]
fn an_error_leaves_the_chunk_as_it_was() {
    let mut fx = Fx::new();
    fx.func("first", 1, |m, std, b| {
        sink(m, std, b, b.arg(0)?)?;
        b.ret(Some(b.arg(0)?)).map(|_| ())
    });
    let with_slot = |size: u64| {
        move |_: &mut Module, _: &Standard, b: &mut FunctionBuilder| {
            let slot = b.add_slot(size, 8)?;
            let slot = b.slot(slot)?;
            b.store(b.arg(0)?, slot, al8())?;
            let r = b.load(I64, slot, al8())?;
            b.ret(Some(r)).map(|_| ())
        }
    };
    fx.func("core-frame", 1, with_slot(1 << 31));
    fx.func("target-frame", 1, with_slot(2 << 20));
    // About ten bytes an addition: beyond a far branch's reach.
    chain(&mut fx, "too-long", (isa::FAR_REACH / 6) as u64);
    fx.func("last", 1, |m, _, b| {
        let two = int(m, 2);
        let r = b.binary(Opcode::Mul, I64, b.arg(0)?, two)?;
        b.ret(Some(r)).map(|_| ())
    });
    let expected = [
        (1, CgError::unsupported(LIMIT_FRAME_SIZE)),
        (2, CgError::unsupported(LIMIT_FRAME_SIZE)),
        (3, CgError::unsupported(LIMIT_FUNCTION_SIZE)),
    ];
    let tm = Tm::default();
    let options = Options::default();
    let mut scratch = Scratch::default();
    let mut chunk = Chunk::new();
    compile_function(&tm, &fx.m, &fx.bodies[0], &options, &mut scratch, &mut chunk).unwrap();
    let before = chunk.clone();
    for (index, error) in expected {
        let got =
            compile_function(&tm, &fx.m, &fx.bodies[index], &options, &mut scratch, &mut chunk);
        assert_eq!(got, Err(error), "function {index}");
        assert_eq!(chunk, before, "function {index} left a trace");
    }
    compile_function(&tm, &fx.m, &fx.bodies[4], &options, &mut scratch, &mut chunk).unwrap();
    let mut clean = Chunk::new();
    for index in [0, 4] {
        compile_function(
            &tm,
            &fx.m,
            &fx.bodies[index],
            &options,
            &mut Scratch::default(),
            &mut clean,
        )
        .unwrap();
    }
    assert_eq!(chunk, clean);
}

/// §5.2: a slot aligned beyond the stack's alignment is placed at run
/// time (`Target::dynamic_slot`), and is aligned.
#[test]
fn over_aligned_slot() {
    let mut fx = Fx::new();
    let f = fx.func("f", 1, |m, std, b| {
        let small = b.add_slot(8, 8)?;
        let wide = b.add_slot(24, 64)?;
        let (small, wide) = (b.slot(small)?, b.slot(wide)?);
        let a = b.arg(0)?;
        b.store(a, small, al8())?;
        b.store(a, wide, al8())?;
        sink(m, std, b, a)?;
        let address = b.cast(Opcode::PtrToInt, I64, wide)?;
        let mask = int(m, 63);
        let misalign = b.binary(Opcode::And, I64, address, mask)?;
        let k = int(m, 1000);
        let scaled = b.binary(Opcode::Mul, I64, misalign, k)?;
        let x = b.load(I64, wide, al8())?;
        let y = b.load(I64, small, al8())?;
        let sum = b.binary(Opcode::Add, I64, x, y)?;
        let sum = b.binary(Opcode::Add, I64, sum, scaled)?;
        b.ret(Some(sum))?;
        Ok(())
    });
    assert_eq!(fx.check(f, &[21]), 42);
}
