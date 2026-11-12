//! Atomic memory operations (`spec/codegen.md` §5.5.1 "Atomics"): atomic
//! `load` and `store`, `atomicrmw`, `cmpxchg`, `fence`.
//!
//! **Owner: WP-074.** Tests: `tests/atomic.rs`. `target.rs` sends every
//! atomic `load` and `store` here, whatever the value's class, and every
//! `fence`, `cmpxchg` and `atomicrmw`.
//!
//! # Sequences
//!
//! Which sequence an operation gets depends on the function's
//! `target-features` (the last `±outline-atomics` and `±lse` items):
//!
//! - **outline atomics** (`+outline-atomics` without `+lse`, what clang
//!   and kiln write for AArch64 Linux): where `llc` makes one call of a
//!   libgcc/compiler-rt helper, hod makes the same call:
//!   `__aarch64_cas{1,2,4,8,16}_<order>` for `cmpxchg` and for the
//!   `i128` atomic load, `__aarch64_{swp,ldadd,ldclr,ldeor,ldset}{1,2,4,8}_<order>`
//!   for `xchg`, `add`, `sub` (the operand negated), `and` (the operand
//!   inverted), `or`, `xor`. What `llc` expands into a loop around a
//!   `cas` call (the other operations, the floating-point ones, `i128`
//!   stores and read-modify-writes) is the exclusive loop below: both are
//!   one atomic read-modify-write of the location, and the architecture
//!   makes exclusives and the LSE instructions the helpers use atomic with
//!   respect to each other.
//! - **otherwise** (no feature, `-outline-atomics`, or `+lse`): load and
//!   store exclusive loops, always correct on ARMv8.0. `+lse` code from
//!   `llc` uses the LSE instructions inline; the loop is equivalent.
//!
//! An atomic `load`/`store` of up to 8 bytes is `ldar`/`stlr` when the
//! ordering acquires/releases, else a plain `ldr`/`str`, under every
//! feature set, as `llc` writes it. `fence` is `dmb ishld` for
//! `acquire`, `dmb ish` otherwise; `syncscope("singlethread")` fences emit
//! nothing.
//!
//! # The exclusive loops
//!
//! The load exclusive is `ldaxr*` when the ordering acquires (for
//! `cmpxchg`: the success or the failure ordering does), the store
//! exclusive `stlxr*` when it releases (a `seq_cst` failure ordering
//! counts). Every operand is put in a register before the loop label: the
//! loop is built of [`put`] alone, so no glue runs inside it, and neither
//! a spill, a reload, nor any other memory access sits between the
//! exclusive load and the exclusive store that could clear the monitor.
//! An `i128` operand is held in `v30`/`v31`, so that the four registers a
//! lowering may pin suffice. The `i128` load stores back what it read
//! (the pair is single-copy atomic only when the store exclusive
//! succeeds), and so does a failing `i128` `cmpxchg`; a failing narrower
//! `cmpxchg` clears the monitor with `clrex`. A `weak` `cmpxchg` is a
//! strong one (it never fails spuriously, which `LangRef` allows).
//!
//! # Refused
//!
//! [`UNALIGNED`]: an access whose alignment is below its size (LLVM calls
//! libatomic's `__atomic_*` for it). [`TYPE`]: `fp128`, vectors, integer
//! widths other than 8, 16, 32, 64 and 128 (LLVM's verifier allows only
//! these for an `atomicrmw`/`cmpxchg`; wider ones are libatomic calls).
//! [`BFLOAT_OP`]: floating-point arithmetic on `bfloat`.

use hod_cg::{CallArg, CgResult, Class, FnCx, FrameRef, PReg, RuntimeFn};
use hod_ir::{
    AtomicOrdering, FnStr, Inst, MemFlags, RmwOp, SymbolKind, SyncScope, TypeId, TypeKind, Value,
};
use hod_mc_a64 as mc;
use hod_mc_a64::{
    Barrier, Cond, Extend, FpFormat, FpSize, Load, MemSize, Reg, RegZr, Size, VMemSize, VReg,
};

use crate::common::{
    Cursor, V30, V31, X16, X17, branch, branch_cond, branch_zero, deferred_first, load_fpr_at,
    load_gpr_at, put, store_gpr_at, unsupported, value_ty, vr, xr,
};
use crate::target::A64;

/// An atomic access whose alignment is below its size.
pub const UNALIGNED: &str = "a64:atomic-unaligned";
/// An atomic access of a type hod has no sequence for: `fp128`, a
/// vector, an integer that is not 8, 16, 32, 64 or 128 bits wide.
pub const TYPE: &str = "a64:atomic-type";
/// `atomicrmw fadd` and the other floating-point operations on `bfloat`.
pub const BFLOAT_OP: &str = "a64:atomic-bfloat-op";

/// Every refusal of this module.
pub const REASONS: [&str; 3] = [UNALIGNED, TYPE, BFLOAT_OP];

// ---- shapes and orderings ----

/// A floating-point format of an atomic value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fp {
    H,
    B,
    S,
    D,
}

/// What an atomic value is, for the sequences.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    /// An integer or pointer of 1, 2, 4 or 8 bytes in a general register.
    Int(MemSize),
    /// A floating-point value in a vector register.
    Float(Fp),
    /// `i128`, of class `Mem`.
    Wide,
}

impl Fp {
    fn mem_size(self) -> MemSize {
        match self {
            Fp::H | Fp::B => MemSize::H,
            Fp::S => MemSize::W,
            Fp::D => MemSize::X,
        }
    }
}

impl Shape {
    fn bytes(self) -> u64 {
        match self {
            Shape::Int(ms) => 1 << ms.log2(),
            Shape::Float(fp) => 1 << fp.mem_size().log2(),
            Shape::Wide => 16,
        }
    }
}

