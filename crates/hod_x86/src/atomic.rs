//! Atomic memory operations on i386 (`spec/codegen-x86.md` §12; design
//! 0008 §5.2, I11b, WP-316): ordered `load` and `store`, `atomicrmw`,
//! `cmpxchg`, `fence`. The orderings are x86's TSO ones (as `hod_x64`).
//!
//! # 1, 2 and 4 bytes (`i8`, `i16`, `i32`, `ptr`, `float`)
//!
//! - **load**: `mov` (`movzx` below 4 bytes) for every ordering.
//! - **store**: `mov`; `xchg` for `seq_cst` (as `llc` 19 and 22).
//! - **`atomicrmw`**: `xchg` for `xchg`, `lock xadd` for `add` and `sub`
//!   (the operand negated); every other operation a `lock cmpxchg` loop:
//!   the old value in `eax`, the new one computed in `ecx` from the
//!   operand's copy in `edx`, `jne` back. The result is `movzx`'d from
//!   `al`/`ax`.
//! - **`cmpxchg`**: `lock cmpxchg` with the expected value in `eax`, the
//!   flag `sete`; a `weak` one is the same.
//!
//! # 8 bytes (`i64`, `double`)
//!
//! Values of class `Mem`, two words in a slot (§8).
//!
//! - **load**: `fild qword [p]` · `fistp qword [result]`, as `llc` writes
//!   it without SSE2 (an aligned 8-byte x87 access is single-copy
//!   atomic; the 64-bit integer survives the round trip exactly). With
//!   SSE2 `llc` uses `movq`; hod keeps the x87 pair (I12 may change it).
//! - **store**: `fild qword [value]` · `fistp qword [p]`, then the fence
//!   of a `seq_cst` store.
//! - **`atomicrmw`**, **`cmpxchg`**: `lock cmpxchg8b [p]` (loop): the old
//!   value in `edx:eax`, the new one in `ecx:ebx`. `ebx` is taken for the
//!   sequence (`FnCx::take_reg`); under PIC it is also the GOT pointer,
//!   which `pic::emit_call` computes again before every PLT call (§9.2).
//!
//! # `fence`
//!
//! `seq_cst`: `mfence` with SSE2, `lock or dword [esp], 0` without (as
//! `llc`); other orderings and `syncscope("singlethread")`: nothing.
//!
//! # Refused
//!
//! [`NO_CX8`]: an 8-byte operation on a CPU without `cmpxchg8b`
//! (`i386`, `i486`: `llc` calls libatomic's `__atomic_*_8`).
//! [`LIBATOMIC`]: a size other than 1, 2, 4, 8 bytes, or an access below
//! its natural alignment (`llc` calls libatomic's generic routines:
//! `hod_cg`'s runtime table has their 64-bit signatures only).
//! [`FLOAT_OP`]: `fmax`, `fmin` (and the other floating operations but
//! `fadd` and `fsub`, which are loops around an x87 operation, see
//! `rmw_float`): `llc` calls libm's `fmax`/`fmin` inside its loop. [`OP`]: `uinc_wrap`, `udec_wrap`, `usub_cond`,
//! `usub_sat`. [`TYPE`]: another type (`x86_fp80`, vectors, ...).

use hod_cg::{CgResult, FnCx, FrameRef, Label};
use hod_ir::{
    AtomicOrdering, FnStr, Inst, MemFlags, RmwOp, SymbolKind, SyncScope, TypeId, TypeKind, Value,
    X86Features,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Mem, Reg, Size, X87Size};

use crate::common::{
    EAX, ECX, EDX, ESP, branch_cond, frame_inst, gp, load_gpr, put, store_gpr, unsupported,
    value_ty,
};
use crate::target::X86;

/// An 8-byte atomic operation on a CPU without `cmpxchg8b`.
pub const NO_CX8: &str = "x86:atomic:no-cx8";
/// An atomic access that `llc` lowers with a libatomic call.
pub const LIBATOMIC: &str = "x86:atomic:libatomic";
/// A floating-point `atomicrmw` operation.
pub const FLOAT_OP: &str = "x86:atomic:float-op";
/// `atomicrmw` `uinc_wrap`, `udec_wrap`, `usub_cond`, `usub_sat`.
pub const OP: &str = "x86:atomic:op";
/// An atomic value of a type without a sequence here.
pub const TYPE: &str = "x86:atomic:type";

