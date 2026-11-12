//! What `classify` costs, not a gate (`spec/ir.md` §8.11).
//!
//! ```text
//! cargo run --release -p hod_ir --example classify_throughput [instructions] [symbols]
//! ```
//!
//! Builds the bodies of `body_throughput` (the instruction mix of
//! unoptimized C++) in a module with real symbols, and a module of
//! declarations, definitions and initialized variables, and reports:
//! the time per instruction of the body pass when everything is accepted
//! (next to the time it takes to build the same instruction), the time
//! per symbol of the module pass, the time to refuse a body at its first
//! reason, and the time per instruction of the census.

use std::hint::black_box;
use std::time::Instant;

use hod_ir::{
    Arch, Body, CallDesc, Census, Classifier, FnAttrs, FunctionBuilder, FunctionDesc,
    IntPredicate, IrResult, MemFlags, Module, Opcode, Support, SymId, TargetSpec, TypeId, Value,
    VariableDesc,
};

/// The instructions of one block of [`build`].
const PER_BLOCK: usize = 13;
const SLOTS: u32 = 24;

/// What every body of a run shares.
struct Shape {
    function: SymId,
    fn_ty: TypeId,
    callee: Value,
    step: Value,
}

fn shape(m: &mut Module) -> IrResult<Shape> {
    let fn_ty = m.types.function(TypeId::I32, &[TypeId::PTR, TypeId::I32], false)?;
    let mut attrs = FnAttrs::new();
    attrs.set(FnAttrs::NOUNWIND)?;
    let attrs = m.intern_fn_attrs(&attrs)?;
    let desc = FunctionDesc { attrs, ..FunctionDesc::new(fn_ty) };
    let function = m.add_function(b"_Z1fPii", &FunctionDesc { has_body: true, ..desc })?;
    let callee = m.add_function(b"_Z6calleePii", &desc)?;
    let step = m.consts.int(&m.types, TypeId::I32, 1)?;
    Ok(Shape { function, fn_ty, callee: Value::sym(callee)?, step: Value::constant(step)? })
}

/// The body of `body_throughput`: per block four loads, three stores,
/// two address computations, an add, a compare, a call and a branch.
fn build(m: &Module, shape: &Shape, insts: usize, storage: Option<Body>) -> IrResult<Body> {
    let int = MemFlags::aligned(4)?;
    let ptr = MemFlags::aligned(8)?;
    let mut b = match storage {
        Some(storage) => FunctionBuilder::reusing(m, shape.function, shape.fn_ty, storage)?,
        None => FunctionBuilder::new(m, shape.function, shape.fn_ty)?,
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
        let result = b.call(TypeId::I32, &CallDesc::new(shape.callee, shape.fn_ty, &args))?;
        b.store(result, limit, int)?;
        b.store(field, cursor, ptr)?;
        if index % 4 == 0 { b.cond_br(less, next, exit) } else { b.br(next) }?;
    }
    b.begin_block(exit)?;
    let result = b.load(TypeId::I32, slots[0], int)?;
    b.ret(Some(result))?;
    b.finish()
}

/// A module of `symbols` symbols as a C++ translation unit has them:
/// half function declarations, three in ten functions with a body, two
/// in ten variables with an initializer (every other one an address).
fn symbols(count: usize) -> IrResult<Module> {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let fn_ty = m.types.function(TypeId::I32, &[TypeId::PTR, TypeId::I32], false)?;
    let mut attrs = FnAttrs::new();
    attrs.set(FnAttrs::NOUNWIND)?;
    let attrs = m.intern_fn_attrs(&attrs)?;
    let seven = m.consts.int(&m.types, TypeId::I32, 7)?;
    let mut last = None;
    for index in 0..count {
        let name = format!("_ZN3std3__16detail4implI{index}E5applyEPKci");
        let name = name.as_bytes();
        match index % 10 {
            0..=4 => {
                last = Some(m.add_function(name, &FunctionDesc { attrs, ..FunctionDesc::new(fn_ty) })?);
            }
            5..=7 => {
                let desc = FunctionDesc { attrs, has_body: true, ..FunctionDesc::new(fn_ty) };
                m.add_function(name, &desc)?;
            }
            8 => {
                let desc = VariableDesc { init: Some(seven), ..VariableDesc::new(TypeId::I32) };
                m.add_variable(name, &desc)?;
            }
            _ => {
                let address = m.consts.addr(last.expect("a function"), 0)?;
                let desc = VariableDesc { init: Some(address), ..VariableDesc::new(TypeId::PTR) };
                m.add_variable(name, &desc)?;
            }
        }
    }
    Ok(m)
}

