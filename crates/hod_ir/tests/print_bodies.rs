//! `spec/ir.md` §9.4 and §9.5: every instruction, in every spelling the
//! printer has for it, is text LLVM accepts. One module of hand-written
//! bodies holds each opcode, predicate, cast, atomic operation, memory
//! flag, calling convention, tail marker, call-site attribute, operand
//! bundle, inline-assembly flag, landing-pad clause and a spread of
//! intrinsics; it is printed for LLVM 22 and LLVM 19 and must pass
//! `llvm-as` and `opt -passes=verify`.
//!
//! The coverage is asserted, not assumed: the test walks `Opcode::ALL`,
//! `IntPredicate::ALL`, `FloatPredicate::ALL` and `RmwOp::ALL` and requires
//! each member in an accepted body, or, for what a dialect lacks, the error
//! that names it (§9.2). The funclet opcodes have no text: each is the
//! error `inst:<opcode>`.

mod print_support;

use std::collections::BTreeSet;

use hod_ir::{
    AtomicOrdering, BlockId, Body, Bundle, CallConv, CallDesc, Clause, Dialect, FloatPredicate,
    FunctionBuilder, FunctionDesc, InlineAsm, InstKind, IntPredicate, Intrinsic, MemFlags, Module,
    Opcode, ParamAttrs, ParamMem, PrintError, RmwOp, SHUFFLE_UNDEF, SymId, SyncScope, TailKind,
    TargetSpec, TypeId, Value, VariableDesc,
};
use print_support::{Fx, Llvm, declare, define, int, printed};

const VOID: TypeId = TypeId::VOID;
const I1: TypeId = TypeId::I1;
const I8: TypeId = TypeId::I8;
const I32: TypeId = TypeId::I32;
const I64: TypeId = TypeId::I64;
const PTR: TypeId = TypeId::PTR;
const FLOAT: TypeId = TypeId::FLOAT;
const DOUBLE: TypeId = TypeId::DOUBLE;

/// A body, and the first LLVM major whose dialect has all of it: before
/// that, printing it is the error `construct`.
struct Case {
    body: Body,
    since: u32,
    construct: &'static str,
}

/// The module being written and its bodies.
struct Bodies {
    m: Module,
    cases: Vec<Case>,
}

impl Bodies {
    fn body(&mut self, name: &str, ret: TypeId, params: &[TypeId]) -> Fx<'_> {
        let f = define(&mut self.m, name, ret, params);
        Fx::new(&mut self.m, f)
    }

    fn push(&mut self, body: Body) {
        self.cases.push(Case { body, since: 19, construct: "" });
    }

    fn push_since(&mut self, body: Body, since: u32, construct: &'static str) {
        self.cases.push(Case { body, since, construct });
    }
}

/// Every integer and floating-point binary operator, `fneg`, and a
/// constant of every floating-point type.
fn arithmetic(all: &mut Bodies) {
    let mut x = all.body("ints", I64, &[I64, I64]);
    let mut acc = x.arg(0);
    for &op in Opcode::ALL.iter().filter(|op| op.is_binary() && !op.name().starts_with('f')) {
        acc = x.bin(op, I64, acc, x.arg(1));
    }
    x.ret(acc);
    let body = x.done();
    all.push(body);

    let mut x = all.body("floats", DOUBLE, &[DOUBLE, DOUBLE]);
    let mut acc = x.arg(0);
    for &op in Opcode::ALL.iter().filter(|op| op.is_binary() && op.name().starts_with('f')) {
        acc = x.bin(op, DOUBLE, acc, x.arg(1));
    }
    let acc = x.b.fneg(DOUBLE, acc).unwrap();
    x.ret(acc);
    let body = x.done();
    all.push(body);

    // 1.0 in each format, added to an argument of its type.
    let kinds = [
        (TypeId::HALF, 0x3c00_u128),
        (TypeId::BFLOAT, 0x3f80),
        (FLOAT, 0x3f80_0000),
        (DOUBLE, 0x3ff0_0000_0000_0000),
        (TypeId::FP128, 0x3fff_u128 << 112),
        (TypeId::X86_FP80, (0x3fff_u128 << 64) | (1 << 63)),
    ];
    let params: Vec<TypeId> = kinds.iter().map(|&(ty, _)| ty).collect();
    let mut x = all.body("float_kinds", VOID, &params);
    for (at, (ty, bits)) in kinds.into_iter().enumerate() {
        let one = x.m.consts.float(&x.m.types, ty, bits).unwrap();
        let one = x.konst(one);
        x.bin(Opcode::FAdd, ty, x.arg(at as u32), one);
    }
    x.ret_void();
    let body = x.done();
    all.push(body);
}

/// Every predicate of `icmp` and `fcmp`, on scalars, pointers and
/// vectors.
fn comparisons(all: &mut Bodies) {
    let v4 = all.m.types.vector(I32, 4).unwrap();
    let v4i1 = all.m.types.vector(I1, 4).unwrap();
    let mut x = all.body("comparisons", I1, &[I64, I64, DOUBLE, DOUBLE, PTR, v4]);
    let mut acc = x.int(I1, 1);
    for &pred in IntPredicate::ALL {
        let bit = x.icmp(pred, x.arg(0), x.arg(1));
        acc = x.bin(Opcode::And, I1, acc, bit);
    }
    for &pred in FloatPredicate::ALL {
        let bit = x.fcmp(pred, x.arg(2), x.arg(3));
        acc = x.bin(Opcode::Xor, I1, acc, bit);
    }
    let null = x.null();
    let bit = x.icmp(IntPredicate::Ne, x.arg(4), null);
    acc = x.bin(Opcode::Or, I1, acc, bit);
    x.b.icmp(IntPredicate::Sgt, v4i1, x.arg(5), x.arg(5)).unwrap();
    x.ret(acc);
    let body = x.done();
    all.push(body);
}