const EBX: Reg = Reg::Rbx;

/// An atomic value: its size in bytes, and whether it is a `Gpr` value.
#[derive(Clone, Copy, Debug)]
struct Shape {
    bytes: u32,
    gpr: bool,
    float: bool,
}

impl Shape {
    fn size(self) -> Size {
        match self.bytes {
            1 => Size::B,
            2 => Size::W,
            _ => Size::D,
        }
    }
}

/// The function's CPU features (as `x87.rs` reads them).
pub fn features(cx: &FnCx<'_, X86>) -> X86Features {
    let mcpu = cx.target.cpu();
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return X86Features::of_function(None, None, mcpu, &[]).unwrap_or(X86Features::NONE);
    };
    let attrs = cx.module.fn_attrs(attrs);
    let text = |what| attrs.string(what).and_then(|s| std::str::from_utf8(s).ok());
    X86Features::of_function(text(FnStr::TargetCpu), text(FnStr::TargetFeatures), mcpu, &[])
        .unwrap_or(X86Features::NONE)
}

/// The [`Shape`] of an atomic access of `ty` with `mem`'s alignment
/// (`align` 0: the access is aligned by the instruction's definition).
fn shape(cx: &FnCx<'_, X86>, ty: TypeId, align: u64) -> CgResult<Shape> {
    let types = cx.types();
    let float = matches!(types.kind(ty), TypeKind::Float | TypeKind::Double);
    let int = types.int_bits(ty).is_some() || matches!(types.kind(ty), TypeKind::Ptr { .. });
    if !float && !int {
        return unsupported(TYPE);
    }
    let bytes = types.size_of(ty);
    if !matches!(bytes, 1 | 2 | 4 | 8)
        || types.int_bits(ty).is_some_and(|b| u64::from(b) != 8 * bytes)
    {
        return unsupported(LIBATOMIC);
    }
    if align != 0 && align < bytes {
        return unsupported(LIBATOMIC);
    }
    if bytes == 8 && !features(cx).has(X86Features::CX8) {
        return unsupported(NO_CX8);
    }
    Ok(Shape { bytes: bytes as u32, gpr: crate::abi::is_gpr(types, ty), float })
}

/// `lock` `inst`.
fn locked(cx: &mut FnCx<'_, X86>, inst: mc::Inst) {
    put(cx, mc::lock(inst));
}

/// The pointer operand in a register.
fn pointer(cx: &mut FnCx<'_, X86>, ptr: Value) -> CgResult<Reg> {
    Ok(gp(cx.use_reg(ptr)?))
}

/// `movzx rd, al`/`ax` (or `mov`) after a sequence that left the old
/// value in `eax`.
fn from_eax(cx: &mut FnCx<'_, X86>, rd: Reg, size: Size) {
    match size {
        Size::B | Size::W => put(cx, mc::movzx_rr(Size::D, rd, size, EAX)),
        _ => put(cx, mc::mov_rr(Size::D, rd, EAX)),
    }
}

/// The fence after a `seq_cst` store and for `fence seq_cst`.
fn full_fence(cx: &mut FnCx<'_, X86>) {
    if features(cx).has(X86Features::SSE2) {
        put(cx, mc::mfence());
    } else {
        locked(cx, mc::or_mi(Size::D, Mem::base(ESP, 0), 0));
    }
}

/// `%r = load atomic <ty>, ptr %ptr <ordering>`.
///
/// # Errors
/// The refusals of [`shape`]; what the operands report.
pub fn load<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    let s = shape(cx, inst.ty(), mem.align())?;
    let p = pointer(cx, ptr)?;
    if s.gpr {
        let rd = cx.def_reg(inst)?;
        crate::common::load_at(cx, gp(rd), s.bytes, EDX, |k| Mem::base(p, k));
        return cx.finish(inst, rd);
    }
    let dst = cx.def_mem(inst)?;
    if s.bytes == 4 {
        put(cx, mc::mov_rm(Size::D, ECX, Mem::base(p, 0)));
        store_gpr(cx, ECX, dst, 4);
    } else {
        put(cx, mc::fild_m(Size::Q, Mem::base(p, 0)));
        frame_inst(cx, dst, |m| mc::fistp_m(Size::Q, m));
    }
    Ok(())
}