/// Nanoseconds per item, for display.
#[allow(clippy::cast_precision_loss, reason = "a figure printed with two decimals")]
fn per(nanos: u128, items: usize) -> f64 {
    nanos as f64 / items as f64
}

/// The fastest of seven runs of `run`, in nanoseconds, and its last result.
fn fastest<T>(mut run: impl FnMut() -> T) -> (u128, T) {
    let mut best: Option<(u128, T)> = None;
    for _ in 0..7 {
        let start = Instant::now();
        let result = run();
        let nanos = start.elapsed().as_nanos();
        if best.as_ref().is_none_or(|&(least, _)| nanos < least) {
            best = Some((nanos, result));
        }
    }
    best.expect("seven runs")
}

fn main() -> IrResult<()> {
    let mut args = std::env::args().skip(1);
    let mut number = |default: usize| match args.next() {
        Some(text) => text.parse().expect("a number"),
        None => default,
    };
    let (insts, symbol_count) = (number(1_000_000), number(100_000));
    let arch = Arch::Aarch64;
    let everything = Support::everything_implemented(arch);
    let accepting = Classifier::new(&everything);
    let shipped = Classifier::new(Support::shipped(arch));

    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let shape = shape(&mut m)?;

    // One large body.
    let (building, body) = fastest(|| build(&m, &shape, insts, None));
    let body = body?;
    let count = body.inst_count();
    let (accept, result) = fastest(|| accepting.body(black_box(&m), black_box(&body)));
    result.expect("everything implemented accepts the body");
    let (counting, census) = fastest(|| {
        let mut census = Census::new();
        shipped.census_body(black_box(&m), black_box(&body), &mut census);
        census
    });
    println!("one body of {count} instructions in {} blocks", body.block_count());
    println!("  build                        {:7.2} ns/instruction", per(building, count));
    println!("  classify, accepted           {:7.2} ns/instruction", per(accept, count));
    println!(
        "  census, {:2} constructs        {:7.2} ns/instruction",
        census.len(),
        per(counting, count)
    );

    // The same instructions as many small bodies.
    let small = 200;
    let functions = (insts / small).max(1);
    let bodies: Vec<Body> =
        (0..functions).map(|_| build(&m, &shape, small, None)).collect::<IrResult<_>>()?;
    let total: usize = bodies.iter().map(Body::inst_count).sum();
    let (accept, refused) = fastest(|| {
        bodies.iter().filter(|body| accepting.body(black_box(&m), body).is_err()).count()
    });
    assert_eq!(refused, 0, "everything implemented accepts the bodies");
    let (reject, refused) = fastest(|| {
        bodies.iter().filter(|body| shipped.body(black_box(&m), body).is_err()).count()
    });
    assert_eq!(refused, functions, "the shipped matrix refuses the bodies");
    println!("{functions} bodies of {} instructions", total / functions);
    println!("  classify, accepted           {:7.2} ns/instruction", per(accept, total));
    println!("  classify, accepted           {:7.1} ns/body", per(accept, functions));
    println!("  classify, refused at once    {:7.1} ns/body", per(reject, functions));

    // The module pass.
    let module = symbols(symbol_count)?;
    let (accept, result) = fastest(|| accepting.module(black_box(&module)));
    result.expect("everything implemented accepts the module");
    let (counting, census) = fastest(|| {
        let mut census = Census::new();
        shipped.census_module(black_box(&module), &mut census);
        census
    });
    println!("one module of {} symbols", module.symbol_count());
    println!("  classify, accepted           {:7.2} ns/symbol", per(accept, symbol_count));
    println!(
        "  census, {:2} constructs        {:7.2} ns/symbol",
        census.len(),
        per(counting, symbol_count)
    );
    Ok(())
}