/// Every cast.
fn casts(all: &mut Bodies) {
    let far = all.m.types.ptr(1).unwrap();
    let mut x = all.body("casts", VOID, &[I64, DOUBLE, PTR, FLOAT, I32]);
    let (a, d, p, f, w) = (x.arg(0), x.arg(1), x.arg(2), x.arg(3), x.arg(4));
    for (op, ty, arg) in [
        (Opcode::Trunc, I8, a),
        (Opcode::ZExt, I64, w),
        (Opcode::SExt, I64, w),
        (Opcode::FPTrunc, FLOAT, d),
        (Opcode::FPExt, DOUBLE, f),
        (Opcode::FPToUI, I32, d),
        (Opcode::FPToSI, I32, d),
        (Opcode::UIToFP, DOUBLE, a),
        (Opcode::SIToFP, DOUBLE, a),
        (Opcode::PtrToInt, I64, p),
        (Opcode::PtrToInt, I8, p),
        (Opcode::PtrToAddr, I64, p),
        (Opcode::IntToPtr, PTR, a),
        (Opcode::BitCast, I64, d),
        (Opcode::AddrSpaceCast, far, p),
    ] {
        x.cast(op, ty, arg);
    }
    x.ret_void();
    let body = x.done();
    all.push(body);
}

/// Loads and stores with every flag, slots, dynamic allocations, `fence`,
/// `cmpxchg` and every `atomicrmw` operation.
fn memory(all: &mut Bodies) {
    let pair = all.m.types.struct_type(&[I64, I1], false).unwrap();
    let mut x = all.body("memory", VOID, &[PTR, I64, DOUBLE]);
    let slot = x.b.add_slot(24, 16).unwrap();
    let slot = x.b.slot(slot).unwrap();
    let (p, n, d) = (x.arg(0), x.arg(1), x.arg(2));
    let plain = MemFlags::aligned(8).unwrap();
    let byte = MemFlags::aligned(1).unwrap();
    let big = MemFlags::aligned(4096).unwrap();
    let ordered = |ordering| plain.with_atomic(ordering, SyncScope::System);
    let single = plain.with_atomic(AtomicOrdering::SeqCst, SyncScope::SingleThread);
    for mem in [plain, byte, big, plain.with_volatile(), single, single.with_volatile()] {
        let loaded = x.b.load(I64, p, mem).unwrap();
        x.b.store(loaded, slot, mem).unwrap();
    }
    for &ordering in AtomicOrdering::ALL {
        if ordering.valid_for_load() {
            x.b.load(I64, p, ordered(ordering)).unwrap();
        }
        if ordering.valid_for_store() {
            x.b.store(n, p, ordered(ordering)).unwrap();
        }
    }
    x.b.alloca(PTR, n, 1, 1).unwrap();
    let eight = x.i32(8);
    x.b.alloca(PTR, eight, 24, 32).unwrap();
    x.b.fence(AtomicOrdering::SeqCst, SyncScope::System).unwrap();
    x.b.fence(AtomicOrdering::Acquire, SyncScope::SingleThread).unwrap();
    x.b.cmpxchg(pair, p, n, n, ordered(AtomicOrdering::SeqCst), AtomicOrdering::SeqCst, false)
        .unwrap();
    x.b.cmpxchg(
        pair,
        p,
        n,
        n,
        single.with_volatile(),
        AtomicOrdering::Monotonic,
        true,
    )
    .unwrap();
    x.b.cmpxchg(pair, p, n, n, ordered(AtomicOrdering::AcqRel), AtomicOrdering::Acquire, true)
        .unwrap();
    for &op in RmwOp::ALL {
        if rmw_since(op) > 19 {
            continue;
        }
        let (ty, value) = if op.is_float() { (DOUBLE, d) } else { (I64, n) };
        x.b.atomic_rmw(op, ty, p, value, ordered(AtomicOrdering::SeqCst)).unwrap();
    }
    x.b.atomic_rmw(RmwOp::Xchg, PTR, p, p, single.with_volatile()).unwrap();
    x.b.atomic_rmw(RmwOp::Xchg, DOUBLE, p, d, ordered(AtomicOrdering::Release)).unwrap();
    x.ret_void();
    let body = x.done();
    all.push(body);

    // The operations LLVM gained later, each alone in a body.
    for &op in RmwOp::ALL {
        let since = rmw_since(op);
        if since == 19 {
            continue;
        }
        let name = format!("rmw_{}", op.name());
        let mut x = all.body(&name, VOID, &[PTR, I64, DOUBLE]);
        let (ty, value) = if op.is_float() { (DOUBLE, x.arg(2)) } else { (I64, x.arg(1)) };
        x.b.atomic_rmw(op, ty, x.arg(0), value, ordered(AtomicOrdering::SeqCst)).unwrap();
        x.ret_void();
        let body = x.done();
        let construct = match op {
            RmwOp::USubCond => "atomicrmw:usub_cond",
            RmwOp::USubSat => "atomicrmw:usub_sat",
            RmwOp::FMaximum => "atomicrmw:fmaximum",
            RmwOp::FMinimum => "atomicrmw:fminimum",
            other => panic!("{other:?} is in every dialect"),
        };
        all.push_since(body, since, construct);
    }
}

/// The first LLVM major with this `atomicrmw` operation (§9.2).
fn rmw_since(op: RmwOp) -> u32 {
    match op {
        RmwOp::USubCond | RmwOp::USubSat => 20,
        RmwOp::FMaximum | RmwOp::FMinimum => 21,
        _ => 19,
    }
}