/// The shape of a value of `ty` accessed with `mem`.
fn shape(cx: &FnCx<'_, A64>, ty: TypeId, mem: MemFlags) -> CgResult<Shape> {
    let shape = match cx.types().kind(ty) {
        TypeKind::Int(8) => Shape::Int(MemSize::B),
        TypeKind::Int(16) => Shape::Int(MemSize::H),
        TypeKind::Int(32) => Shape::Int(MemSize::W),
        TypeKind::Int(64) | TypeKind::Ptr { .. } => Shape::Int(MemSize::X),
        TypeKind::Int(128) => Shape::Wide,
        TypeKind::Half => Shape::Float(Fp::H),
        TypeKind::BFloat => Shape::Float(Fp::B),
        TypeKind::Float => Shape::Float(Fp::S),
        TypeKind::Double => Shape::Float(Fp::D),
        _ => return unsupported(TYPE),
    };
    if mem.align() < shape.bytes() {
        return unsupported(UNALIGNED);
    }
    Ok(shape)
}

fn acquires(o: AtomicOrdering) -> bool {
    matches!(o, AtomicOrdering::Acquire | AtomicOrdering::AcqRel | AtomicOrdering::SeqCst)
}

fn releases(o: AtomicOrdering) -> bool {
    matches!(o, AtomicOrdering::Release | AtomicOrdering::AcqRel | AtomicOrdering::SeqCst)
}

/// The barrier halves of an operation: acquire, release.
#[derive(Clone, Copy, Debug)]
struct Order {
    acq: bool,
    rel: bool,
}

impl Order {
    fn of(o: AtomicOrdering) -> Order {
        Order { acq: acquires(o), rel: releases(o) }
    }

    /// A `cmpxchg`'s: the success ordering merged with the failure one.
    fn merged(success: AtomicOrdering, failure: AtomicOrdering) -> Order {
        Order {
            acq: acquires(success) || acquires(failure),
            rel: releases(success) || failure == AtomicOrdering::SeqCst,
        }
    }

    /// The helper suffix's index: `relax`, `acq`, `rel`, `acq_rel`.
    fn index(self) -> usize {
        usize::from(self.acq) | usize::from(self.rel) << 1
    }

    fn load_exclusive(self, ms: MemSize, rt: Reg, rn: Reg) -> u32 {
        if self.acq { mc::ldaxr(ms, rt.zr(), rn.sp()) } else { mc::ldxr(ms, rt.zr(), rn.sp()) }
    }

    fn store_exclusive(self, ms: MemSize, rs: Reg, rt: RegZr, rn: Reg) -> u32 {
        if self.rel {
            mc::stlxr(ms, rs.zr(), rt, rn.sp())
        } else {
            mc::stxr(ms, rs.zr(), rt, rn.sp())
        }
    }

    fn load_pair(self, rt: Reg, rt2: Reg, rn: Reg) -> u32 {
        if self.acq {
            mc::ldaxp(Size::X, rt.zr(), rt2.zr(), rn.sp())
        } else {
            mc::ldxp(Size::X, rt.zr(), rt2.zr(), rn.sp())
        }
    }

    fn store_pair(self, rs: Reg, rt: Reg, rt2: Reg, rn: Reg) -> u32 {
        if self.rel {
            mc::stlxp(Size::X, rs.zr(), rt.zr(), rt2.zr(), rn.sp())
        } else {
            mc::stxp(Size::X, rs.zr(), rt.zr(), rt2.zr(), rn.sp())
        }
    }
}

/// Whether the function's `target-features` ask for outline atomics
/// without LSE: the last `±outline-atomics` and `±lse` items decide.
fn outline(cx: &FnCx<'_, A64>) -> bool {
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return false;
    };
    let Some(features) = cx.module.fn_attrs(attrs).string(FnStr::TargetFeatures) else {
        return false;
    };
    let (mut helpers, mut lse) = (false, false);
    for item in features.split(|&b| b == b',') {
        match item {
            b"+outline-atomics" => helpers = true,
            b"-outline-atomics" => helpers = false,
            b"+lse" => lse = true,
            b"-lse" => lse = false,
            _ => {}
        }
    }
    helpers && !lse
}

/// The register size an `ms` access computes in.
fn size_of(ms: MemSize) -> Size {
    if ms == MemSize::X { Size::X } else { Size::W }
}

fn load_op(ms: MemSize) -> Load {
    match ms {
        MemSize::B => Load::B,
        MemSize::H => Load::H,
        MemSize::W => Load::W,
        MemSize::X => Load::X,
    }
}

/// The `S`/`D` view an `fp`'s bits move through between registers.
fn fmov_size(fp: Fp) -> Size {
    if fp == Fp::D { Size::X } else { Size::W }
}

/// `value` in a register, or the register of an earlier operand that is
/// the same value (a register value may be asked for once).
fn operand(cx: &mut FnCx<'_, A64>, value: Value, earlier: &[(Value, PReg)]) -> CgResult<PReg> {
    match earlier.iter().find(|(v, _)| *v == value) {
        Some(&(_, reg)) => Ok(reg),
        None => cx.use_reg(value),
    }
}

/// `value`'s register given back before the instruction ends.
fn give_back(cx: &mut FnCx<'_, A64>, value: Value, reg: PReg) {
    cx.release(value);
    cx.free_temp(reg);
}

/// `rd` = the sub-word integer in `rn` zero-extended (with `signed`
/// sign-extended) to 32 bits; nothing for a word or doubleword.
fn extend(cx: &mut FnCx<'_, A64>, ms: MemSize, rd: Reg, rn: Reg, signed: bool) {
    let (d, n) = (rd.zr(), rn.zr());
    match (ms, signed) {
        (MemSize::B, false) => put(cx, mc::uxtb(d, n)),
        (MemSize::H, false) => put(cx, mc::uxth(d, n)),
        (MemSize::B, true) => put(cx, mc::sxtb(Size::W, d, n)),
        (MemSize::H, true) => put(cx, mc::sxth(Size::W, d, n)),
        _ => {}
    }
}

// ---- load, store, fence ----