/// `store atomic <ty> %value, ptr %ptr <ordering>`.
///
/// # Errors
/// The refusals of [`shape`]; what the operands report.
pub fn store<'a>(
    cx: &mut FnCx<'a, X86>,
    _inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let s = shape(cx, value_ty(cx, value), mem.align())?;
    let seq_cst = mem.ordering() == Some(AtomicOrdering::SeqCst);
    if s.bytes == 8 {
        let v = cx.use_mem(value)?;
        let p = pointer(cx, ptr)?;
        frame_inst(cx, v, |m| mc::fild_m(Size::Q, m));
        put(cx, mc::fistp_m(Size::Q, Mem::base(p, 0)));
        if seq_cst {
            full_fence(cx);
        }
        return Ok(());
    }
    if s.gpr {
        let v = gp(cx.use_reg(value)?);
        put(cx, mc::mov_rr(Size::D, ECX, v));
    } else {
        let v = cx.use_mem(value)?;
        load_gpr(cx, ECX, v, 4);
    }
    let p = pointer(cx, ptr)?;
    let size = s.size();
    if seq_cst {
        put(cx, mc::xchg_mr(size, Mem::base(p, 0), ECX));
    } else {
        put(cx, mc::mov_mr(size, Mem::base(p, 0), ECX));
    }
    Ok(())
}

/// `fence [syncscope(...)] <ordering>`.
///
/// # Errors
/// None.
pub fn fence(cx: &mut FnCx<'_, X86>, ordering: AtomicOrdering, scope: SyncScope) -> CgResult<()> {
    if ordering == AtomicOrdering::SeqCst && scope != SyncScope::SingleThread {
        full_fence(cx);
    }
    Ok(())
}

/// The refusal of an operation no sequence here computes.
fn check_op(op: RmwOp, s: Shape) -> CgResult<()> {
    match op {
        RmwOp::Xchg => Ok(()),
        RmwOp::Add
        | RmwOp::Sub
        | RmwOp::And
        | RmwOp::Nand
        | RmwOp::Or
        | RmwOp::Xor
        | RmwOp::Max
        | RmwOp::Min
        | RmwOp::UMax
        | RmwOp::UMin
            if !s.float =>
        {
            Ok(())
        }
        RmwOp::FAdd | RmwOp::FSub if s.float => Ok(()),
        RmwOp::UIncWrap | RmwOp::UDecWrap | RmwOp::USubCond | RmwOp::USubSat => unsupported(OP),
        _ => unsupported(FLOAT_OP),
    }
}

/// `%r = atomicrmw <op> ptr %ptr, <ty> %value <ordering>`.
///
/// # Errors
/// The refusals of [`shape`] and [`check_op`]; what binding a label and
/// the operands report.
pub fn rmw<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    op: RmwOp,
    ptr: Value,
    value: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let s = shape(cx, inst.ty(), mem.align())?;
    check_op(op, s)?;
    if matches!(op, RmwOp::FAdd | RmwOp::FSub) {
        return rmw_float(cx, inst, op, ptr, value, s);
    }
    if s.bytes == 8 {
        return rmw_wide(cx, inst, op, ptr, value);
    }
    if !s.gpr {
        // `float` `xchg`: the bits through `ecx`.
        let v = cx.use_mem(value)?;
        load_gpr(cx, ECX, v, 4);
        let p = pointer(cx, ptr)?;
        put(cx, mc::xchg_mr(Size::D, Mem::base(p, 0), ECX));
        let dst = cx.def_mem(inst)?;
        store_gpr(cx, ECX, dst, 4);
        return Ok(());
    }
    let size = s.size();
    let v = gp(cx.use_reg(value)?);
    put(cx, mc::mov_rr(Size::D, EDX, v));
    let p = pointer(cx, ptr)?;
    let at = Mem::base(p, 0);
    match op {
        RmwOp::Xchg => {
            put(cx, mc::xchg_mr(size, at, EDX));
            put(cx, mc::mov_rr(Size::D, EAX, EDX));
        }
        RmwOp::Add | RmwOp::Sub => {
            if op == RmwOp::Sub {
                put(cx, mc::neg_r(Size::D, EDX));
            }
            locked(cx, mc::xadd_mr(size, at, EDX));
            put(cx, mc::mov_rr(Size::D, EAX, EDX));
        }
        _ => {
            crate::common::load_at(cx, EAX, s.bytes, ECX, |k| Mem::base(p, k));
            let again = cx.new_label();
            cx.bind(again)?;
            put(cx, mc::mov_rr(Size::D, ECX, EAX));
            new_value(cx, op, size)?;
            locked(cx, mc::cmpxchg_mr(size, at, ECX));
            branch_cond(cx, Cond::Ne, again)?;
        }
    }
    let rd = cx.def_reg(inst)?;
    from_eax(cx, gp(rd), size);
    cx.finish(inst, rd)
}