/// `PtrAdd` in every shape, aggregates by byte offset through nested and
/// packed types, and the vector instructions, scalable vectors too.
fn aggregates(all: &mut Bodies) {
    let types = &mut all.m.types;
    let i16_ty = types.int(16).unwrap();
    let shorts = types.array(i16_ty, 2).unwrap();
    let inner = types.struct_type(&[I32, shorts], false).unwrap();
    let packed = types.struct_type(&[I8, I64], true).unwrap();
    let empty = types.struct_type(&[], false).unwrap();
    let none = types.array(I32, 0).unwrap();
    // { i8, { i32, [2 x i16] }, <{ i8, i64 }>, [0 x i32], {}, i32 }
    let record = types.struct_type(&[I8, inner, packed, none, empty, I32], false).unwrap();
    let offsets = types.field_offsets(record).unwrap().to_vec();
    assert_eq!(offsets, [0, 4, 12, 24, 24, 24]);
    let v4 = types.vector(I32, 4).unwrap();
    let v2 = types.vector(I32, 2).unwrap();
    let v8 = types.vector(I32, 8).unwrap();
    let nx4 = types.scalable_vector(I32, 4).unwrap();
    let vptr = types.vector(PTR, 2).unwrap();
    let v2i64 = types.vector(I64, 2).unwrap();
    let i128_ty = types.int(128).unwrap();

    let mut x = all.body("fields", record, &[record, I64]);
    let (s, n) = (x.arg(0), x.arg(1));
    for (ty, offset) in [
        (I8, 0),
        (inner, 4),
        (I32, 4),
        (shorts, 8),
        (i16_ty, 8),
        (i16_ty, 10),
        (packed, 12),
        (I8, 12),
        (I64, 13),
        (none, 24),
        (empty, 24),
        (I32, 24),
    ] {
        x.b.extract_value(ty, s, offset).unwrap();
    }
    let short = x.int(i16_ty, -2);
    let updated = x.b.insert_value(record, s, short, 10).unwrap();
    let updated = x.b.insert_value(record, updated, n, 13).unwrap();
    let whole = x.b.extract_value(inner, updated, 4).unwrap();
    let updated = x.b.insert_value(record, updated, whole, 4).unwrap();
    x.ret(updated);
    let body = x.done();
    all.push(body);

    let mut x = all.body("pointers", PTR, &[PTR, I8, i128_ty, vptr, v2i64]);
    let (p, narrow, wide, pointers, lanes) = (x.arg(0), x.arg(1), x.arg(2), x.arg(3), x.arg(4));
    x.ptr_add(p, 0, &[]);
    x.ptr_add(p, i64::MIN, &[]);
    x.ptr_add(p, -1, &[(narrow, 1), (wide, 24), (narrow, 0x1_0000_0000)]);
    let seven = x.i64(7);
    x.ptr_add(p, 16, &[(seven, 3)]);
    // A vector of pointers, and vector indices.
    x.b.ptr_add(vptr, pointers, 8, &[(lanes, 4)]).unwrap();
    x.b.ptr_add(vptr, p, 0, &[(lanes, 12)]).unwrap();
    let last = x.b.ptr_add(vptr, pointers, 0, &[(narrow, 2)]).unwrap();
    let zero = x.i32(0);
    let result = x.b.extract_element(PTR, last, zero).unwrap();
    x.ret(result);
    let body = x.done();
    all.push(body);

    let mut x = all.body("lanes", VOID, &[v4, nx4, I32, I64]);
    let (v, sv, e, i) = (x.arg(0), x.arg(1), x.arg(2), x.arg(3));
    x.b.extract_element(I32, v, i).unwrap();
    x.b.extract_element(I32, sv, e).unwrap();
    x.b.insert_element(v4, v, e, i).unwrap();
    x.b.insert_element(nx4, sv, e, e).unwrap();
    x.b.shuffle_vector(v4, v, v, &[0, 5, SHUFFLE_UNDEF, 7]).unwrap();
    x.b.shuffle_vector(v2, v, v, &[3, 4]).unwrap();
    x.b.shuffle_vector(v8, v, v, &[0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
    x.b.shuffle_vector(v4, v, v, &[SHUFFLE_UNDEF; 4]).unwrap();
    x.b.shuffle_vector(nx4, sv, sv, &[0; 4]).unwrap();
    x.b.shuffle_vector(nx4, sv, sv, &[SHUFFLE_UNDEF; 4]).unwrap();
    x.ret_void();
    let body = x.done();
    all.push(body);
}

/// `phi`, `select`, `freeze`, `switch`, the branches, `indirectbr` over
/// `blockaddress` operands, `ret` and `unreachable`.
fn control(all: &mut Bodies) {
    let i128_ty = all.m.types.int(128).unwrap();
    let v4 = all.m.types.vector(I32, 4).unwrap();
    let v4i1 = all.m.types.vector(I1, 4).unwrap();
    let f = define(&mut all.m, "control", I32, &[I32, i128_ty, v4, v4i1]);
    let wide_cases = [
        all.m.consts.int_from_words(&all.m.types, i128_ty, &[5, 1 << 40]).unwrap(),
        all.m.consts.int_from_words(&all.m.types, i128_ty, &[u64::MAX, u64::MAX]).unwrap(),
    ];
    let mut x = Fx::new(&mut all.m, f);
    let entry = x.here();
    let (a, b, c, merge, dead, wide, jump, end) =
        (x.block(), x.block(), x.block(), x.block(), x.block(), x.block(), x.block(), x.block());
    let cases =
        [(int(x.m, I32, 0), a), (int(x.m, I32, -1), b), (int(x.m, I32, i64::from(i32::MIN)), a)];
    x.b.switch(x.arg(0), c, &cases).unwrap();
    x.at(a);
    let frozen = x.b.freeze(I32, x.arg(0)).unwrap();
    x.br(merge);
    x.at(b);
    let zero = x.i32(0);
    let negative = x.icmp(IntPredicate::Slt, x.arg(0), zero);
    x.cond_br(negative, merge, wide);
    x.at(c);
    x.b.select(v4, x.arg(3), x.arg(2), x.arg(2)).unwrap();
    x.br(merge);
    x.at(merge);
    let one = x.i32(1);
    let joined = x.phi(I32, &[(frozen, a), (one, b), (x.arg(0), c)]);
    let is_negative = negative_or(&mut x, joined);
    let chosen = x.select(I32, is_negative, joined, one);
    x.ret(chosen);
    x.at(dead);
    x.b.unreachable().unwrap();
    x.at(wide);
    x.b.switch(x.arg(1), jump, &[(wide_cases[0], end), (wide_cases[1], jump)]).unwrap();
    x.at(jump);
    // A switch with no cases, then a computed jump.
    let first = x.m.consts.block_addr(f, end).unwrap();
    let second = x.m.consts.block_addr(f, dead).unwrap();
    let (first, second) = (x.konst(first), x.konst(second));
    let target = x.select(PTR, negative, first, second);
    x.b.indirect_br(target, &[end, dead]).unwrap();
    x.at(end);
    x.b.switch(x.arg(0), dead, &[]).unwrap();
    assert_eq!(entry, BlockId::new(0));
    let body = x.done();
    all.push(body);
}

/// `value < 0`.
fn negative_or(x: &mut Fx<'_>, value: Value) -> Value {
    let zero = x.i32(0);
    x.icmp(IntPredicate::Slt, value, zero)
}

/// `va_arg` and the intrinsics around it, in a variadic function.
fn variadic(all: &mut Bodies) {
    let ty = all.m.types.function(I32, &[I32], true).unwrap();
    let f = all
        .m
        .add_function(b"variadic", &FunctionDesc { has_body: true, ..FunctionDesc::new(ty) })
        .unwrap();
    let mut x = Fx::new(&mut all.m, f);
    let list = x.b.add_slot(32, 8).unwrap();
    let copy = x.b.add_slot(32, 8).unwrap();
    let (list, copy) = (x.b.slot(list).unwrap(), x.b.slot(copy).unwrap());
    x.intrinsic(Intrinsic::VaStart, &[PTR], VOID, &[PTR], &[list]);
    let first = x.b.va_arg(I32, list).unwrap();
    x.intrinsic(Intrinsic::VaCopy, &[PTR], VOID, &[PTR, PTR], &[copy, list]);
    x.b.va_arg(DOUBLE, copy).unwrap();
    x.intrinsic(Intrinsic::VaEnd, &[PTR], VOID, &[PTR], &[copy]);
    x.intrinsic(Intrinsic::VaEnd, &[PTR], VOID, &[PTR], &[list]);
    x.ret(first);
    let body = x.done();
    all.push(body);
}

/// Calls: every calling convention, tail marker, flag, call-site
/// attribute and bundle; inline-assembly callees with every flag.
fn calls(all: &mut Bodies) {
    let m = &mut all.m;
    let unary = m.types.function(I64, &[I64], false).unwrap();
    let nullary = m.types.function(VOID, &[], false).unwrap();
    let printf = declare(m, "printf", I32, &[PTR], true);
    let printf_ty = m.symbol(printf).ty();
    let callee = declare(m, "callee", I64, &[I64], false);
    let i16_ty = m.types.int(16).unwrap();
    let pair = m.types.struct_type(&[I64, I64], false).unwrap();

    // A callee whose parameters carry what the call sites say.
    let flag = |flags| ParamAttrs::NONE.with(flags).unwrap();
    let memory = |mem| ParamAttrs::NONE.with_mem(mem, pair).unwrap().with_align(16).unwrap();
    // LLVM wants `sret` first and on a function that returns nothing.
    let attrs = [
        memory(ParamMem::Sret),
        flag(ParamAttrs::ZEROEXT),
        flag(ParamAttrs::SIGNEXT),
        flag(ParamAttrs::INREG),
        flag(ParamAttrs::NEST),
        flag(ParamAttrs::SWIFT_SELF),
        flag(ParamAttrs::SWIFT_ERROR),
        flag(ParamAttrs::SWIFT_ASYNC),
        memory(ParamMem::ByVal),
        memory(ParamMem::ByRef),
        ParamAttrs::NONE.with_align(64).unwrap(),
        flag(ParamAttrs::ZEROEXT | ParamAttrs::INREG).with_stack_align(16).unwrap(),
    ];
    let params = [PTR, I8, i16_ty, I64, PTR, PTR, PTR, PTR, PTR, PTR, PTR, I32];
    let attributed_ty = m.types.function(VOID, &params, false).unwrap();
    let desc = FunctionDesc { param_attrs: &attrs, ..FunctionDesc::new(attributed_ty) };
    let attributed = m.add_function(b"attributed", &desc).unwrap();
    let narrow_ty = m.types.function(i16_ty, &[i16_ty], false).unwrap();
    let narrow_attrs = [flag(ParamAttrs::SIGNEXT)];
    let desc = FunctionDesc {
        ret_attrs: flag(ParamAttrs::ZEROEXT),
        param_attrs: &narrow_attrs,
        ..FunctionDesc::new(narrow_ty)
    };
    let narrow = m.add_function(b"narrow", &desc).unwrap();

    // calls(ptr f, i64 x, ptr swifterror e, ptr p)
    let ty = m.types.function(VOID, &[PTR, I64, PTR, PTR], false).unwrap();
    let swifterror = [ParamAttrs::NONE, ParamAttrs::NONE, attrs[6], ParamAttrs::NONE];
    let desc = FunctionDesc { param_attrs: &swifterror, has_body: true, ..FunctionDesc::new(ty) };
    let f = m.add_function(b"calls", &desc).unwrap();
    let asm = |m: &mut Module, template: &[u8], constraints: &[u8], flags: u8, ty: TypeId| {
        Value::asm(m.inline_asm(&InlineAsm { template, constraints, flags, ty }).unwrap()).unwrap()
    };
    let nop = asm(m, b"nop", b"", 0, nullary);
    let all_flags = InlineAsm::SIDE_EFFECT
        | InlineAsm::ALIGN_STACK
        | InlineAsm::INTEL_DIALECT
        | InlineAsm::UNWIND;
    let flagged = asm(m, b"nop // \"quoted\" \\ and\n\ttabbed", b"~{x0},~{memory}", all_flags, nullary);
    let moved = asm(m, b"mov $0, $1", b"=r,r", InlineAsm::SIDE_EFFECT, unary);
    let indirect_ty = m.types.function(VOID, &[PTR], false).unwrap();
    let indirect = asm(m, b"ldr x0, $0", b"*m,~{x0}", InlineAsm::SIDE_EFFECT, indirect_ty);
    let element = ParamAttrs::NONE.with_mem(ParamMem::ElementType, I64).unwrap();

    let mut x = Fx::new(m, f);
    let (target, n, error, p) = (x.arg(0), x.arg(1), x.arg(2), x.arg(3));
    let conventions = [
        CallConv::C,
        CallConv::FAST,
        CallConv::COLD,
        CallConv::GHC,
        CallConv::ANY_REG,
        CallConv::PRESERVE_MOST,
        CallConv::PRESERVE_ALL,
        CallConv::SWIFT,
        CallConv::CXX_FAST_TLS,
        CallConv::TAIL,
        CallConv::CFGUARD_CHECK,
        CallConv::SWIFT_TAIL,
        CallConv::PRESERVE_NONE,
        CallConv::from_llvm(97).unwrap(),
        CallConv::from_llvm(u32::from(CallConv::MAX)).unwrap(),
    ];
    for cc in conventions {
        x.b.call(I64, &CallDesc { cc, ..CallDesc::new(target, unary, &[n]) }).unwrap();
    }
    for tail in [TailKind::Tail, TailKind::NoTail] {
        let direct = Value::sym(callee).unwrap();
        x.b.call(I64, &CallDesc { tail, ..CallDesc::new(direct, unary, &[n]) }).unwrap();
    }
    for (noreturn, nounwind, returns_twice) in
        [(true, false, false), (false, true, false), (false, false, true), (true, true, true)]
    {
        let args = [n];
        let desc =
            CallDesc { noreturn, nounwind, returns_twice, ..CallDesc::new(target, unary, &args) };
        x.b.call(I64, &desc).unwrap();
    }
    // A variadic callee: the call spells the whole function type.
    let text = x.null();
    let half = x.f64(0.5);
    let printf_value = x.sym(printf);
    x.b.call(I32, &CallDesc::new(printf_value, printf_ty, &[text, n, half])).unwrap();
    // Every call-site attribute.
    let (byte, short, word) = (x.int(I8, -1), x.int(i16_ty, 0x7fff), x.i32(7));
    let args = [p, byte, short, n, p, p, error, p, p, p, p, word];
    let desc = CallDesc {
        arg_attrs: &attrs,
        ..CallDesc::new(Value::sym(attributed).unwrap(), attributed_ty, &args)
    };
    x.b.call(VOID, &desc).unwrap();
    let args = [short];
    let desc = CallDesc {
        ret_attrs: flag(ParamAttrs::ZEROEXT),
        arg_attrs: &narrow_attrs,
        ..CallDesc::new(Value::sym(narrow).unwrap(), narrow_ty, &args)
    };
    x.b.call(i16_ty, &desc).unwrap();
    // Bundles: with operands, without, two on one call, and the two LLVM
    // checks the operands of.
    let one = x.i32(1);
    let (kcfi, key, discriminator) = (x.i32(0x1234_5678), x.i32(1), x.i64(42));
    let bundle_sets: [&[Bundle<'_>]; 4] = [
        &[Bundle { tag: "deopt", inputs: &[one, p, n] }],
        &[Bundle { tag: "hod.tag", inputs: &[] }, Bundle { tag: "deopt", inputs: &[] }],
        &[Bundle { tag: "kcfi", inputs: &[kcfi] }],
        &[Bundle { tag: "ptrauth", inputs: &[key, discriminator] }],
    ];
    for bundles in bundle_sets {
        x.b.call(I64, &CallDesc { bundles, ..CallDesc::new(target, unary, &[n]) }).unwrap();
    }
    // Inline assembly.
    x.b.call(VOID, &CallDesc::new(nop, nullary, &[])).unwrap();
    x.b.call(VOID, &CallDesc::new(flagged, nullary, &[])).unwrap();
    x.b.call(I64, &CallDesc::new(moved, unary, &[n])).unwrap();
    x.b.call(VOID, &CallDesc { arg_attrs: &[element], ..CallDesc::new(indirect, indirect_ty, &[p]) })
        .unwrap();
    x.ret_void();
    let body = x.done();
    all.push(body);

    // `musttail`: the caller's signature, and `ret` next.
    let mut x = all.body("must", I64, &[I64]);
    let args = [x.arg(0)];
    let desc = CallDesc {
        tail: TailKind::MustTail,
        ..CallDesc::new(Value::sym(callee).unwrap(), unary, &args)
    };
    let result = x.b.call(I64, &desc).unwrap();
    x.ret(result);
    let body = x.done();
    all.push(body);

    // In a variadic function it forwards the variable arguments.
    let forward_ty = all.m.types.function(I32, &[PTR], true).unwrap();
    let f = all
        .m
        .add_function(b"forward", &FunctionDesc { has_body: true, ..FunctionDesc::new(forward_ty) })
        .unwrap();
    let mut x = Fx::new(&mut all.m, f);
    let args = [x.arg(0)];
    let desc = CallDesc {
        tail: TailKind::MustTail,
        ..CallDesc::new(Value::sym(printf).unwrap(), printf_ty, &args)
    };
    let result = x.b.call(I32, &desc).unwrap();
    x.ret(result);
    let body = x.done();
    all.push(body);
}

/// Intrinsics: names with one, two and three overload types, vector and
/// scalable-vector types among them, a pair result, immediate arguments,
/// an intrinsic hod does not know, and the ones later LLVMs added.
fn intrinsics(all: &mut Bodies) {
    let types = &mut all.m.types;
    let v4 = types.vector(I32, 4).unwrap();
    let v2 = types.vector(I32, 2).unwrap();
    let nx4 = types.scalable_vector(I32, 4).unwrap();
    let frexp = types.struct_type(&[DOUBLE, I32], false).unwrap();
    let mut x = all.body("intrinsics", VOID, &[PTR, I64, DOUBLE, v4, nx4, I32]);
    let (p, n, d, v, sv, w) = (x.arg(0), x.arg(1), x.arg(2), x.arg(3), x.arg(4), x.arg(5));
    let (no, yes) = (x.int(I1, 0), x.int(I1, 1));
    let (zero, three) = (x.i32(0), x.i32(3));
    let (byte, index) = (x.int(I8, 0), x.i64(0));
    let mem = [PTR, PTR, I64, I1];
    x.intrinsic(Intrinsic::Memcpy, &[PTR, PTR, I64], VOID, &mem, &[p, p, n, no]);
    x.intrinsic(Intrinsic::Memmove, &[PTR, PTR, I64], VOID, &mem, &[p, p, n, yes]);
    x.intrinsic(Intrinsic::Memset, &[PTR, I64], VOID, &[PTR, I8, I64, I1], &[p, byte, n, no]);
    x.intrinsic(Intrinsic::Fshl, &[I64], I64, &[I64, I64, I64], &[n, n, n]);
    x.intrinsic(Intrinsic::Ctlz, &[I64], I64, &[I64, I1], &[n, yes]);
    x.intrinsic(Intrinsic::Abs, &[v4], v4, &[v4, I1], &[v, no]);
    x.intrinsic(Intrinsic::Scmp, &[I8, I64], I8, &[I64, I64], &[n, n]);
    x.intrinsic(Intrinsic::Fma, &[DOUBLE], DOUBLE, &[DOUBLE, DOUBLE, DOUBLE], &[d, d, d]);
    x.intrinsic(Intrinsic::Powi, &[DOUBLE, I32], DOUBLE, &[DOUBLE, I32], &[d, w]);
    x.intrinsic(Intrinsic::Frexp, &[DOUBLE, I32], frexp, &[DOUBLE], &[d]);
    x.intrinsic(Intrinsic::Lround, &[I64, DOUBLE], I64, &[DOUBLE], &[d]);
    x.intrinsic(Intrinsic::FptosiSat, &[I32, DOUBLE], I32, &[DOUBLE], &[d]);
    x.intrinsic(Intrinsic::IsFpClass, &[DOUBLE], I1, &[DOUBLE, I32], &[d, three]);
    x.intrinsic(Intrinsic::VectorReduceAdd, &[v4], I32, &[v4], &[v]);
    x.intrinsic(Intrinsic::VectorReduceAdd, &[nx4], I32, &[nx4], &[sv]);
    x.intrinsic(Intrinsic::VectorExtract, &[v2, nx4], v2, &[nx4, I64], &[sv, index]);
    x.intrinsic(Intrinsic::ObjectSize, &[I64, PTR], I64, &[PTR, I1, I1, I1], &[p, no, yes, no]);
    x.intrinsic(Intrinsic::Ptrmask, &[PTR, I64], PTR, &[PTR, I64], &[p, n]);
    x.intrinsic(Intrinsic::Prefetch, &[PTR], VOID, &[PTR, I32, I32, I32], &[p, zero, three, zero]);
    let saved = x.intrinsic(Intrinsic::StackSave, &[PTR], PTR, &[], &[]);
    x.intrinsic(Intrinsic::StackRestore, &[PTR], VOID, &[PTR], &[saved]);
    x.intrinsic(Intrinsic::FrameAddress, &[PTR], PTR, &[I32], &[zero]);
    x.intrinsic(Intrinsic::ReturnAddress, &[], PTR, &[I32], &[zero]);
    x.intrinsic(Intrinsic::ThreadPointer, &[PTR], PTR, &[], &[]);
    // By name: an intrinsic hod has no member for, and one it has.
    x.intrinsic(Intrinsic::Other("llvm.donothing"), &[], VOID, &[], &[]);
    x.intrinsic(Intrinsic::Other("llvm.expect.i64"), &[], I64, &[I64, I64], &[n, n]);
    x.intrinsic(Intrinsic::Trap, &[], VOID, &[], &[]);
    x.ret_void();
    let body = x.done();
    all.push(body);

    for (name, intrinsic, since, construct) in [
        ("minimumnum", Intrinsic::MinimumNum, 20, "intrinsic:llvm.minimumnum"),
        ("maximumnum", Intrinsic::MaximumNum, 20, "intrinsic:llvm.maximumnum"),
        ("atan2", Intrinsic::Atan2, 20, "intrinsic:llvm.atan2"),
    ] {
        let mut x = all.body(name, DOUBLE, &[DOUBLE]);
        let d = x.arg(0);
        let result = x.intrinsic(intrinsic, &[DOUBLE], DOUBLE, &[DOUBLE, DOUBLE], &[d, d]);
        x.ret(result);
        let body = x.done();
        all.push_since(body, since, construct);
    }
    let mut x = all.body("clmul", I64, &[I64]);
    let n = x.arg(0);
    let result = x.intrinsic(Intrinsic::Clmul, &[I64], I64, &[I64, I64], &[n, n]);
    x.ret(result);
    let body = x.done();
    all.push_since(body, 22, "intrinsic:llvm.clmul");
}

/// `invoke`, `landingpad` with every kind of clause, `resume`, and
/// `callbr` of inline assembly.
fn unwinding(all: &mut Bodies) {
    let m = &mut all.m;
    let personality = declare(m, "__gxx_personality_v0", I32, &[], true);
    let may_throw = declare(m, "may_throw", I64, &[I64], false);
    let may_throw_ty = m.symbol(may_throw).ty();
    let type_info =
        m.add_variable(b"_ZTIi", &VariableDesc { constant: true, ..VariableDesc::new(PTR) }).unwrap();
    let pad_ty = m.types.struct_type(&[PTR, I32], false).unwrap();
    let info = m.consts.addr(type_info, 0).unwrap();
    let null = m.consts.null(&m.types, PTR).unwrap();
    let one = m.types.array(PTR, 1).unwrap();
    let none = m.types.array(PTR, 0).unwrap();
    let filter = m.consts.aggregate(&m.types, one, &[info]).unwrap();
    let empty_filter = m.consts.zero(&m.types, none).unwrap();
    let ty = m.types.function(I64, &[I64, PTR], false).unwrap();
    let desc =
        FunctionDesc { personality: Some(personality), has_body: true, ..FunctionDesc::new(ty) };
    let f = m.add_function(b"unwinding", &desc).unwrap();
    let nullary = m.types.function(VOID, &[], false).unwrap();
    let throwing_asm = m
        .inline_asm(&InlineAsm {
            template: b"bl may_throw",
            constraints: b"~{x30}",
            flags: InlineAsm::SIDE_EFFECT | InlineAsm::UNWIND,
            ty: nullary,
        })
        .unwrap();
    let unary = m.types.function(I64, &[I64], false).unwrap();

    let mut x = Fx::new(m, f);
    let (next, second, third, done) = (x.block(), x.block(), x.block(), x.block());
    let (cleanup, catch_all, clauses, filters) = (x.block(), x.block(), x.block(), x.block());
    let (n, target) = (x.arg(0), x.arg(1));
    let callee = Value::sym(may_throw).unwrap();
    let first = x.b.invoke(I64, &CallDesc::new(callee, may_throw_ty, &[n]), next, cleanup).unwrap();
    x.at(next);
    // An indirect invoke with a convention, attributes and a bundle.
    let signext = ParamAttrs::NONE.with(ParamAttrs::SIGNEXT).unwrap();
    let args = [first];
    let desc = CallDesc {
        cc: CallConv::FAST,
        ret_attrs: signext,
        arg_attrs: &[signext],
        nounwind: false,
        noreturn: true,
        bundles: &[Bundle { tag: "deopt", inputs: &[first] }],
        ..CallDesc::new(target, unary, &args)
    };
    x.b.invoke(I64, &desc, second, catch_all).unwrap();
    x.at(second);
    x.b.invoke(VOID, &CallDesc::new(Value::asm(throwing_asm).unwrap(), nullary, &[]), third, clauses)
        .unwrap();
    x.at(third);
    x.b.invoke(I64, &CallDesc::new(callee, may_throw_ty, &[n]), done, filters).unwrap();
    x.at(done);
    x.ret(first);
    x.at(cleanup);
    let pad = x.b.landing_pad(pad_ty, true, &[]).unwrap();
    x.b.resume(pad).unwrap();
    x.at(catch_all);
    x.b.landing_pad(pad_ty, false, &[Clause::Catch(null)]).unwrap();
    x.ret(n);
    x.at(clauses);
    let pad = x
        .b
        .landing_pad(pad_ty, true, &[Clause::Catch(info), Clause::Filter(filter), Clause::Catch(null)])
        .unwrap();
    x.b.resume(pad).unwrap();
    x.at(filters);
    x.b.landing_pad(pad_ty, false, &[Clause::Filter(empty_filter)]).unwrap();
    x.b.unreachable().unwrap();
    let body = x.done();
    all.push(body);

    // asm goto: `callbr` with one and with no indirect destination.
    let jump_ty = all.m.types.function(VOID, &[I64], false).unwrap();
    let jump = all
        .m
        .inline_asm(&InlineAsm {
            template: b"cbz $0, ${1:l}",
            constraints: b"r,!i",
            flags: InlineAsm::SIDE_EFFECT,
            ty: jump_ty,
        })
        .unwrap();
    let nop = all
        .m
        .inline_asm(&InlineAsm { template: b"nop", constraints: b"", flags: 0, ty: nullary })
        .unwrap();
    let mut x = all.body("asm_goto", I64, &[I64]);
    let (through, taken, end) = (x.block(), x.block(), x.block());
    x.b.callbr(VOID, &CallDesc::new(Value::asm(jump).unwrap(), jump_ty, &[x.arg(0)]), through, &[taken])
        .unwrap();
    x.at(through);
    x.b.callbr(VOID, &CallDesc::new(Value::asm(nop).unwrap(), nullary, &[]), end, &[]).unwrap();
    x.at(taken);
    let zero = x.i64(0);
    x.ret(zero);
    x.at(end);
    x.ret(x.arg(0));
    let body = x.done();
    all.push(body);
}

fn bodies() -> Bodies {
    let mut all = Bodies { m: Module::new(TargetSpec::aarch64_linux_gnu()), cases: Vec::new() };
    arithmetic(&mut all);
    comparisons(&mut all);
    casts(&mut all);
    memory(&mut all);
    aggregates(&mut all);
    control(&mut all);
    variadic(&mut all);
    calls(&mut all);
    intrinsics(&mut all);
    unwinding(&mut all);
    all.m.verify_symbols().unwrap();
    all
}

/// What the accepted bodies hold, for the coverage assertions.
#[derive(Default)]
struct Seen {
    opcodes: BTreeSet<Opcode>,
    int_predicates: BTreeSet<IntPredicate>,
    float_predicates: BTreeSet<FloatPredicate>,
    rmw_ops: BTreeSet<RmwOp>,
    tails: BTreeSet<TailKind>,
}

impl Seen {
    fn add(&mut self, body: &Body) {
        for inst in body.insts() {
            self.opcodes.insert(inst.opcode());
            match inst.kind() {
                InstKind::ICmp { pred, .. } => {
                    self.int_predicates.insert(pred);
                }
                InstKind::FCmp { pred, .. } => {
                    self.float_predicates.insert(pred);
                }
                InstKind::AtomicRmw { op, .. } => {
                    self.rmw_ops.insert(op);
                }
                _ => {}
            }
        }
        for call in body.calls() {
            self.tails.insert(call.tail());
        }
    }
}

/// §9.4, §9.5: the module of every instruction is accepted by `llvm-as`
/// and `opt -passes=verify` of LLVM 22 and LLVM 19, each given the bodies
/// its dialect has; and a body with something a dialect lacks is the
/// error that names it, in all four dialects.
#[test]
fn every_instruction_is_accepted_by_llvm() {
    let all = bodies();
    for dialect in Dialect::ALL {
        let major = dialect.major();
        let mut seen = Seen::default();
        let mut accepted = Vec::new();
        for case in &all.cases {
            if case.since <= major {
                seen.add(&case.body);
                accepted.push(&case.body);
                continue;
            }
            // Alone in the module, the body is the error.
            let mut text = String::new();
            let error = all.m.print(&mut text, dialect, [&case.body]).unwrap_err();
            let expected = PrintError::Unsupported {
                construct: case.construct.to_string(),
                symbol: Some(case.body.symbol()),
            };
            assert_eq!(error, expected, "LLVM {major}");
        }

        // Every member of each enumeration is in an accepted body, or is
        // one this dialect lacks.
        for &opcode in Opcode::ALL {
            assert_eq!(seen.opcodes.contains(&opcode), !opcode.is_funclet(), "{}", opcode.name());
        }
        for &pred in IntPredicate::ALL {
            assert!(seen.int_predicates.contains(&pred), "icmp {}", pred.name());
        }
        for &pred in FloatPredicate::ALL {
            assert!(seen.float_predicates.contains(&pred), "fcmp {}", pred.name());
        }
        for &op in RmwOp::ALL {
            assert_eq!(seen.rmw_ops.contains(&op), rmw_since(op) <= major, "{}", op.name());
        }
        for &tail in TailKind::ALL {
            assert!(seen.tails.contains(&tail), "{tail:?}");
        }

        let mut text = String::new();
        all.m.print(&mut text, dialect, accepted.iter().copied()).unwrap();
        // Dialects 20 and 21 are specified and not run (§9.2).
        if !hod_testutil::LLVM_MAJORS.contains(&major) {
            continue;
        }
        let Some(llvm) = Llvm::new(major) else { continue };
        if let Ok(dir) = std::env::var("HOD_WP006_DUMP") { std::fs::write(format!("{dir}/bodies{major}.ll"), &text).unwrap(); }
        llvm.accept(&text, "the module of every instruction");
        let instructions: usize = accepted.iter().map(|body| body.inst_count()).sum();
        eprintln!(
            "print_bodies: LLVM {major}: {} bodies, {instructions} instructions, {} opcodes, \
             {} bytes of text accepted and verified",
            accepted.len(),
            seen.opcodes.len(),
            text.len()
        );
    }
}

/// §9.5: the funclet instructions have no text. hod keeps none of their
/// operands, so each is the error `inst:<opcode>`.
#[test]
fn funclet_instructions_are_errors() {
    let mut count = 0;
    for &opcode in Opcode::ALL.iter().filter(|opcode| opcode.is_funclet()) {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let f = define(&mut m, "funclet", VOID, &[]);
        let ty = m.symbol(f).ty();
        let mut b = FunctionBuilder::new(&m, f, ty).unwrap();
        let entry = b.create_block().unwrap();
        b.begin_block(entry).unwrap();
        let result = if opcode == Opcode::CatchPad || opcode == Opcode::CleanupPad {
            TypeId::TOKEN
        } else {
            VOID
        };
        b.funclet(opcode, result).unwrap();
        if !opcode.is_terminator() {
            b.unreachable().unwrap();
        }
        let body = b.finish().unwrap();
        for dialect in Dialect::ALL {
            let mut text = String::new();
            let error = m.print(&mut text, dialect, [&body]).unwrap_err();
            let expected = PrintError::Unsupported {
                construct: format!("inst:{}", opcode.name()),
                symbol: Some(f),
            };
            assert_eq!(error, expected);
        }
        count += 1;
    }
    assert_eq!(count, 5);
}

/// §9.2: what each dialect does with `ptrtoaddr`, `llvm.thread.pointer`
/// and a tail marker on something that is not a `call`.
#[test]
fn dialects_differ_in_instructions_as_specified() {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let callee = declare(&mut m, "callee", VOID, &[], false);
    let callee_ty = m.symbol(callee).ty();
    let personality = declare(&mut m, "__gxx_personality_v0", I32, &[], true);

    let f = define(&mut m, "address", I64, &[PTR]);
    let mut x = Fx::new(&mut m, f);
    let address = x.cast(Opcode::PtrToAddr, I64, x.arg(0));
    x.intrinsic(Intrinsic::ThreadPointer, &[PTR], PTR, &[], &[]);
    x.ret(address);
    let address = x.done();
    for dialect in Dialect::ALL {
        let text = printed(&m, dialect, std::slice::from_ref(&address));
        // Before LLVM 22 an address is all of a pointer of address space
        // 0, and `ptrtoint` says the same.
        let (cast, pointer) = if dialect.major() >= 22 {
            ("%v0 = ptrtoaddr ptr %a0 to i64", "@llvm.thread.pointer.p0()")
        } else {
            ("%v0 = ptrtoint ptr %a0 to i64", "@llvm.thread.pointer()")
        };
        assert!(text.contains(cast), "{text}");
        assert!(text.contains(&format!("%v1 = call ptr {pointer}")), "{text}");
        assert!(text.contains(&format!("declare ptr {pointer}")), "{text}");
    }

    // LLVM has tail markers on `call` alone; `Body::verify` takes them
    // on `invoke` and `callbr` too.
    let ty = m.types.function(VOID, &[], false).unwrap();
    let desc =
        FunctionDesc { personality: Some(personality), has_body: true, ..FunctionDesc::new(ty) };
    let f = m.add_function(b"marked", &desc).unwrap();
    let pad_ty = m.types.struct_type(&[PTR, I32], false).unwrap();
    let mut x = Fx::new(&mut m, f);
    let (normal, pad) = (x.block(), x.block());
    let desc =
        CallDesc { tail: TailKind::Tail, ..CallDesc::new(Value::sym(callee).unwrap(), callee_ty, &[]) };
    x.b.invoke(VOID, &desc, normal, pad).unwrap();
    x.at(normal);
    x.ret_void();
    x.at(pad);
    let caught = x.b.landing_pad(pad_ty, true, &[]).unwrap();
    x.b.resume(caught).unwrap();
    let marked = x.done();
    for dialect in Dialect::ALL {
        let mut text = String::new();
        let error = m.print(&mut text, dialect, [&marked]).unwrap_err();
        let expected =
            PrintError::Unsupported { construct: "invoke:tail".to_string(), symbol: Some(f) };
        assert_eq!(error, expected);
    }
    let _: SymId = f;
}