/// `%r = load atomic <ty>, ptr %ptr <ordering>`.
///
/// # Errors
/// [`UNALIGNED`], [`TYPE`]; what asking for the operands reports.
pub fn load<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    let shape = shape(cx, inst.ty(), mem)?;
    let ordering = mem.ordering().unwrap_or(AtomicOrdering::SeqCst);
    deferred_first(cx, inst, None)?;
    let acq = acquires(ordering);
    let word = |ms: MemSize, rt: Reg, rn: Reg| {
        if acq {
            mc::ldar(ms, rt.zr(), rn.sp())
        } else {
            mc::ldr_off(load_op(ms), rt.zr(), rn.sp(), 0)
        }
    };
    match shape {
        Shape::Int(ms) => {
            let a = xr(cx.use_reg(ptr)?);
            let rd = cx.def_reg(inst)?;
            put(cx, word(ms, xr(rd), a));
            cx.finish(inst, rd)
        }
        Shape::Float(fp) => {
            let a = xr(cx.use_reg(ptr)?);
            put(cx, word(fp.mem_size(), X16, a));
            let rd = cx.def_reg(inst)?;
            put(cx, mc::fmov_gpr_to_fp(fmov_size(fp), vr(rd), X16.zr()));
            cx.finish(inst, rd)
        }
        Shape::Wide if outline(cx) => {
            // `llc`: `__aarch64_cas16_<order>(0, 0, ptr)`.
            let zero = cx.alloc_slot(16, 16)?;
            let mut at = Cursor::frame(cx, zero, X16);
            store_gpr_at(cx, MemSize::X, RegZr::ZR, &mut at);
            at.advance(8);
            store_gpr_at(cx, MemSize::X, RegZr::ZR, &mut at);
            let r = cx.def_mem(inst)?;
            let func = CAS[4][Order::of(ordering).index()];
            cx.call_runtime(
                func,
                &[CallArg::Mem(zero), CallArg::Mem(zero), CallArg::Value(ptr)],
                Some(r),
            )?;
            Ok(())
        }
        Shape::Wide => {
            let order = Order { acq, rel: ordering == AtomicOrdering::SeqCst };
            let a = xr(cx.use_reg(ptr)?);
            let lo = xr(cx.temp(Class::Gpr)?);
            let hi = xr(cx.temp(Class::Gpr)?);
            let again = cx.new_label();
            cx.bind(again)?;
            put(cx, order.load_pair(lo, hi, a));
            put(cx, order.store_pair(X17, lo, hi, a));
            branch_zero(cx, Size::W, X17, true, again)?;
            let r = cx.def_mem(inst)?;
            store_pair_to(cx, r, lo, hi);
            Ok(())
        }
    }
}

/// `lo`, `hi` to the 16 bytes at `slot`.
fn store_pair_to(cx: &mut FnCx<'_, A64>, slot: FrameRef, lo: Reg, hi: Reg) {
    let mut at = Cursor::frame(cx, slot, X16);
    store_gpr_at(cx, MemSize::X, lo.zr(), &mut at);
    at.advance(8);
    store_gpr_at(cx, MemSize::X, hi.zr(), &mut at);
}

/// The 16 bytes at `slot` to `v`.
fn load_q(cx: &mut FnCx<'_, A64>, slot: FrameRef, v: VReg) {
    let mut at = Cursor::frame(cx, slot, X16);
    load_fpr_at(cx, VMemSize::Q, v, &mut at);
}

/// `store atomic <ty> %value, ptr %ptr <ordering>`.
///
/// # Errors
/// [`UNALIGNED`], [`TYPE`]; what asking for the operands reports.
pub fn store<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let shape = shape(cx, value_ty(cx, value), mem)?;
    let ordering = mem.ordering().unwrap_or(AtomicOrdering::SeqCst);
    deferred_first(cx, inst, None)?;
    let rel = releases(ordering);
    let word = |ms: MemSize, rt: RegZr, rn: Reg| {
        if rel { mc::stlr(ms, rt, rn.sp()) } else { mc::str_off(ms, rt, rn.sp(), 0) }
    };
    match shape {
        Shape::Int(ms) => {
            let a = cx.use_reg(ptr)?;
            let v = operand(cx, value, &[(ptr, a)])?;
            put(cx, word(ms, xr(v).zr(), xr(a)));
        }
        Shape::Float(fp) => {
            let a = xr(cx.use_reg(ptr)?);
            let v = vr(cx.use_reg(value)?);
            put(cx, mc::fmov_fp_to_gpr(fmov_size(fp), X16.zr(), v));
            put(cx, word(fp.mem_size(), X16.zr(), a));
        }
        Shape::Wide => {
            // `ldxp` (discarded) · `st(l)xp` the value, until it succeeds.
            let src = cx.use_mem(value)?;
            let a = xr(cx.use_reg(ptr)?);
            let lo = xr(cx.temp(Class::Gpr)?);
            let hi = xr(cx.temp(Class::Gpr)?);
            let status = xr(cx.temp(Class::Gpr)?);
            let mut at = Cursor::frame(cx, src, X16);
            load_gpr_at(cx, Load::X, lo, &mut at);
            at.advance(8);
            load_gpr_at(cx, Load::X, hi, &mut at);
            let order = Order { acq: false, rel };
            let again = cx.new_label();
            cx.bind(again)?;
            put(cx, order.load_pair(X16, X17, a));
            put(cx, order.store_pair(status, lo, hi, a));
            branch_zero(cx, Size::W, status, true, again)?;
        }
    }
    Ok(())
}

/// `fence [syncscope("singlethread")] <ordering>`.
///
/// # Errors
/// None.
pub fn fence(cx: &mut FnCx<'_, A64>, ordering: AtomicOrdering, scope: SyncScope) -> CgResult<()> {
    if scope == SyncScope::SingleThread {
        return Ok(());
    }
    let barrier = if ordering == AtomicOrdering::Acquire { Barrier::Ishld } else { Barrier::Ish };
    put(cx, mc::dmb(barrier));
    Ok(())
}

// ---- atomicrmw ----

/// The helpers by size (log2) and order index.
type Helpers = [[RuntimeFn; 4]; 4];

