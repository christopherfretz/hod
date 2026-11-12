//! The AArch64 skeleton (WP-033, `spec/codegen.md` §5.5, §8.2, §12):
//! a builder-made `main` linked by every linker and run, the classes, the
//! glue's constant and symbol sequences, the refusals of the groups that
//! have not landed, and the cost of a trivial function.

mod common;

use std::time::Instant;

use common::linking::{Toolchain, exit_code};
use common::{code_words, define, emit, int, module, returns, write_object};
use hod_a64::A64;
use hod_cg::{CgError, Chunk, Class, Options, Scratch, Target, compile_function};
use hod_ir::{AtomicOrdering, FunctionBuilder, MemFlags, Opcode, SyncScope, TypeId, TypeKind};
use hod_mc_a64 as mc;
use hod_mc_a64::{Reg, RegSp, Size};

/// The acceptance case: `i32 main() { ret i32 42 }`, built with the
/// builder, emitted, linked by GNU ld, lld and mold, run: exit 42.
#[test]
fn main_returning_42_links_and_runs_with_every_linker() {
    let (m, bodies) = returns(42);
    let emitted = emit(&m, &bodies, &Options::default());
    let dir = hod_testutil::Scratch::new("a64-42");
    let object = write_object(&dir, "main.o", &emitted);
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    assert!(
        !hod_testutil::tools_required() || toolchain.linkers.len() == 3,
        "bfd, lld and mold are all required"
    );
    for linker in &toolchain.linkers {
        let program = dir.join(format!("main-{}", linker.name));
        let ran = toolchain.link_and_run(linker, &[], &[&object], &program);
        assert_eq!(exit_code(&ran), Some(42), "{}: {}", linker.name, ran.stderr_text());
    }
}

/// §5.5: the exact code of the smallest function: the prologue, the
/// constant, the epilogue that the last block falls into.
#[test]
fn the_smallest_function_is_prologue_constant_epilogue() {
    let (m, bodies) = returns(42);
    let words = code_words(&m, &bodies, &Options::default());
    let sp = RegSp::SP;
    let fp = Reg::X29.sp();
    let expect = [
        mc::sub_imm(Size::X, sp, sp, 16),
        mc::sub_imm(Size::X, sp, sp, 0),
        mc::stp_off(Size::X, Reg::X29.zr(), Reg::X30.zr(), sp, 0),
        mc::mov_sp(Size::X, fp, sp),
        mc::movz(Size::W, Reg::X0.zr(), 42, 0),
        mc::ldp_off(Size::X, Reg::X29.zr(), Reg::X30.zr(), sp, 0),
        mc::add_imm(Size::X, sp, sp, 16),
        mc::ret(Reg::X30.zr()),
    ];
    assert_eq!(words, expect, "{words:08x?}");
}

/// §2.2: the class of each kind of type.
#[test]
fn classes() {
    let mut m = module();
    let t = &mut m.types;
    let two_i32 = t.vector(TypeId::I32, 2).unwrap();
    let four_f32 = t.vector(TypeId::FLOAT, 4).unwrap();
    let three_i32 = t.vector(TypeId::I32, 3).unwrap();
    let eight_i32 = t.vector(TypeId::I32, 8).unwrap();
    let st = t.struct_type(&[TypeId::I32, TypeId::I64], false).unwrap();
    let i128 = t.int(128).unwrap();
    let i33 = t.int(33).unwrap();
    let half = TypeId::HALF;
    let fp128 = TypeId::FP128;
    let types = &m.types;
    for (ty, class) in [
        (TypeId::I1, Class::Gpr),
        (TypeId::I8, Class::Gpr),
        (i33, Class::Gpr),
        (TypeId::I64, Class::Gpr),
        (TypeId::PTR, Class::Gpr),
        (half, Class::Fpr),
        (TypeId::FLOAT, Class::Fpr),
        (TypeId::DOUBLE, Class::Fpr),
        (fp128, Class::Fpr),
        (two_i32, Class::Fpr),
        (four_f32, Class::Fpr),
        (three_i32, Class::Mem),
        (eight_i32, Class::Mem),
        (i128, Class::Mem),
        (st, Class::Mem),
    ] {
        assert_eq!(A64.class_of(types, ty), class, "{:?}", types.kind(ty));
    }
    assert!(matches!(types.kind(fp128), TypeKind::Fp128));
}

/// Each group's refusal is its stable name until its package lands.
#[test]
fn unimplemented_groups_are_refused_by_name() {
    let mut m = module();
    let (sym, ty) = define(&mut m, "f", TypeId::I64, &[TypeId::I64]);
    let one = int(&mut m, TypeId::I64, 1);
    let mut b = FunctionBuilder::new(&m, sym, ty).unwrap();
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    let x = b.arg(0).unwrap();
    // The `add` is lowered (WP-035) and its code emitted before the
    // atomic `fp128` load, which has no sequence (WP-074), is refused.
    let sum = b.binary(Opcode::Add, TypeId::I64, x, one).unwrap();
    let p = b.cast(Opcode::IntToPtr, TypeId::PTR, x).unwrap();
    let atomic =
        MemFlags::aligned(16).unwrap().with_atomic(AtomicOrdering::SeqCst, SyncScope::System);
    b.load(TypeId::FP128, p, atomic).unwrap();
    b.ret(Some(sum)).unwrap();
    let body = b.finish().unwrap();
    let mut scratch = Scratch::new();
    let mut chunk = Chunk::new();
    let result = compile_function(&A64, &m, &body, &Options::default(), &mut scratch, &mut chunk);
    assert_eq!(result, Err(CgError::unsupported(hod_a64::atomic::TYPE)));
    assert!(chunk.code.is_empty(), "a refused function leaves no trace");
}

/// §11: the cost of compiling a trivial function with a warm worker, in
/// nanoseconds per function (reported, not a gate).
#[test]
fn nanoseconds_per_trivial_function() {
    let (m, bodies) = returns(7);
    let body = &bodies[0];
    let options = Options::default();
    let mut scratch = Scratch::new();
    let mut chunk = Chunk::new();
    let rounds = if cfg!(debug_assertions) { 20_000 } else { 200_000 };
    for _ in 0..100 {
        compile_function(&A64, &m, body, &options, &mut scratch, &mut chunk).unwrap();
    }
    let start = Instant::now();
    for round in 0..rounds {
        if round % 1000 == 0 {
            chunk = Chunk::new();
        }
        compile_function(&A64, &m, body, &options, &mut scratch, &mut chunk).unwrap();
    }
    let ns = start.elapsed().as_nanos() / rounds as u128;
    let build = if cfg!(debug_assertions) { "debug" } else { "release" };
    eprintln!("a64: {ns} ns per trivial function ({build} build, {rounds} rounds)");
}
