//! A throughput sanity check for function bodies, not a gate
//! (`spec/ir.md` §5.9).
//!
//! ```text
//! cargo run --release -p hod_ir --example body_throughput [instructions]
//! ```
//!
//! Builds bodies with the instruction mix of unoptimized C++ (loads and
//! stores of stack slots, calls, address arithmetic, compares, branches)
//! and reports the time per instruction to build, to verify and to read
//! one, and the bytes per instruction a body holds: for one large body
//! and for many small ones, in fresh storage and in recycled storage.

use std::time::Instant;

use hod_ir::{
    Body, CallDesc, FunctionBuilder, InstKind, IntPredicate, IrResult, MemFlags, Module, Opcode,
    SymId, TargetSpec, TypeId, Value,
};

/// The instructions of one block of [`build`].
const PER_BLOCK: usize = 13;
const SLOTS: u32 = 24;

/// What every body of a run shares.
struct Shape {
    fn_ty: TypeId,
    callee_ty: TypeId,
    callee: Value,
    step: Value,
}

fn shape(m: &mut Module) -> IrResult<Shape> {
    let fn_ty = m.types.function(TypeId::I32, &[TypeId::PTR, TypeId::I32], false)?;
    let step = m.consts.int(&m.types, TypeId::I32, 1)?;
    Ok(Shape {
        fn_ty,
        callee_ty: fn_ty,
        callee: Value::sym(SymId::new(1))?,
        step: Value::constant(step)?,
    })
}

/// A body of about `insts` instructions in blocks of [`PER_BLOCK`]: four
/// loads, three stores, two address computations (one with a constant
/// offset, one with a scaled index), an add, a compare, a call and a
/// branch, on values kept in stack slots as `-O0` keeps them.
fn build(m: &Module, shape: &Shape, insts: usize, storage: Option<Body>) -> IrResult<Body> {
    let int = MemFlags::aligned(4)?;
    let ptr = MemFlags::aligned(8)?;
    let sym = SymId::new(0);
    let mut b = match storage {
        Some(storage) => FunctionBuilder::reusing(m, sym, shape.fn_ty, storage)?,
        None => FunctionBuilder::new(m, sym, shape.fn_ty)?,
    };
    let mut slots = Vec::with_capacity(SLOTS as usize);
    for index in 0..SLOTS {
        let (size, align) = if index % 3 == 2 { (8, 8) } else { (4, 4) };
        let slot = b.add_slot(size, align)?;
        slots.push(b.slot(slot)?);
    }
    let blocks = (insts / PER_BLOCK).max(1);
    let exit = b.create_block()?;
    let mut next = b.create_block()?;
    for index in 0..blocks {
        let block = next;
        next = if index + 1 == blocks { exit } else { b.create_block()? };
        b.begin_block(block)?;
        let group = (index % (SLOTS as usize / 3)) * 3;
        let (count, limit, cursor) = (slots[group], slots[group + 1], slots[group + 2]);
        let index_value = b.load(TypeId::I32, count, int)?;
        let bound = b.load(TypeId::I32, limit, int)?;
        let less = b.icmp(IntPredicate::Slt, TypeId::I1, index_value, bound)?;
        let base = b.load(TypeId::PTR, cursor, ptr)?;
        let field = b.ptr_add(TypeId::PTR, base, 8 + (index % 16) as i64 * 4, &[])?;
        let elem = b.ptr_add(TypeId::PTR, field, 0, &[(index_value, 4)])?;
        let value = b.load(TypeId::I32, elem, int)?;
        let sum = b.binary(Opcode::Add, TypeId::I32, value, shape.step)?;
        b.store(sum, count, int)?;
        let args = [base, sum];
        let result = b.call(TypeId::I32, &CallDesc::new(shape.callee, shape.callee_ty, &args))?;
        b.store(result, limit, int)?;
        b.store(field, cursor, ptr)?;
        // Mostly straight on, sometimes around a block.
        if index % 4 == 0 { b.cond_br(less, next, exit) } else { b.br(next) }?;
    }
    b.begin_block(exit)?;
    let result = b.load(TypeId::I32, slots[0], int)?;
    b.ret(Some(result))?;
    b.finish()
}