const SWP: Helpers = {
    #[allow(clippy::enum_glob_use, reason = "a table of helper names")]
    use RuntimeFn::*;
    [
        [Swp1Relax, Swp1Acq, Swp1Rel, Swp1AcqRel],
        [Swp2Relax, Swp2Acq, Swp2Rel, Swp2AcqRel],
        [Swp4Relax, Swp4Acq, Swp4Rel, Swp4AcqRel],
        [Swp8Relax, Swp8Acq, Swp8Rel, Swp8AcqRel],
    ]
};
const LDADD: Helpers = {
    #[allow(clippy::enum_glob_use, reason = "a table of helper names")]
    use RuntimeFn::*;
    [
        [Ldadd1Relax, Ldadd1Acq, Ldadd1Rel, Ldadd1AcqRel],
        [Ldadd2Relax, Ldadd2Acq, Ldadd2Rel, Ldadd2AcqRel],
        [Ldadd4Relax, Ldadd4Acq, Ldadd4Rel, Ldadd4AcqRel],
        [Ldadd8Relax, Ldadd8Acq, Ldadd8Rel, Ldadd8AcqRel],
    ]
};
const LDCLR: Helpers = {
    #[allow(clippy::enum_glob_use, reason = "a table of helper names")]
    use RuntimeFn::*;
    [
        [Ldclr1Relax, Ldclr1Acq, Ldclr1Rel, Ldclr1AcqRel],
        [Ldclr2Relax, Ldclr2Acq, Ldclr2Rel, Ldclr2AcqRel],
        [Ldclr4Relax, Ldclr4Acq, Ldclr4Rel, Ldclr4AcqRel],
        [Ldclr8Relax, Ldclr8Acq, Ldclr8Rel, Ldclr8AcqRel],
    ]
};
const LDEOR: Helpers = {
    #[allow(clippy::enum_glob_use, reason = "a table of helper names")]
    use RuntimeFn::*;
    [
        [Ldeor1Relax, Ldeor1Acq, Ldeor1Rel, Ldeor1AcqRel],
        [Ldeor2Relax, Ldeor2Acq, Ldeor2Rel, Ldeor2AcqRel],
        [Ldeor4Relax, Ldeor4Acq, Ldeor4Rel, Ldeor4AcqRel],
        [Ldeor8Relax, Ldeor8Acq, Ldeor8Rel, Ldeor8AcqRel],
    ]
};
const LDSET: Helpers = {
    #[allow(clippy::enum_glob_use, reason = "a table of helper names")]
    use RuntimeFn::*;
    [
        [Ldset1Relax, Ldset1Acq, Ldset1Rel, Ldset1AcqRel],
        [Ldset2Relax, Ldset2Acq, Ldset2Rel, Ldset2AcqRel],
        [Ldset4Relax, Ldset4Acq, Ldset4Rel, Ldset4AcqRel],
        [Ldset8Relax, Ldset8Acq, Ldset8Rel, Ldset8AcqRel],
    ]
};
/// By size log2 (0 to 4) and order index.
const CAS: [[RuntimeFn; 4]; 5] = {
    #[allow(clippy::enum_glob_use, reason = "a table of helper names")]
    use RuntimeFn::*;
    [
        [Cas1Relax, Cas1Acq, Cas1Rel, Cas1AcqRel],
        [Cas2Relax, Cas2Acq, Cas2Rel, Cas2AcqRel],
        [Cas4Relax, Cas4Acq, Cas4Rel, Cas4AcqRel],
        [Cas8Relax, Cas8Acq, Cas8Rel, Cas8AcqRel],
        [Cas16Relax, Cas16Acq, Cas16Rel, Cas16AcqRel],
    ]
};

/// The helper of an operation `llc` lowers to one call, and how its
/// operand is transformed first: negated (`sub`), inverted (`and`).
fn helper(op: RmwOp) -> Option<(&'static Helpers, Option<bool>)> {
    Some(match op {
        RmwOp::Xchg => (&SWP, None),
        RmwOp::Add => (&LDADD, None),
        RmwOp::Sub => (&LDADD, Some(true)),
        RmwOp::And => (&LDCLR, Some(false)),
        RmwOp::Or => (&LDSET, None),
        RmwOp::Xor => (&LDEOR, None),
        _ => return None,
    })
}

/// `%r = atomicrmw <op> ptr %ptr, <ty> %value <ordering>`.
///
/// # Errors
/// [`UNALIGNED`], [`TYPE`], [`BFLOAT_OP`]; what asking for the operands
/// reports.
pub fn rmw<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    op: RmwOp,
    ptr: Value,
    value: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let shape = shape(cx, value_ty(cx, value), mem)?;
    let order = Order::of(mem.ordering().unwrap_or(AtomicOrdering::SeqCst));
    if op.is_float() && shape == Shape::Float(Fp::B) {
        return unsupported(BFLOAT_OP);
    }
    // Anything else is invalid IR (LLVM's verifier).
    let float_shape = matches!(shape, Shape::Float(_));
    if op.is_float() != float_shape && !(op == RmwOp::Xchg && float_shape) {
        return unsupported(TYPE);
    }
    deferred_first(cx, inst, None)?;
    let ms = match shape {
        Shape::Wide => return rmw_wide(cx, inst, op, ptr, value, order),
        Shape::Int(ms) => ms,
        Shape::Float(fp) => fp.mem_size(),
    };
    if let Some((table, transform)) = helper(op).filter(|_| outline(cx)) {
        let func = table[ms.log2() as usize][order.index()];
        return rmw_call(cx, inst, shape, ms, func, transform, ptr, value);
    }
    match shape {
        Shape::Float(fp) if op.is_float() => rmw_float(cx, inst, op, fp, ptr, value, order),
        Shape::Float(fp) => {
            // `xchg` of a float: its bits, through `x16`.
            let a = xr(cx.use_reg(ptr)?);
            let v = vr(cx.use_reg(value)?);
            put(cx, mc::fmov_fp_to_gpr(fmov_size(fp), X16.zr(), v));
            let old = xr(cx.temp(Class::Gpr)?);
            exclusive_loop(cx, ms, order, a, old, |_| X16.zr())?;
            let rd = cx.def_reg(inst)?;
            put(cx, mc::fmov_gpr_to_fp(fmov_size(fp), vr(rd), old.zr()));
            cx.finish(inst, rd)
        }
        _ => rmw_int(cx, inst, op, ms, ptr, value, order),
    }
}