/// `atomicrmw fadd`/`fsub` of `float` (4 bytes) or `double` (8 bytes),
/// as `llc`'s loop (measured, 19 and 22, `-O0`): the old bits pushed,
/// `fld` at the type, `fadd`/`fsub` the operand's slot, `fstp` at the
/// type (the new value rounded to it, as `llc` stores it), the new bits
/// popped into `ecx` (`ecx:ebx` at 8 bytes), `lock cmpxchg` /
/// `lock cmpxchg8b`, `jne` back. The x87 stack is empty before and after;
/// `esp` moves inside the sequence only (the frame is `ebp`-based, §3).
fn rmw_float<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    op: RmwOp,
    ptr: Value,
    value: Value,
    s: Shape,
) -> CgResult<()> {
    let wide = s.bytes == 8;
    let (w, fp) = if wide { (X87Size::D, FpSize::D) } else { (X87Size::S, FpSize::S) };
    if wide {
        cx.take_reg(crate::reg::gpr(EBX))?;
    }
    let v = cx.use_mem(value)?;
    let p = pointer(cx, ptr)?;
    put(cx, mc::mov_rm(Size::D, EAX, Mem::base(p, 0)));
    if wide {
        put(cx, mc::mov_rm(Size::D, EDX, Mem::base(p, 4)));
    }
    let again = cx.new_label();
    cx.bind(again)?;
    if wide {
        put(cx, mc::push_r(EDX));
    }
    put(cx, mc::push_r(EAX));
    put(cx, mc::fld_m(w, Mem::base(ESP, 0)));
    if op == RmwOp::FAdd {
        frame_inst(cx, v, |m| mc::fadd_m(fp, m));
    } else {
        frame_inst(cx, v, |m| mc::fsub_m(fp, m));
    }
    put(cx, mc::fstp_m(w, Mem::base(ESP, 0)));
    if wide {
        put(cx, mc::pop_r(EBX));
        put(cx, mc::pop_r(ECX));
        locked(cx, mc::cmpxchg8b_m(Mem::base(p, 0)));
    } else {
        put(cx, mc::pop_r(ECX));
        locked(cx, mc::cmpxchg_mr(Size::D, Mem::base(p, 0), ECX));
    }
    branch_cond(cx, Cond::Ne, again)?;
    let dst = cx.def_mem(inst)?;
    store_gpr(cx, EAX, dst, 4);
    if wide {
        store_gpr(cx, EDX, dst.offset_by(4), 4);
    }
    Ok(())
}

/// `ecx` = `op(ecx, edx)` at `size` (the cmpxchg loop's body).
fn new_value(cx: &mut FnCx<'_, X86>, op: RmwOp, size: Size) -> CgResult<()> {
    match op {
        RmwOp::And => put(cx, mc::and_rr(Size::D, ECX, EDX)),
        RmwOp::Or => put(cx, mc::or_rr(Size::D, ECX, EDX)),
        RmwOp::Xor => put(cx, mc::xor_rr(Size::D, ECX, EDX)),
        RmwOp::Nand => {
            put(cx, mc::and_rr(Size::D, ECX, EDX));
            put(cx, mc::not_r(Size::D, ECX));
        }
        RmwOp::Max | RmwOp::Min | RmwOp::UMax | RmwOp::UMin => {
            // Keep the old value when it wins, else take the operand.
            let keep = match op {
                RmwOp::Max => Cond::G,
                RmwOp::Min => Cond::L,
                RmwOp::UMax => Cond::A,
                _ => Cond::B,
            };
            let done: Label = cx.new_label();
            put(cx, mc::cmp_rr(size, ECX, EDX));
            branch_cond(cx, keep, done)?;
            put(cx, mc::mov_rr(Size::D, ECX, EDX));
            cx.bind(done)?;
        }
        _ => return cx.bug("an atomicrmw operation without a loop body"),
    }
    Ok(())
}