/// What code generation asks of every instruction, summed so that the
/// reads are not optimized away.
fn read(body: &Body) -> u64 {
    let mut sum = 0u64;
    for block in body.blocks() {
        sum += u64::from(block.has_phis());
        for inst in block.insts() {
            sum += u64::from(inst.ty().raw()) + u64::from(inst.uses().count());
            sum += u64::from(inst.uses().only_use_is_next());
            sum += match inst.kind() {
                InstKind::Call(call) => call.args().len() as u64,
                _ => inst.operands().len() as u64,
            };
        }
    }
    sum
}

/// Nanoseconds per instruction, for display.
#[allow(clippy::cast_precision_loss, reason = "a figure printed with one decimal")]
fn per_inst(nanos: u128, insts: usize) -> f64 {
    nanos as f64 / insts as f64
}

#[allow(clippy::cast_precision_loss, reason = "a figure printed with one decimal")]
fn ratio(bytes: usize, insts: usize) -> f64 {
    bytes as f64 / insts as f64
}

/// The fastest of five runs of `run`, in nanoseconds, and its last result.
fn fastest<T>(mut run: impl FnMut() -> IrResult<T>) -> IrResult<(u128, T)> {
    let mut best: Option<(u128, T)> = None;
    for _ in 0..5 {
        let start = Instant::now();
        let result = run()?;
        let nanos = start.elapsed().as_nanos();
        if best.as_ref().is_none_or(|&(least, _)| nanos < least) {
            best = Some((nanos, result));
        }
    }
    Ok(best.expect("five runs"))
}

fn main() -> IrResult<()> {
    let insts: usize = match std::env::args().nth(1) {
        Some(text) => text.parse().expect("the number of instructions"),
        None => 1_000_000,
    };
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let shape = shape(&mut m)?;

    // One large body.
    let (fresh, body) = fastest(|| build(&m, &shape, insts, None))?;
    let count = body.inst_count();
    let mut storage = Some(body);
    let (recycled, ()) = fastest(|| {
        storage = Some(build(&m, &shape, insts, storage.take())?);
        Ok(())
    })?;
    let body = storage.expect("the storage");
    let (verify, ()) = fastest(|| body.verify(&m))?;
    let (reading, sum) = fastest(|| Ok(read(&body)))?;
    println!("one body of {count} instructions in {} blocks", body.block_count());
    println!("  build, fresh storage     {:6.1} ns/instruction", per_inst(fresh, count));
    println!("  build, recycled storage  {:6.1} ns/instruction", per_inst(recycled, count));
    println!("  verify                   {:6.1} ns/instruction", per_inst(verify, count));
    println!("  read (sum {sum})  {:6.1} ns/instruction", per_inst(reading, count));
    println!(
        "  table bytes              {:6.1} bytes/instruction",
        ratio(body.table_bytes(), count)
    );
    println!(
        "  allocated, fresh         {:6.1} bytes/instruction",
        ratio(build(&m, &shape, insts, None)?.allocated_bytes(), count)
    );

    // The same instructions as many small bodies.
    let small = 200;
    let functions = (insts / small).max(1);
    let (fresh, total) = fastest(|| {
        let mut total = 0;
        for _ in 0..functions {
            total += build(&m, &shape, small, None)?.inst_count();
        }
        Ok(total)
    })?;
    let (recycled, held) = fastest(|| {
        let mut storage = None;
        for _ in 0..functions {
            storage = Some(build(&m, &shape, small, storage)?);
        }
        Ok(storage.map_or(0, |body| body.allocated_bytes()))
    })?;
    println!("{functions} bodies of {} instructions", total / functions);
    println!("  build, fresh storage     {:6.1} ns/instruction", per_inst(fresh, total));
    println!("  build, recycled storage  {:6.1} ns/instruction", per_inst(recycled, total));
    println!("  storage held when recycling: {held} bytes");
    Ok(())
}