/// One call of a helper: `old = helper(operand, ptr)`.
#[allow(clippy::too_many_arguments, reason = "the parts of one atomicrmw")]
fn rmw_call<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    shape: Shape,
    ms: MemSize,
    func: RuntimeFn,
    transform: Option<bool>,
    ptr: Value,
    value: Value,
) -> CgResult<()> {
    let t = cx.temp(Class::Gpr)?;
    let p = cx.temp(Class::Gpr)?;
    let rt = xr(t).zr();
    let sz = size_of(ms);
    let rp = cx.use_reg(ptr)?;
    put(cx, mc::mov(Size::X, xr(p).zr(), xr(rp).zr()));
    let v = operand(cx, value, &[(ptr, rp)])?;
    match (shape, transform) {
        (Shape::Float(fp), _) => put(cx, mc::fmov_fp_to_gpr(fmov_size(fp), rt, vr(v))),
        (_, Some(true)) => put(cx, mc::neg(sz, rt, xr(v).zr())),
        (_, Some(false)) => put(cx, mc::mvn(sz, rt, xr(v).zr())),
        (_, None) => put(cx, mc::mov(Size::X, rt, xr(v).zr())),
    }
    // Both are in the temporaries now: the call displaces nothing for
    // them.
    give_back(cx, ptr, rp);
    if value != ptr {
        give_back(cx, value, v);
    }
    let Some(old) = cx.call_runtime(func, &[CallArg::Reg(t), CallArg::Reg(p)], None)? else {
        return cx.bug("an atomic helper returned nothing");
    };
    match shape {
        Shape::Float(fp) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::fmov_gpr_to_fp(fmov_size(fp), vr(rd), xr(old).zr()));
            cx.free_temp(old);
            cx.finish(inst, rd)
        }
        _ => cx.finish(inst, old),
    }
}

/// `again: ld(a)xr old, [a]` · the new value (`compute` emits it and
/// names its register) · `st(l)xr w17, new, [a]` · `cbnz w17, again`.
/// `compute` emits words only, and may use `x16`.
fn exclusive_loop(
    cx: &mut FnCx<'_, A64>,
    ms: MemSize,
    order: Order,
    a: Reg,
    old: Reg,
    compute: impl FnOnce(&mut FnCx<'_, A64>) -> RegZr,
) -> CgResult<()> {
    let again = cx.new_label();
    cx.bind(again)?;
    put(cx, order.load_exclusive(ms, old, a));
    let new = compute(cx);
    put(cx, order.store_exclusive(ms, X17, new, a));
    branch_zero(cx, Size::W, X17, true, again)
}

/// An integer `atomicrmw` of up to 8 bytes as an exclusive loop.
#[allow(clippy::many_single_char_names, reason = "register names")]
fn rmw_int<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    op: RmwOp,
    ms: MemSize,
    ptr: Value,
    value: Value,
    order: Order,
) -> CgResult<()> {
    let pa = cx.use_reg(ptr)?;
    let pv = operand(cx, value, &[(ptr, pa)])?;
    let (a, v) = (xr(pa), xr(pv));
    let sz = size_of(ms);
    // The compares read the operand extended as the loaded value is
    // (`ldxr` zero-extends; the signed ones extend `old` in `x16`). A
    // pointer operand (`xchg`) is 8 bytes and never extended.
    let signed = matches!(op, RmwOp::Max | RmwOp::Min);
    let compares = signed
        || matches!(
            op,
            RmwOp::UMax
                | RmwOp::UMin
                | RmwOp::UIncWrap
                | RmwOp::UDecWrap
                | RmwOp::USubCond
                | RmwOp::USubSat
        );
    if compares {
        extend(cx, ms, v, v, signed);
    }
    let rd = cx.def_reg(inst)?;
    let old = xr(rd);
    let (o, w, n) = (old.zr(), v.zr(), X16.zr());
    exclusive_loop(cx, ms, order, a, old, |cx| {
        let simple = |f: fn(Size, RegZr, RegZr, RegZr) -> u32| f(sz, n, o, w);
        match op {
            RmwOp::Xchg => return w,
            RmwOp::Add => put(cx, simple(mc::add)),
            RmwOp::Sub => put(cx, simple(mc::sub)),
            RmwOp::And => put(cx, simple(mc::and)),
            RmwOp::Or => put(cx, simple(mc::orr)),
            RmwOp::Xor => put(cx, simple(mc::eor)),
            RmwOp::Nand => {
                put(cx, simple(mc::and));
                put(cx, mc::mvn(sz, n, n));
            }
            RmwOp::Max | RmwOp::Min | RmwOp::UMax | RmwOp::UMin => {
                let cond = match op {
                    RmwOp::Max => Cond::Gt,
                    RmwOp::Min => Cond::Lt,
                    RmwOp::UMax => Cond::Hi,
                    _ => Cond::Lo,
                };
                let lhs = if signed && sz == Size::W && ms != MemSize::W {
                    extend(cx, ms, X16, old, true);
                    n
                } else {
                    o
                };
                put(cx, mc::cmp(sz, lhs, w));
                put(cx, mc::csel(sz, n, lhs, w, cond));
            }
            RmwOp::UIncWrap => {
                put(cx, mc::add_imm(sz, X16.sp(), old.sp(), 1));
                put(cx, mc::cmp(sz, o, w));
                put(cx, mc::csel(sz, n, RegZr::ZR, n, Cond::Hs));
            }
            RmwOp::UDecWrap => {
                put(cx, mc::sub_imm(sz, X16.sp(), old.sp(), 1));
                put(cx, mc::cmp(sz, o, w));
                put(cx, mc::csel(sz, n, w, n, Cond::Hi));
                put(cx, mc::cmp_imm(sz, old.sp(), 0));
                put(cx, mc::csel(sz, n, w, n, Cond::Eq));
            }
            RmwOp::USubCond => {
                put(cx, mc::subs(sz, n, o, w));
                put(cx, mc::csel(sz, n, n, o, Cond::Hs));
            }
            RmwOp::USubSat => {
                put(cx, mc::subs(sz, n, o, w));
                put(cx, mc::csel(sz, n, n, RegZr::ZR, Cond::Hs));
            }
            // `rmw` refused it.
            _ => {}
        }
        n
    })?;
    cx.finish(inst, rd)
}