/// The 8-byte `atomicrmw`: a `lock cmpxchg8b` loop, the operand read
/// from its slot `v` at every turn.
fn rmw_wide<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    op: RmwOp,
    ptr: Value,
    value: Value,
) -> CgResult<()> {
    cx.take_reg(crate::reg::gpr(EBX))?;
    let v = cx.use_mem(value)?;
    let p = pointer(cx, ptr)?;
    put(cx, mc::mov_rm(Size::D, EAX, Mem::base(p, 0)));
    put(cx, mc::mov_rm(Size::D, EDX, Mem::base(p, 4)));
    let again = cx.new_label();
    cx.bind(again)?;
    let lo = v;
    let hi = v.offset_by(4);
    let pair = |cx: &mut FnCx<'_, X86>,
                a: fn(Size, Reg, Mem) -> mc::Inst,
                b: fn(Size, Reg, Mem) -> mc::Inst| {
        put(cx, mc::mov_rr(Size::D, EBX, EAX));
        frame_inst(cx, lo, |m| a(Size::D, EBX, m));
        put(cx, mc::mov_rr(Size::D, ECX, EDX));
        frame_inst(cx, hi, |m| b(Size::D, ECX, m));
    };
    match op {
        RmwOp::Xchg => {
            load_gpr(cx, EBX, lo, 4);
            load_gpr(cx, ECX, hi, 4);
        }
        RmwOp::Add => pair(cx, mc::add_rm, mc::adc_rm),
        RmwOp::Sub => pair(cx, mc::sub_rm, mc::sbb_rm),
        RmwOp::And => pair(cx, mc::and_rm, mc::and_rm),
        RmwOp::Or => pair(cx, mc::or_rm, mc::or_rm),
        RmwOp::Xor => pair(cx, mc::xor_rm, mc::xor_rm),
        RmwOp::Nand => {
            pair(cx, mc::and_rm, mc::and_rm);
            put(cx, mc::not_r(Size::D, EBX));
            put(cx, mc::not_r(Size::D, ECX));
        }
        _ => {
            // The flags of `old - operand` over 64 bits, then the
            // winner: the old value (kept) or the operand.
            let keep = match op {
                RmwOp::Max => Cond::Ge,
                RmwOp::Min => Cond::L,
                RmwOp::UMax => Cond::Ae,
                _ => Cond::B,
            };
            pair(cx, mc::sub_rm, mc::sbb_rm);
            put(cx, mc::mov_rr(Size::D, EBX, EAX));
            put(cx, mc::mov_rr(Size::D, ECX, EDX));
            let done = cx.new_label();
            branch_cond(cx, keep, done)?;
            load_gpr(cx, EBX, lo, 4);
            load_gpr(cx, ECX, hi, 4);
            cx.bind(done)?;
        }
    }
    locked(cx, mc::cmpxchg8b_m(Mem::base(p, 0)));
    branch_cond(cx, Cond::Ne, again)?;
    let dst = cx.def_mem(inst)?;
    store_gpr(cx, EAX, dst, 4);
    store_gpr(cx, EDX, dst.offset_by(4), 4);
    Ok(())
}

/// `%r = cmpxchg [weak] ptr %ptr, <ty> %expected, <ty> %replacement
/// <success> <failure>`: the `{ <ty>, i1 }` result in its slot, the
/// value at offset 0 and the flag at the second field's offset.
///
/// # Errors
/// The refusals of [`shape`]; what the operands report.
pub fn cmpxchg<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    ptr: Value,
    expected: Value,
    replacement: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let ty = value_ty(cx, expected);
    let s = shape(cx, ty, mem.align())?;
    let flag = cx.types().field_offset(inst.ty(), 1) as u32;
    if s.bytes == 8 {
        cx.take_reg(crate::reg::gpr(EBX))?;
        let e = cx.use_mem(expected)?;
        let r = cx.use_mem(replacement)?;
        let p = pointer(cx, ptr)?;
        load_gpr(cx, EAX, e, 4);
        load_gpr(cx, EDX, e.offset_by(4), 4);
        load_gpr(cx, EBX, r, 4);
        load_gpr(cx, ECX, r.offset_by(4), 4);
        locked(cx, mc::cmpxchg8b_m(Mem::base(p, 0)));
        put(cx, mc::setcc_r(Cond::E, ECX));
        let dst = cx.def_mem(inst)?;
        store_gpr(cx, EAX, dst, 4);
        store_gpr(cx, EDX, dst.offset_by(4), 4);
        store_gpr(cx, ECX, dst.offset_by(flag), 1);
        return Ok(());
    }
    if s.gpr {
        let e = gp(cx.use_reg(expected)?);
        put(cx, mc::mov_rr(Size::D, EAX, e));
        let r = if replacement == expected { e } else { gp(cx.use_reg(replacement)?) };
        put(cx, mc::mov_rr(Size::D, ECX, r));
    } else {
        let e = cx.use_mem(expected)?;
        load_gpr(cx, EAX, e, 4);
        let r = cx.use_mem(replacement)?;
        load_gpr(cx, ECX, r, 4);
    }
    let p = pointer(cx, ptr)?;
    locked(cx, mc::cmpxchg_mr(s.size(), Mem::base(p, 0), ECX));
    put(cx, mc::setcc_r(Cond::E, EDX));
    let dst = cx.def_mem(inst)?;
    store_gpr(cx, EAX, dst, s.bytes);
    store_gpr(cx, EDX, dst.offset_by(flag), 1);
    Ok(())
}