/// A floating-point `atomicrmw` as an exclusive loop over the bits,
/// computed in `v30` (`half` in single precision, exact for these
/// operations).
fn rmw_float<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    op: RmwOp,
    fp: Fp,
    ptr: Value,
    value: Value,
    order: Order,
) -> CgResult<()> {
    let a = xr(cx.use_reg(ptr)?);
    let v = vr(cx.use_reg(value)?);
    let fsz = if fp == Fp::D { FpSize::D } else { FpSize::S };
    if fp == Fp::H {
        put(cx, mc::fcvt(FpFormat::S, v, FpFormat::H, v));
    }
    let old = xr(cx.temp(Class::Gpr)?);
    let ms = fp.mem_size();
    let gsz = fmov_size(fp);
    exclusive_loop(cx, ms, order, a, old, |cx| {
        put(cx, mc::fmov_gpr_to_fp(gsz, V30, old.zr()));
        if fp == Fp::H {
            put(cx, mc::fcvt(FpFormat::S, V30, FpFormat::H, V30));
        }
        let f = match op {
            RmwOp::FAdd => mc::fadd,
            RmwOp::FSub => mc::fsub,
            RmwOp::FMax => mc::fmaxnm,
            RmwOp::FMin => mc::fminnm,
            RmwOp::FMaximum => mc::fmax,
            _ => mc::fmin,
        };
        put(cx, f(fsz, V30, V30, v));
        if fp == Fp::H {
            put(cx, mc::fcvt(FpFormat::H, V30, FpFormat::S, V30));
        }
        put(cx, mc::fmov_fp_to_gpr(gsz, X16.zr(), V30));
        X16.zr()
    })?;
    let rd = cx.def_reg(inst)?;
    put(cx, mc::fmov_gpr_to_fp(gsz, vr(rd), old.zr()));
    cx.finish(inst, rd)
}

/// An `i128` `atomicrmw` as an exclusive pair loop: the operand in
/// `v30`, moved to `x16`:`x17` on each pass and combined with the old
/// value there.
#[allow(clippy::many_single_char_names, reason = "register names")]
fn rmw_wide<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    op: RmwOp,
    ptr: Value,
    value: Value,
    order: Order,
) -> CgResult<()> {
    let src = cx.use_mem(value)?;
    let a = xr(cx.use_reg(ptr)?);
    let lo = xr(cx.temp(Class::Gpr)?);
    let hi = xr(cx.temp(Class::Gpr)?);
    let s = xr(cx.temp(Class::Gpr)?);
    // After every request: glue may use `v30`.
    load_q(cx, src, V30);
    let (x, y, ol, oh, z) = (X16.zr(), X17.zr(), lo.zr(), hi.zr(), RegZr::ZR);
    let again = cx.new_label();
    let skip = cx.new_label();
    cx.bind(again)?;
    put(cx, order.load_pair(lo, hi, a));
    put(cx, mc::fmov_fp_to_gpr(Size::X, x, V30));
    put(cx, mc::fmov_hi_to_gpr(y, V30));
    let pair = |cx: &mut FnCx<'_, A64>, f: fn(Size, RegZr, RegZr, RegZr) -> u32| {
        put(cx, f(Size::X, x, ol, x));
        put(cx, f(Size::X, y, oh, y));
    };
    // `old - v` in the flags: `hs`/`lo`, `ge`/`lt` hold, `eq` does not.
    let compare = |cx: &mut FnCx<'_, A64>| {
        put(cx, mc::cmp(Size::X, ol, x));
        put(cx, mc::sbcs(Size::X, z, oh, y));
    };
    let select = |cx: &mut FnCx<'_, A64>, a: RegZr, b: RegZr, c: RegZr, d: RegZr, cond| {
        put(cx, mc::csel(Size::X, x, a, b, cond));
        put(cx, mc::csel(Size::X, y, c, d, cond));
    };
    match op {
        RmwOp::Xchg => {}
        RmwOp::Add => {
            put(cx, mc::adds(Size::X, x, ol, x));
            put(cx, mc::adc(Size::X, y, oh, y));
        }
        RmwOp::Sub => {
            put(cx, mc::subs(Size::X, x, ol, x));
            put(cx, mc::sbc(Size::X, y, oh, y));
        }
        RmwOp::And => pair(cx, mc::and),
        RmwOp::Or => pair(cx, mc::orr),
        RmwOp::Xor => pair(cx, mc::eor),
        RmwOp::Nand => {
            pair(cx, mc::and);
            put(cx, mc::mvn(Size::X, x, x));
            put(cx, mc::mvn(Size::X, y, y));
        }
        RmwOp::Max | RmwOp::Min | RmwOp::UMax | RmwOp::UMin => {
            compare(cx);
            let cond = match op {
                RmwOp::Max => Cond::Ge,
                RmwOp::Min => Cond::Lt,
                RmwOp::UMax => Cond::Hs,
                _ => Cond::Lo,
            };
            select(cx, ol, x, oh, y, cond);
        }
        RmwOp::UIncWrap => {
            // old >= v ? 0 : old + 1
            compare(cx);
            put(cx, mc::cset(Size::W, s.zr(), Cond::Hs));
            put(cx, mc::adds_imm(Size::X, x, lo.sp(), 1));
            put(cx, mc::adc(Size::X, y, oh, z));
            put(cx, mc::cmp_imm(Size::W, s.sp(), 0));
            select(cx, x, z, y, z, Cond::Eq);
        }
        RmwOp::UDecWrap => {
            // (old == 0 || old > v) ? v : old - 1
            put(cx, mc::cmp(Size::X, x, ol));
            put(cx, mc::sbcs(Size::X, z, y, oh));
            put(cx, mc::cset(Size::W, s.zr(), Cond::Lo));
            put(cx, mc::cmp_imm(Size::X, lo.sp(), 0));
            put(cx, mc::ccmp_imm(Size::X, oh, 0, 0, Cond::Eq));
            put(cx, mc::csinc(Size::W, s.zr(), s.zr(), z, Cond::Ne));
            branch_zero(cx, Size::W, s, true, skip)?;
            put(cx, mc::subs_imm(Size::X, x, lo.sp(), 1));
            put(cx, mc::sbc(Size::X, y, oh, z));
        }
        RmwOp::USubCond | RmwOp::USubSat => {
            put(cx, mc::subs(Size::X, x, ol, x));
            put(cx, mc::sbcs(Size::X, y, oh, y));
            if op == RmwOp::USubCond {
                select(cx, x, ol, y, oh, Cond::Hs);
            } else {
                select(cx, x, z, y, z, Cond::Hs);
            }
        }
        _ => return cx.bug("a floating-point atomicrmw of i128"),
    }
    cx.bind(skip)?;
    put(cx, order.store_pair(s, X16, X17, a));
    branch_zero(cx, Size::W, s, true, again)?;
    let r = cx.def_mem(inst)?;
    store_pair_to(cx, r, lo, hi);
    Ok(())
}

// ---- cmpxchg ----

/// `%r = cmpxchg [weak] ptr %ptr, <ty> %expected, <ty> %replacement
/// <success> <failure>`: the `{ <ty>, i1 }` result in its slot.
///
/// # Errors
/// [`UNALIGNED`], [`TYPE`]; what asking for the operands reports.
#[allow(clippy::too_many_arguments, reason = "the parts of one cmpxchg")]
pub fn cmpxchg<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    ptr: Value,
    expected: Value,
    replacement: Value,
    mem: MemFlags,
    failure: AtomicOrdering,
    weak: bool,
) -> CgResult<()> {
    // A weak `cmpxchg` is a strong one.
    let _ = weak;
    let shape = shape(cx, value_ty(cx, expected), mem)?;
    let ms = match shape {
        Shape::Int(ms) => ms,
        Shape::Wide => MemSize::X,
        // LLVM's verifier allows only integers and pointers.
        Shape::Float(_) => return unsupported(TYPE),
    };
    let order = Order::merged(mem.ordering().unwrap_or(AtomicOrdering::SeqCst), failure);
    let Ok(flag_at) = u32::try_from(cx.types().field_offset(inst.ty(), 1)) else {
        return unsupported(hod_cg::LIMIT_FRAME_SIZE);
    };
    deferred_first(cx, inst, None)?;
    match (shape, outline(cx)) {
        (Shape::Wide, true) => {
            cmpxchg_wide_call(cx, inst, ptr, expected, replacement, order, flag_at)
        }
        (Shape::Wide, false) => cmpxchg_wide(cx, inst, ptr, expected, replacement, order, flag_at),
        (_, true) => cmpxchg_call(cx, inst, ms, ptr, expected, replacement, order, flag_at),
        (_, false) => cmpxchg_loop(cx, inst, ms, ptr, expected, replacement, order, flag_at),
    }
}

/// The flag in `w`, the old value's `bytes` from `old` (a register) to
/// the result slot.
fn finish_pair<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    ms: MemSize,
    old: Reg,
    flag: Reg,
    flag_at: u32,
) -> CgResult<()> {
    let r = cx.def_mem(inst)?;
    let tmp = if flag == X17 { X16 } else { X17 };
    let mut at = Cursor::frame(cx, r, tmp);
    store_gpr_at(cx, ms, old.zr(), &mut at);
    let mut at = Cursor::frame(cx, r.offset_by(flag_at), tmp);
    store_gpr_at(cx, MemSize::B, flag.zr(), &mut at);
    Ok(())
}

/// Up to 8 bytes as an exclusive loop.
#[allow(clippy::too_many_arguments, reason = "the parts of one cmpxchg")]
fn cmpxchg_loop<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    ms: MemSize,
    ptr: Value,
    expected: Value,
    replacement: Value,
    order: Order,
    flag_at: u32,
) -> CgResult<()> {
    let pa = cx.use_reg(ptr)?;
    let pe = operand(cx, expected, &[(ptr, pa)])?;
    let pn = operand(cx, replacement, &[(ptr, pa), (expected, pe)])?;
    let (a, e, n) = (xr(pa), xr(pe), xr(pn));
    // `ldxr` zero-extends; so must the expected value. (Should it be the
    // replacement's register too, its stored low bits are unchanged.)
    extend(cx, ms, e, e, false);
    let old = xr(cx.temp(Class::Gpr)?);
    let sz = size_of(ms);
    let (again, fail, done) = (cx.new_label(), cx.new_label(), cx.new_label());
    cx.bind(again)?;
    put(cx, order.load_exclusive(ms, old, a));
    put(cx, mc::cmp(sz, old.zr(), e.zr()));
    branch_cond(cx, Cond::Ne, fail)?;
    put(cx, order.store_exclusive(ms, X17, n.zr(), a));
    branch_zero(cx, Size::W, X17, true, again)?;
    branch(cx, done)?;
    cx.bind(fail)?;
    put(cx, mc::clrex());
    cx.bind(done)?;
    put(cx, mc::cset(Size::W, X16.zr(), Cond::Eq));
    finish_pair(cx, inst, ms, old, X16, flag_at)
}