/// The dispatch arm of `fence`, `cmpxchg` and `atomicrmw`.
///
/// # Errors
/// What [`fence`], [`cmpxchg`] and [`rmw`] report.
pub fn lower<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
    match inst.kind() {
        hod_ir::InstKind::Fence { ordering, scope } => fence(cx, ordering, scope),
        hod_ir::InstKind::CmpXchg { ptr, expected, replacement, mem, .. } => {
            cmpxchg(cx, inst, ptr, expected, replacement, mem)
        }
        hod_ir::InstKind::AtomicRmw { op, ptr, value, mem } => rmw(cx, inst, op, ptr, value, mem),
        _ => cx.bug("an instruction that is no fence, cmpxchg or atomicrmw"),
    }
}

/// Whether `llc` lowers the `atomicrmw` `inst` (operation `op`) with a
/// block boundary, an x87 barrier (§11, §12 "Barriers"). Measured in the
/// `-O0` assembly of `llc` 19 and 22 at CPU `generic` and `i686`: a
/// loop for every 8-byte operation (`lock cmpxchg8b`); at 1, 2 and 4
/// bytes for every operation except `xchg`, `add` and `sub`, and except
/// `and`, `or`, `xor` whose result is unused (`lock and`/`or`/`xor`, no
/// loop); the floating operations are loops. `cmpxchg` (strong or weak,
/// 8 bytes included), fences, ordered loads and stores have no branch.
pub fn barrier(cx: &FnCx<'_, X86>, inst: Inst<'_>, op: RmwOp) -> bool {
    if cx.types().size_of(inst.ty()) == 8 {
        return true;
    }
    match op {
        RmwOp::Xchg | RmwOp::Add | RmwOp::Sub => false,
        RmwOp::And | RmwOp::Or | RmwOp::Xor => !inst.uses().is_unused(),
        _ => true,
    }
}

/// Whether `agg` is the result of a `cmpxchg` (whose fields
/// [`extract`] reads).
pub fn is_cmpxchg_result(cx: &FnCx<'_, X86>, agg: Value) -> bool {
    match agg.kind() {
        hod_ir::ValueKind::Inst(id) => {
            matches!(cx.body.inst(id).kind(), hod_ir::InstKind::CmpXchg { .. })
        }
        _ => false,
    }
}

/// `%v = extractvalue { <ty>, i1 } %pair, <n>` of a `cmpxchg` result
/// (`offset` the field's byte offset).
///
/// # Errors
/// What the operands report.
pub fn extract<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    agg: Value,
    offset: u64,
) -> CgResult<()> {
    let src: FrameRef = cx.use_mem(agg)?.offset_by(offset as u32);
    let bytes = cx.types().size_of(inst.ty()) as u32;
    if crate::abi::is_gpr(cx.types(), inst.ty()) {
        let rd = cx.def_reg(inst)?;
        load_gpr(cx, gp(rd), src, bytes);
        return cx.finish(inst, rd);
    }
    let dst = cx.def_mem(inst)?;
    crate::common::copy_frame(cx, dst, src, bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_are_stable() {
        assert_eq!(super::NO_CX8, "x86:atomic:no-cx8");
        assert_eq!(super::LIBATOMIC, "x86:atomic:libatomic");
        assert_eq!(super::FLOAT_OP, "x86:atomic:float-op");
        assert_eq!(super::OP, "x86:atomic:op");
        assert_eq!(super::TYPE, "x86:atomic:type");
    }
}