/// Up to 8 bytes through `__aarch64_cas<N>_<order>(expected,
/// replacement, ptr)`; the expected value is kept in a slot across the
/// call and compared with what it returns.
#[allow(clippy::too_many_arguments, reason = "the parts of one cmpxchg")]
fn cmpxchg_call<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    ms: MemSize,
    ptr: Value,
    expected: Value,
    replacement: Value,
    order: Order,
    flag_at: u32,
) -> CgResult<()> {
    let keep = cx.alloc_slot(8, 8)?;
    // Each operand is copied to a temporary and given back at once, so
    // that at most four registers are pinned; a repeated operand is
    // copied from the earlier temporary.
    let values = [expected, replacement, ptr];
    let mut temps = [PReg::new(0); 3];
    for i in 0..3 {
        let t = cx.temp(Class::Gpr)?;
        temps[i] = t;
        if let Some(j) = (0..i).find(|&j| values[j] == values[i]) {
            put(cx, mc::mov(Size::X, xr(t).zr(), xr(temps[j]).zr()));
        } else {
            let r = cx.use_reg(values[i])?;
            put(cx, mc::mov(Size::X, xr(t).zr(), xr(r).zr()));
            give_back(cx, values[i], r);
        }
    }
    let mut at = Cursor::frame(cx, keep, X16);
    store_gpr_at(cx, MemSize::X, xr(temps[0]).zr(), &mut at);
    let args = temps.map(CallArg::Reg);
    let func = CAS[ms.log2() as usize][order.index()];
    let Some(old) = cx.call_runtime(func, &args, None)? else {
        return cx.bug("an atomic helper returned nothing");
    };
    let old = xr(old);
    let mut at = Cursor::frame(cx, keep, X16);
    load_gpr_at(cx, load_op(ms), X16, &mut at);
    // The loaded copy is zero-extended; the helper's result is compared
    // at its width.
    match ms {
        MemSize::B => put(cx, mc::cmp_ext(Size::W, X16.sp(), old.zr(), Extend::Uxtb, 0)),
        MemSize::H => put(cx, mc::cmp_ext(Size::W, X16.sp(), old.zr(), Extend::Uxth, 0)),
        _ => put(cx, mc::cmp(size_of(ms), X16.zr(), old.zr())),
    }
    put(cx, mc::cset(Size::W, X16.zr(), Cond::Eq));
    finish_pair(cx, inst, ms, old, X16, flag_at)
}

/// `i128` as an exclusive pair loop, the expected value in `v30` and the
/// replacement in `v31`; a mismatch stores the old value back (as `llc`
/// does) so that what was read is one atomic read.
#[allow(clippy::too_many_arguments, reason = "the parts of one cmpxchg")]
#[allow(clippy::many_single_char_names, reason = "register names")]
fn cmpxchg_wide<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    ptr: Value,
    expected: Value,
    replacement: Value,
    order: Order,
    flag_at: u32,
) -> CgResult<()> {
    let e = cx.use_mem(expected)?;
    let n = cx.use_mem(replacement)?;
    let a = xr(cx.use_reg(ptr)?);
    let lo = xr(cx.temp(Class::Gpr)?);
    let hi = xr(cx.temp(Class::Gpr)?);
    let s = xr(cx.temp(Class::Gpr)?);
    // After every request: glue may use `v30` and `v31`.
    load_q(cx, e, V30);
    load_q(cx, n, V31);
    let (x, y) = (X16.zr(), X17.zr());
    let (again, fail, done) = (cx.new_label(), cx.new_label(), cx.new_label());
    cx.bind(again)?;
    put(cx, order.load_pair(lo, hi, a));
    put(cx, mc::fmov_fp_to_gpr(Size::X, x, V30));
    put(cx, mc::fmov_hi_to_gpr(y, V30));
    put(cx, mc::cmp(Size::X, lo.zr(), x));
    put(cx, mc::ccmp(Size::X, hi.zr(), y, 0, Cond::Eq));
    branch_cond(cx, Cond::Ne, fail)?;
    put(cx, mc::fmov_fp_to_gpr(Size::X, x, V31));
    put(cx, mc::fmov_hi_to_gpr(y, V31));
    put(cx, order.store_pair(s, X16, X17, a));
    branch_zero(cx, Size::W, s, true, again)?;
    branch(cx, done)?;
    cx.bind(fail)?;
    put(cx, order.store_pair(s, lo, hi, a));
    branch_zero(cx, Size::W, s, true, again)?;
    cx.bind(done)?;
    put(cx, mc::cset(Size::W, s.zr(), Cond::Eq));
    let r = cx.def_mem(inst)?;
    store_pair_to(cx, r, lo, hi);
    let mut at = Cursor::frame(cx, r.offset_by(flag_at), X16);
    store_gpr_at(cx, MemSize::B, s.zr(), &mut at);
    Ok(())
}

/// `i128` through `__aarch64_cas16_<order>`, the old value returned
/// into the result slot and compared with the expected one there.
#[allow(clippy::too_many_arguments, reason = "the parts of one cmpxchg")]
fn cmpxchg_wide_call<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    ptr: Value,
    expected: Value,
    replacement: Value,
    order: Order,
    flag_at: u32,
) -> CgResult<()> {
    let e = cx.use_mem(expected)?;
    let n = cx.use_mem(replacement)?;
    let r = cx.def_mem(inst)?;
    let func = CAS[4][order.index()];
    cx.call_runtime(func, &[CallArg::Mem(e), CallArg::Mem(n), CallArg::Value(ptr)], Some(r))?;
    let t = xr(cx.temp(Class::Gpr)?);
    let mut at = Cursor::frame(cx, r, X16);
    load_gpr_at(cx, Load::X, t, &mut at);
    let mut at = Cursor::frame(cx, e, X16);
    load_gpr_at(cx, Load::X, X17, &mut at);
    put(cx, mc::eor(Size::X, t.zr(), t.zr(), X17.zr()));
    let mut at = Cursor::frame(cx, r.offset_by(8), X16);
    load_gpr_at(cx, Load::X, X17, &mut at);
    let mut at = Cursor::frame(cx, e.offset_by(8), X16);
    load_gpr_at(cx, Load::X, X16, &mut at);
    put(cx, mc::eor(Size::X, X17.zr(), X17.zr(), X16.zr()));
    put(cx, mc::orr(Size::X, t.zr(), t.zr(), X17.zr()));
    put(cx, mc::cmp_imm(Size::X, t.sp(), 0));
    put(cx, mc::cset(Size::W, t.zr(), Cond::Eq));
    let mut at = Cursor::frame(cx, r.offset_by(flag_at), X16);
    store_gpr_at(cx, MemSize::B, t.zr(), &mut at);
    Ok(())
}
