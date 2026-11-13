//! `x86_fp80` (C `long double`) through the x87 unit: WP-131
//! (`spec/codegen.md` §5.6.12).
//!
//! `x86_fp80` is class `Mem`: a 16-byte slot of which 10 bytes are the
//! value. Every lowering here loads its operands onto the x87 stack from
//! their slots (`fld tbyte`), operates, and stores the result to its slot
//! (`fstp tbyte`) or to a register: **the x87 stack is empty between two
//! IR instructions**, and at every call. Its loads, stores, `phi`s and
//! `freeze` are `aggr`'s copies of the 10 bytes; a `select` too, unless
//! its condition is an `x86_fp80` compare to fuse ([`select`]).
//!
//! What sets `x86_fp80` apart from the scalar group (`float`):
//!
//! - **Memory below the stack pointer.** A conversion or a constant that
//!   needs a few bytes of memory (the control word, an integer for
//!   `fild`/`fistp`, a `float` bound) takes 16 bytes below `rsp` by `lea
//!   rsp, [rsp - 16]` and gives them back by `lea rsp, [rsp + 16]` (no red
//!   zone; `lea` keeps the flags).
//! - **Truncation.** `fptosi`/`fptoui` set the rounding control to
//!   "toward zero" around `fistp` (`fnstcw`, `or 0xC00`, `fldcw`, `fistp`,
//!   `fldcw`), as `llc` does; `lrint`/`llrint` use the current mode.
//! - **Sign operations** (`fneg`, `fabs`, `copysign`) work on byte 9 of
//!   the image with integer instructions: bit-exact for every encoding,
//!   the signalling NaNs and the x87-only ones included.
//! - **Calls.** The `long double` routines of libm (`fmodl`, `sinl`, ...)
//!   are declared in `hod_cg::sym` with the `fp128` signatures of
//!   AArch64's `long double`, which `call_runtime` would place in vector
//!   registers. They are called here by [`libcall`]: every register value
//!   displaced, each `x86_fp80` argument copied into a 16-byte stack slot
//!   (`spec/abi-x64.md` §3: class `X87` is passed in memory), the result
//!   taken from `st0` (or `rax`). The routines whose signatures do name
//!   `x86_fp80` (`__extendhfxf2`, `__fixxfti`, `__powixf2`, ...) go
//!   through `call_runtime`, whose glue moves the `st0`/`st1` parts with
//!   [`load_part`] and [`store_part`].
//!
//! Refused, by name: [`BFLOAT`], [`CAST`], [`INTRINSIC`]; integers wider
//! than 128 bits as `aggr::WIDE_FP_CAST`.

use hod_cg::sym::{
    FpOp, FpType, fp_ext_routine, fp_routine, fp_to_int_routine, fp_trunc_routine,
    int_to_fp_routine,
};
use hod_cg::{CallArg, CallTarget, CgResult, FnCx, FrameRef, PReg, RuntimeFn, SymRef};
use hod_ir::{
    Arch, BlockId, Const, FloatPredicate, Inst, InstKind, Intrinsic, Opcode, TypeId, TypeKind,
    Value,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Mem, Reg, Size, St, X87Size};

use crate::aggr::{self, Place};
use crate::common::{
    R10, R11, RSP, branch, branch_cond, copy_frame, deferred_first, frame_inst, gp, mov_imm, put,
    store_fpr, unsupported, value_ty, xm,
};
use crate::float::{FpCond, ucomis_conditions};
use crate::reg::{ST0, is_x87};
use crate::target::X64;

/// `fptrunc x86_fp80 to bfloat`: `llc` 22 calls `__truncxfbf2`, which
/// the routine table does not have, and `llc` 19 cannot select it.
pub const BFLOAT: &str = "x64:x87-bfloat";
/// A cast with an `x86_fp80` side that is no conversion between it and a
/// scalar (invalid IR, or an `fpext` the wrong way).
pub const CAST: &str = "x64:x87-cast";
/// A floating-point intrinsic of `x86_fp80` in a form this group does
/// not lower: an exponent of `powi` or `ldexp` other than `i32`, a
/// result of the wrong shape.
pub const INTRINSIC: &str = "x64:x87-intrinsic";

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;
const RDI: Reg = Reg::Rdi;

/// The bytes of an `x86_fp80` value: the 64-bit significand, then the
/// sign and the 15-bit exponent.
const BYTES: u32 = 10;
/// The byte that holds the sign (bit 7).
const SIGN_BYTE: u32 = 9;
/// The memory below the stack pointer a lowering takes.
const BELOW: i32 = 16;

/// `2^63` as a `float`.
const TWO_63_F32: i64 = 0x5f00_0000;
/// `2^64` as a `float`.
const TWO_64_F32: i64 = 0x5f80_0000;

// ---- the stack and the frame ----

/// `fld tbyte [place]`: pushes the value as it is (no conversion, no
/// exception for a signalling NaN).
fn fld(cx: &mut FnCx<'_, X64>, place: FrameRef) {
    frame_inst(cx, place, |m| mc::fld_m(X87Size::T, m));
}

/// `fstp tbyte [place]`: stores `st0` as it is and pops.
fn fstp(cx: &mut FnCx<'_, X64>, place: FrameRef) {
    frame_inst(cx, place, |m| mc::fstp_m(X87Size::T, m));
}

/// `[rsp + disp]`.
fn sp(disp: i32) -> Mem {
    Mem::base(RSP, disp)
}

/// Takes [`BELOW`] bytes below the stack pointer (flags kept).
fn below(cx: &mut FnCx<'_, X64>) {
    put(cx, mc::lea(Size::Q, RSP, sp(-BELOW)));
}

/// Gives the [`BELOW`] bytes back (flags kept).
fn above(cx: &mut FnCx<'_, X64>) {
    put(cx, mc::lea(Size::Q, RSP, sp(BELOW)));
}

/// `mov dword [rsp + disp], bits` then `fld dword [rsp + disp]`: pushes
/// the `float` of `bits` (flags kept).
fn fld_f32(cx: &mut FnCx<'_, X64>, disp: i32, bits: i64) {
    put(cx, mc::mov_mi(Size::D, sp(disp), i64::from(bits as u32 as i32)));
    put(cx, mc::fld_m(X87Size::S, sp(disp)));
}

/// Glue (`Target::load` of `st0`/`st1`): the `x86_fp80` part at `src`
/// pushed for a return. The parts come in offset order, `st0` first: the
/// second is pushed and exchanged under the first.
pub fn load_part(cx: &mut FnCx<'_, X64>, reg: PReg, src: FrameRef) {
    fld(cx, src);
    if reg != ST0 {
        put(cx, mc::fxch(St::St1));
    }
}

/// Glue (`Target::store` of `st0`/`st1`): the part on top of the x87
/// stack popped to `dst`. The parts of a call's result come in offset
/// order: `st0`, then `st1`, which is the top once `st0` is popped.
pub fn store_part(cx: &mut FnCx<'_, X64>, reg: PReg, dst: FrameRef) {
    debug_assert!(is_x87(reg));
    fstp(cx, dst);
}

/// Whether `ty` is `x86_fp80`.
pub fn is_x86_fp80(cx: &FnCx<'_, X64>, ty: TypeId) -> bool {
    matches!(cx.types().kind(ty), TypeKind::X86Fp80)
}

// ---- arithmetic ----

/// `%r = <op> x86_fp80 %lhs, %rhs`: both pushed, `faddp`/`fsubp`/`fmulp`/
/// `fdivp` into `st1` (`st1 = st1 <op> st0`), the result popped to its
/// slot. `frem` calls `fmodl`, as `llc` does.
///
/// # Errors
/// [`CAST`] for an integer opcode (invalid IR); what the call reports.
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let op = match inst.opcode() {
        Opcode::FAdd => mc::faddp(St::St1),
        Opcode::FSub => mc::fsubp(St::St1),
        Opcode::FMul => mc::fmulp(St::St1),
        Opcode::FDiv => mc::fdivp(St::St1),
        Opcode::FRem => {
            deferred_first(cx, inst, None)?;
            let r = cx.def_mem(inst)?;
            return libcall(cx, RuntimeFn::Fmodl, &[lhs, rhs], Extra::None, Some(r)).map(|_| ());
        }
        _ => return unsupported(CAST),
    };
    deferred_first(cx, inst, None)?;
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let r = cx.def_mem(inst)?;
    fld(cx, a);
    fld(cx, b);
    put(cx, op);
    fstp(cx, r);
    Ok(())
}

/// Copies the value at `src` to `dst` (10 bytes).
fn copy(cx: &mut FnCx<'_, X64>, dst: FrameRef, src: FrameRef) {
    if dst != src {
        copy_frame(cx, dst, src, BYTES);
    }
}

/// `inst`'s result: `arg` with its sign byte changed by `op` (`xor`,
/// `and` with `imm`).
fn sign_op<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    op: fn(Size, Mem, i64) -> mc::Inst,
    imm: i64,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let a = cx.use_mem(arg)?;
    let r = cx.def_mem(inst)?;
    copy(cx, r, a);
    frame_inst(cx, r.offset_by(SIGN_BYTE), |m| op(Size::B, m, imm));
    Ok(())
}

/// `%r = fneg x86_fp80 %arg`: the image with its sign bit flipped.
///
/// # Errors
/// What the operand reports.
pub fn fneg<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    sign_op(cx, inst, arg, mc::xor_mi, -0x80)
}

// ---- compares (§4.6) ----

/// Whether `compare` is an `fcmp` of `x86_fp80` operands.
pub fn compares(cx: &FnCx<'_, X64>, compare: Inst<'_>) -> bool {
    matches!(compare.kind(), InstKind::FCmp { lhs, .. } if is_x86_fp80(cx, value_ty(cx, lhs)))
}

/// Performs the `x86_fp80` `fcmp` `compare` into the flags and returns
/// the condition(s) under which it holds, those of `ucomisd`
/// ([`ucomis_conditions`]): the right operand pushed, then the left one,
/// `fucomip st0, st1` (`ZF`, `PF`, `CF` as `ucomisd left, right` sets
/// them), `fstp st0`, which writes no flag. `true` and `false` are `cmp
/// eax, eax`. The x87 stack is empty again.
///
/// # Errors
/// `Internal` if `compare` is not an `fcmp`; what the operands report.
pub fn flags<'a>(cx: &mut FnCx<'a, X64>, compare: Inst<'a>) -> CgResult<FpCond> {
    let InstKind::FCmp { pred, lhs, rhs } = compare.kind() else {
        return cx.bug("flags of an instruction that is not an fcmp");
    };
    let (swap, cond) = ucomis_conditions(pred);
    if matches!(pred, FloatPredicate::True | FloatPredicate::False) {
        put(cx, mc::cmp_rr(Size::D, RAX, RAX));
        return Ok(cond);
    }
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let (x, y) = if swap { (b, a) } else { (a, b) };
    fld(cx, y);
    fld(cx, x);
    put(cx, mc::fucomip(St::St1));
    put(cx, mc::fstp_st(St::St0));
    Ok(cond)
}

/// `%r = fcmp <pred> x86_fp80 %lhs, %rhs`: deferred when the next
/// instruction consumes it (a `br` or an `x86_fp80` `select` fuses it),
/// else [`flags`] and `setcc` (two and an `and`/`or` for `oeq`, `une`).
///
/// # Errors
/// What [`flags`] reports.
pub fn fcmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (pred, lhs, rhs);
    if cx.may_defer(inst) {
        return cx.defer(inst);
    }
    deferred_first(cx, inst, None)?;
    let cond = flags(cx, inst)?;
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    match cond {
        FpCond::One(c) => put(cx, mc::setcc_r(c, r)),
        FpCond::All(c1, c2) | FpCond::Any(c1, c2) => {
            put(cx, mc::setcc_r(c1, r));
            put(cx, mc::setcc_r(c2, RCX));
            let both = matches!(cond, FpCond::All(..));
            put(cx, if both { mc::and_rr(Size::B, r, RCX) } else { mc::or_rr(Size::B, r, RCX) });
        }
    }
    put(cx, mc::movzx_rr(Size::D, r, Size::B, r));
    cx.finish(inst, rd)
}

/// `br i1 %cond, ...` on the deferred `x86_fp80` compare `compare` (the
/// hook in `float::cond_br`): [`flags`], then `jcc` on its condition(s),
/// the block emitted next not branched to.
///
/// # Errors
/// What [`flags`] and the branches' fixups report.
pub fn cond_br<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    compare: Inst<'a>,
    cond: Value,
    if_true: BlockId,
    if_false: BlockId,
) -> CgResult<()> {
    deferred_first(cx, inst, Some(cond))?;
    let c = flags(cx, compare)?;
    let last = match c {
        FpCond::One(c) => c,
        FpCond::All(c1, c2) => {
            branch_cond(cx, c1.invert(), cx.block_label(if_false))?;
            c2
        }
        FpCond::Any(c1, c2) => {
            branch_cond(cx, c1, cx.block_label(if_true))?;
            c2
        }
    };
    if cx.falls_through(if_true) {
        return branch_cond(cx, last.invert(), cx.block_label(if_false));
    }
    branch_cond(cx, last, cx.block_label(if_true))?;
    crate::term::br(cx, if_false)
}

/// The deferred `x86_fp80` compare a `select` of `x86_fp80` fuses (the
/// hook in `aggr::select`), if `cond` is one.
pub fn fusable<'a>(cx: &FnCx<'a, X64>, inst: Inst<'a>, cond: Value) -> Option<Inst<'a>> {
    if !is_x86_fp80(cx, inst.ty()) {
        return None;
    }
    cx.deferred(cond).filter(|&compare| compares(cx, compare))
}

/// `%r = select i1 %cond, x86_fp80 %a, x86_fp80 %b` whose condition is
/// the deferred `x86_fp80` compare `compare` ([`fusable`]): [`flags`],
/// the chosen operand's address by `cmov` (the true one's, replaced by
/// the false one's unless the condition holds), then one copy.
///
/// # Errors
/// What [`flags`] and the operands report.
pub fn select<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    compare: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, Some(cond))?;
    let r = cx.def_mem(inst)?;
    let a = cx.use_mem(if_true)?;
    let b = cx.use_mem(if_false)?;
    let c = flags(cx, compare)?;
    // `rax` = the address kept unless a `cmov` replaces it by `rcx`'s.
    let (keep, other) = if matches!(c, FpCond::Any(..)) { (b, a) } else { (a, b) };
    crate::common::frame_address(cx, RAX, keep);
    crate::common::frame_address(cx, RCX, other);
    let conds = match c {
        FpCond::One(c) => [Some(c.invert()), None],
        FpCond::All(c1, c2) => [Some(c1.invert()), Some(c2.invert())],
        FpCond::Any(c1, c2) => [Some(c1), Some(c2)],
    };
    for c in conds.into_iter().flatten() {
        put(cx, mc::cmovcc_rr(c, Size::Q, RAX, RCX));
    }
    aggr::copy_place(cx, Place::Frame(r), Place::Ptr(RAX), BYTES);
    Ok(())
}

// ---- conversions ----

/// What a cast's side is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    X87,
    Half,
    BFloat,
    Float,
    Double,
    Quad,
    Int(u32),
    Other,
}

/// The side of type `ty`.
fn side(cx: &FnCx<'_, X64>, ty: TypeId) -> Side {
    match cx.types().kind(ty) {
        TypeKind::X86Fp80 => Side::X87,
        TypeKind::Half => Side::Half,
        TypeKind::BFloat => Side::BFloat,
        TypeKind::Float => Side::Float,
        TypeKind::Double => Side::Double,
        TypeKind::Fp128 => Side::Quad,
        TypeKind::Int(bits) => Side::Int(bits),
        _ => Side::Other,
    }
}

/// `%r = <cast> %arg` with an `x86_fp80` side.
///
/// - `bitcast`: `aggr`'s (the image, between slots or a register).
/// - `fpext` from `float`/`double`: the value stored into the result's
///   slot, `fld dword`/`qword` from there, `fstp tbyte`; `bfloat`
///   through its `float` (shifted left 16); `half` by `__extendhfxf2`.
/// - `fptrunc` to `float`/`double`: `fld tbyte`, `fstp dword`/`qword`
///   below `rsp` (rounded by the control word: to nearest), loaded into
///   a vector register; `half` by `__truncxfhf2`; `bfloat` is [`BFLOAT`].
/// - with `fp128`: `__extendxftf2`, `__trunctfxf2`.
/// - to and from integers of up to 64 bits: [`to_int`], [`from_int`];
///   65 to 128 bits: `__fix[uns]xfti`, `__float[un]tixf`.
///
/// # Errors
/// [`BFLOAT`]; [`CAST`] for what is no conversion;
/// `aggr::WIDE_FP_CAST` for an integer wider than 128 bits; what the
/// operands and the calls report.
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    use Side as K;
    let op = inst.opcode();
    if op == Opcode::BitCast {
        return aggr::cast(cx, inst, arg);
    }
    let (from, to) = (value_ty(cx, arg), inst.ty());
    let (fk, tk) = (side(cx, from), side(cx, to));
    match (op, fk, tk) {
        (Opcode::FPExt, K::Float | K::Double | K::BFloat, K::X87) => {
            deferred_first(cx, inst, None)?;
            let x = xm(cx.use_reg(arg)?);
            let r = cx.def_mem(inst)?;
            let size = if fk == K::Double { X87Size::D } else { X87Size::S };
            match fk {
                K::Double => store_fpr(cx, x, r, 8),
                K::Float => store_fpr(cx, x, r, 4),
                _ => {
                    put(cx, mc::movd_rx(Size::D, RAX, x));
                    put(cx, mc::shl_ri(Size::D, RAX, 16));
                    frame_inst(cx, r, |m| mc::mov_mr(Size::D, m, RAX));
                }
            }
            frame_inst(cx, r, |m| mc::fld_m(size, m));
            fstp(cx, r);
            Ok(())
        }
        (Opcode::FPExt, K::Half, K::X87) | (Opcode::FPTrunc, K::Quad, K::X87) => {
            let func = if fk == K::Half {
                fp_ext_routine(FpType::Half, FpType::X86Fp80, Arch::X86_64)
            } else {
                fp_trunc_routine(FpType::Fp128, FpType::X86Fp80, Arch::X86_64)
            };
            let Some(func) = func else { return cx.bug("no x86_fp80 conversion routine") };
            deferred_first(cx, inst, None)?;
            let r = cx.def_mem(inst)?;
            cx.call_runtime(func, &[CallArg::Value(arg)], Some(r))?;
            Ok(())
        }
        (Opcode::FPTrunc, K::X87, K::Float | K::Double) => {
            deferred_first(cx, inst, None)?;
            let a = cx.use_mem(arg)?;
            let rd = cx.def_reg(inst)?;
            let (size, fp) =
                if tk == K::Double { (X87Size::D, FpSize::D) } else { (X87Size::S, FpSize::S) };
            fld(cx, a);
            below(cx);
            put(cx, mc::fstp_m(size, sp(0)));
            put(cx, mc::movs_rm(fp, xm(rd), sp(0)));
            above(cx);
            cx.finish(inst, rd)
        }
        (Opcode::FPTrunc, K::X87, K::Half) | (Opcode::FPExt, K::X87, K::Quad) => {
            let func = if tk == K::Half {
                fp_trunc_routine(FpType::X86Fp80, FpType::Half, Arch::X86_64)
            } else {
                fp_ext_routine(FpType::X86Fp80, FpType::Fp128, Arch::X86_64)
            };
            let Some(func) = func else { return cx.bug("no x86_fp80 conversion routine") };
            deferred_first(cx, inst, None)?;
            let Some(r) = cx.call_runtime(func, &[CallArg::Value(arg)], None)? else {
                return cx.bug("a runtime function returned nothing");
            };
            cx.finish(inst, r)
        }
        (Opcode::FPTrunc, K::X87, K::BFloat) => unsupported(BFLOAT),
        (Opcode::FPToSI | Opcode::FPToUI, K::X87, K::Int(bits)) => {
            to_int(cx, inst, arg, bits, op == Opcode::FPToSI)
        }
        (Opcode::SIToFP | Opcode::UIToFP, K::Int(bits), K::X87) => {
            from_int(cx, inst, arg, bits, op == Opcode::SIToFP)
        }
        _ => unsupported(CAST),
    }
}

/// The routine of a conversion between `x86_fp80` and an integer of 65
/// to 128 bits, or the refusal of a wider one.
fn wide_routine(bits: u32, to_int: bool, signed: bool) -> CgResult<RuntimeFn> {
    let func = if bits > 128 {
        None
    } else if to_int {
        fp_to_int_routine(FpType::X86Fp80, signed, bits, Arch::X86_64)
    } else {
        int_to_fp_routine(signed, bits, FpType::X86Fp80, Arch::X86_64)
    };
    func.map_or_else(|| unsupported(aggr::WIDE_FP_CAST), Ok)
}

/// `fptosi`/`fptoui` of an `x86_fp80` to `iN`. Up to 64 bits: pushed,
/// truncated ([`truncate`]) into a register (the bits above `N`
/// unspecified; out of range is poison). Above: `__fix[uns]xfti` into the
/// result's slot.
fn to_int<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    bits: u32,
    signed: bool,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if bits > 64 {
        let func = wide_routine(bits, true, signed)?;
        let r = cx.def_mem(inst)?;
        cx.call_runtime(func, &[CallArg::Value(arg)], Some(r))?;
        return Ok(());
    }
    let a = cx.use_mem(arg)?;
    let rd = cx.def_reg(inst)?;
    fld(cx, a);
    below(cx);
    truncate(cx, gp(rd), signed || bits < 64)?;
    above(cx);
    cx.finish(inst, rd)
}

/// `st0` truncated toward zero to a 64-bit integer in `rd`, popped, with
/// the [`BELOW`] bytes taken: the control word saved at `[rsp + 8]`, a
/// copy with the rounding control "toward zero" (`or 0xC00`) at `[rsp +
/// 10]` loaded, `fistp qword [rsp]`, the saved one loaded back. Unless
/// `signed` (`fptoui` to 64 bits: the value may be `2^63` or more), the
/// value is first compared with `2^63` and, from there on, less `2^63`
/// (exact), and bit 63 of the result flipped after (`llc`'s sequence,
/// with a branch for `fcmov`). Writes `rax`, `r11` and the flags.
fn truncate(cx: &mut FnCx<'_, X64>, rd: Reg, signed: bool) -> CgResult<()> {
    if !signed {
        let small = cx.new_label();
        fld_f32(cx, 0, TWO_63_F32);
        put(cx, mc::fxch(St::St1));
        put(cx, mc::fucomi(St::St1));
        put(cx, mc::setcc_r(Cond::Ae, R11));
        branch_cond(cx, Cond::B, small)?;
        put(cx, mc::fsub_0i(St::St1));
        cx.bind(small)?;
        put(cx, mc::fstp_st(St::St1));
    }
    put(cx, mc::fnstcw_m(sp(8)));
    put(cx, mc::movzx_rm(Size::D, RAX, Size::W, sp(8)));
    put(cx, mc::or_ri(Size::D, RAX, 0xc00));
    put(cx, mc::mov_mr(Size::W, sp(10), RAX));
    put(cx, mc::fldcw_m(sp(10)));
    put(cx, mc::fistp_m(Size::Q, sp(0)));
    put(cx, mc::fldcw_m(sp(8)));
    put(cx, mc::mov_rm(Size::Q, rd, sp(0)));
    if !signed {
        put(cx, mc::movzx_rr(Size::D, R11, Size::B, R11));
        put(cx, mc::shl_ri(Size::Q, R11, 63));
        put(cx, mc::xor_rr(Size::Q, rd, R11));
    }
    Ok(())
}

/// `sitofp`/`uitofp` of `iN` to `x86_fp80`. Up to 64 bits: the integer
/// extended to 64 (unsigned below 64 bits is non-negative), stored into
/// the result's slot, `fild qword` (exact), and for 64 bits unsigned with
/// bit 63 set `2^64` added (exact: the sum has 64 significant bits), then
/// `fstp tbyte` over it. Above: the integer extended to 128 bits in the
/// result's slot, `__float[un]tixf` of it into the same slot.
fn from_int<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    bits: u32,
    signed: bool,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if bits > 64 {
        let func = wide_routine(bits, false, signed)?;
        let s = cx.use_mem(arg)?;
        let r = cx.def_mem(inst)?;
        for limb in 0..2 {
            aggr::ld_ext(cx, R10, s, bits, limb, signed);
            aggr::st(cx, R10, r, limb);
        }
        cx.call_runtime(func, &[CallArg::Mem(r)], Some(r))?;
        return Ok(());
    }
    let a = gp(cx.use_reg(arg)?);
    let r = cx.def_mem(inst)?;
    put(cx, mc::mov_rr(Size::Q, R11, a));
    aggr::extend_reg(cx, R11, bits, signed);
    frame_inst(cx, r, |m| mc::mov_mr(Size::Q, m, R11));
    frame_inst(cx, r, |m| mc::fild_m(Size::Q, m));
    if !signed && bits == 64 {
        let done = cx.new_label();
        let high = r.offset_by(8);
        frame_inst(cx, high, |m| mc::mov_mi(Size::D, m, TWO_64_F32));
        put(cx, mc::test_rr(Size::Q, R11, R11));
        branch_cond(cx, Cond::Ns, done)?;
        frame_inst(cx, high, |m| mc::fadd_m(FpSize::S, m));
        cx.bind(done)?;
    }
    fstp(cx, r);
    Ok(())
}

// ---- intrinsics ----

/// The routine operation of an intrinsic that is a call on `x86_fp80`.
fn routine_op(which: Intrinsic<'_>) -> Option<FpOp> {
    use FpOp as O;
    use Intrinsic as I;
    Some(match which {
        I::Sin => O::Sin,
        I::Cos => O::Cos,
        I::Tan => O::Tan,
        I::Asin => O::Asin,
        I::Acos => O::Acos,
        I::Atan => O::Atan,
        I::Atan2 => O::Atan2,
        I::Sinh => O::Sinh,
        I::Cosh => O::Cosh,
        I::Tanh => O::Tanh,
        I::Exp => O::Exp,
        I::Exp2 => O::Exp2,
        I::Exp10 => O::Exp10,
        I::Log => O::Log,
        I::Log2 => O::Log2,
        I::Log10 => O::Log10,
        I::Pow => O::Pow,
        I::Floor => O::Floor,
        I::Ceil => O::Ceil,
        I::Trunc => O::Trunc,
        I::Round => O::Round,
        I::RoundEven => O::RoundEven,
        I::NearbyInt => O::NearbyInt,
        I::Rint => O::Rint,
        I::Fma => O::Fma,
        I::MinNum => O::MinNum,
        I::MaxNum => O::MaxNum,
        I::Minimum => O::Minimum,
        I::Maximum => O::Maximum,
        I::MinimumNum => O::MinimumNum,
        I::MaximumNum => O::MaximumNum,
        I::Lround => O::Lround,
        I::Llround => O::Llround,
        _ => return None,
    })
}

/// The width of `inst`'s integer result (1 to 64), or [`INTRINSIC`].
fn int_result(cx: &FnCx<'_, X64>, inst: Inst<'_>) -> CgResult<u32> {
    match cx.types().kind(inst.ty()) {
        TypeKind::Int(bits) if (1..=64).contains(&bits) => Ok(bits),
        _ => unsupported(INTRINSIC),
    }
}

/// An `i32` operand, or [`INTRINSIC`].
fn int32(cx: &FnCx<'_, X64>, n: Value) -> CgResult<Value> {
    if cx.types().int_bits(value_ty(cx, n)) == Some(32) { Ok(n) } else { unsupported(INTRINSIC) }
}

/// A call of floating-point intrinsic `which` whose first operand is an
/// `x86_fp80` (the hook in `fpintr::intrinsic`).
///
/// - Inline: `fabs`, `copysign` (byte 9 of the image), `sqrt` (`fsqrt`),
///   `canonicalize` (`fmul` by 1, as `llc`), `arithmetic.fence` (a copy),
///   `fmuladd` (`fmul`, then `fadd`: unfused, as `llc`), `lrint` and
///   `llrint` (`fistp` in the current rounding mode), `fptosi.sat` and
///   `fptoui.sat` ([`saturate`]), `is.fpclass` ([`fp_class`]).
/// - Calls: `powi` (`__powixf2`, through `call_runtime`); the libm
///   functions, the roundings, `fma`, the minima and maxima, `ldexp`,
///   `frexp`, `lround`, `llround` (`sinl`, `floorl`, `fmal`, `fminl`,
///   `fminimuml`, `ldexpl`, `frexpl`, `lroundl`, ...: [`libcall`]).
///
/// # Errors
/// [`INTRINSIC`] for another form; what the operands and calls report.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    deferred_first(cx, inst, None)?;
    if let Some(op) = routine_op(which) {
        let Some(func) = fp_routine(op, FpType::X86Fp80, Arch::X86_64) else {
            return unsupported(INTRINSIC);
        };
        let arity = match op {
            FpOp::Fma => 3,
            FpOp::Atan2
            | FpOp::Pow
            | FpOp::MinNum
            | FpOp::MaxNum
            | FpOp::Minimum
            | FpOp::Maximum
            | FpOp::MinimumNum
            | FpOp::MaximumNum => 2,
            _ => 1,
        };
        if args.len() != arity {
            return unsupported(INTRINSIC);
        }
        if matches!(op, FpOp::Lround | FpOp::Llround) {
            int_result(cx, inst)?;
            libcall(cx, func, args, Extra::None, None)?;
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mov_rr(Size::Q, gp(rd), RAX));
            return cx.finish(inst, rd);
        }
        let r = cx.def_mem(inst)?;
        return libcall(cx, func, args, Extra::None, Some(r)).map(|_| ());
    }
    match (which, args) {
        (I::Fabs, &[x]) => sign_op(cx, inst, x, mc::and_mi, 0x7f),
        (I::CopySign, &[x, y]) => {
            let a = cx.use_mem(x)?;
            let b = cx.use_mem(y)?;
            let r = cx.def_mem(inst)?;
            frame_inst(cx, b.offset_by(SIGN_BYTE), |m| mc::movzx_rm(Size::D, RAX, Size::B, m));
            put(cx, mc::and_ri(Size::D, RAX, 0x80));
            copy(cx, r, a);
            let sign = r.offset_by(SIGN_BYTE);
            frame_inst(cx, sign, |m| mc::and_mi(Size::B, m, 0x7f));
            frame_inst(cx, sign, |m| mc::or_mr(Size::B, m, RAX));
            Ok(())
        }
        (I::ArithmeticFence, &[x]) => {
            let a = cx.use_mem(x)?;
            let r = cx.def_mem(inst)?;
            copy(cx, r, a);
            Ok(())
        }
        (I::Sqrt, &[x]) => stack_op(cx, inst, &[x], &[mc::fsqrt()]),
        (I::Canonicalize, &[x]) => stack_op(cx, inst, &[x], &[mc::fld1(), mc::fmulp(St::St1)]),
        (I::FmulAdd, &[a, b, c]) => {
            let ops = [mc::fmulp(St::St1), mc::faddp(St::St1)];
            let (pa, pb, pc) = (cx.use_mem(a)?, cx.use_mem(b)?, cx.use_mem(c)?);
            let r = cx.def_mem(inst)?;
            fld(cx, pa);
            fld(cx, pb);
            put(cx, ops[0]);
            fld(cx, pc);
            put(cx, ops[1]);
            fstp(cx, r);
            Ok(())
        }
        (I::Lrint | I::Llrint, &[x]) => {
            // The store of the result's width, as `llc`: out of its range
            // (or for an invalid encoding) the integer indefinite of that
            // width.
            let bits = int_result(cx, inst)?;
            let a = cx.use_mem(x)?;
            let rd = cx.def_reg(inst)?;
            fld(cx, a);
            below(cx);
            let d = gp(rd);
            match bits {
                ..=16 => {
                    put(cx, mc::fistp_m(Size::W, sp(0)));
                    put(cx, mc::movzx_rm(Size::D, d, Size::W, sp(0)));
                }
                17..=32 => {
                    put(cx, mc::fistp_m(Size::D, sp(0)));
                    put(cx, mc::mov_rm(Size::D, d, sp(0)));
                }
                _ => {
                    put(cx, mc::fistp_m(Size::Q, sp(0)));
                    put(cx, mc::mov_rm(Size::Q, d, sp(0)));
                }
            }
            above(cx);
            cx.finish(inst, rd)
        }
        (I::FptosiSat | I::FptouiSat, &[x]) => saturate(cx, inst, which == I::FptosiSat, x),
        (I::IsFpClass, &[x, mask]) => fp_class(cx, inst, x, mask),
        (I::Powi, &[x, n]) => {
            let n = int32(cx, n)?;
            let r = cx.def_mem(inst)?;
            cx.call_runtime(RuntimeFn::PowiXf2, &[CallArg::Value(x), CallArg::Value(n)], Some(r))?;
            Ok(())
        }
        (I::Ldexp, &[x, n]) => {
            let n = int32(cx, n)?;
            let r = cx.def_mem(inst)?;
            libcall(cx, RuntimeFn::Ldexpl, &[x], Extra::Int(n), Some(r)).map(|_| ())
        }
        (I::Frexp, &[x]) => frexp(cx, inst, x),
        _ => unsupported(INTRINSIC),
    }
}

/// `inst`'s result: the operands `xs` pushed in order, `ops` run, the
/// one value left popped to the result's slot.
fn stack_op<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    xs: &[Value],
    ops: &[mc::Inst],
) -> CgResult<()> {
    let mut places = [FrameRef::Local(0); 3];
    for (place, &x) in places.iter_mut().zip(xs) {
        *place = cx.use_mem(x)?;
    }
    let r = cx.def_mem(inst)?;
    for &place in &places[..xs.len()] {
        fld(cx, place);
    }
    for &op in ops {
        put(cx, op);
    }
    fstp(cx, r);
    Ok(())
}

/// The `float` bits of `±2^k` (`k` at most 64).
fn pow2_f32(k: u32, negative: bool) -> i64 {
    (i64::from(127 + k) << 23) | (i64::from(negative) << 31)
}

/// `fptosi.sat`/`fptoui.sat` (`signed`) of an `x86_fp80` to `iN` (`N` up
/// to 64): 0 for a NaN (`fucomi st0, st0`, `jp`); the minimum where the
/// lower bound (`-2^(N-1)`, or 0) is greater than the value; the maximum
/// where the upper one (`2^(N-1)`, or `2^N`) is not greater; else
/// [`truncate`], which is exact in range. Each bound is a `float` pushed
/// and compared with `fucomip`; every path leaves the stack empty.
fn saturate<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, signed: bool, x: Value) -> CgResult<()> {
    let bits = int_result(cx, inst)?;
    let a = cx.use_mem(x)?;
    let rd = cx.def_reg(inst)?;
    let d = gp(rd);
    let (lo, hi) = if signed {
        (pow2_f32(bits - 1, true), pow2_f32(bits - 1, false))
    } else {
        (0, pow2_f32(bits, false))
    };
    let all = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let (min, max) = if signed { (!(all >> 1), all >> 1) } else { (0, all) };
    let (nan, low, high, done) = (cx.new_label(), cx.new_label(), cx.new_label(), cx.new_label());
    fld(cx, a);
    below(cx);
    put(cx, mc::fucomi(St::St0));
    branch_cond(cx, Cond::P, nan)?;
    fld_f32(cx, 0, lo);
    put(cx, mc::fucomip(St::St1));
    branch_cond(cx, Cond::A, low)?;
    fld_f32(cx, 0, hi);
    put(cx, mc::fucomip(St::St1));
    branch_cond(cx, Cond::Be, high)?;
    truncate(cx, d, signed || bits < 64)?;
    branch(cx, done)?;
    for (label, value) in [(nan, 0), (low, min), (high, max)] {
        cx.bind(label)?;
        put(cx, mc::fstp_st(St::St0));
        mov_imm(cx, d, value, 64);
        if label != high {
            branch(cx, done)?;
        }
    }
    cx.bind(done)?;
    above(cx);
    cx.finish(inst, rd)
}

/// `llvm.is.fpclass` of an `x86_fp80`: the class's index in the test
/// mask (0 signalling NaN .. 9 positive infinity) computed from the
/// image into `edx`, then `bt` of the mask. A pseudo-denormal (exponent
/// 0, integer bit set) is a subnormal, as the x87 loads it. The other
/// encodings only the x87 has (an unnormal, a pseudo-zero, a
/// pseudo-infinity, a pseudo-NaN: a non-zero exponent without the integer
/// bit) get index 10, whose bit is [`invalid_in`]: what `llc` 19 and 22
/// answer for them. Writes `rax`, `rcx`, `rdx`, `r10`, `r11` and the
/// flags.
fn fp_class<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, x: Value, mask: Value) -> CgResult<()> {
    let Some(test) = const_int(cx, mask) else { return unsupported(INTRINSIC) };
    let a = cx.use_mem(x)?;
    let rd = cx.def_reg(inst)?;
    let d = gp(rd);
    let labels: [_; 6] = std::array::from_fn(|_| cx.new_label());
    let [zero_exp, max_exp, invalid, signed, done, inf] = labels;
    frame_inst(cx, a, |m| mc::mov_rm(Size::Q, R11, m));
    frame_inst(cx, a.offset_by(8), |m| mc::movzx_rm(Size::D, RAX, Size::W, m));
    put(cx, mc::mov_rr(Size::D, RCX, RAX));
    put(cx, mc::and_ri(Size::D, RCX, 0x7fff));
    branch_cond(cx, Cond::E, zero_exp)?;
    put(cx, mc::cmp_ri(Size::D, RCX, 0x7fff));
    branch_cond(cx, Cond::E, max_exp)?;
    // A normal exponent: normal with the integer bit, else an unnormal.
    put(cx, mc::bt_ri(Size::Q, R11, 63));
    branch_cond(cx, Cond::Ae, invalid)?;
    mov_imm(cx, RDX, 8, 32);
    branch(cx, signed)?;
    cx.bind(zero_exp)?;
    // Zero, a denormal, or a pseudo-denormal (the integer bit set).
    mov_imm(cx, RDX, 6, 32);
    put(cx, mc::test_rr(Size::Q, R11, R11));
    branch_cond(cx, Cond::E, signed)?;
    mov_imm(cx, RDX, 7, 32);
    branch(cx, signed)?;
    cx.bind(max_exp)?;
    // An infinity or a NaN; without the integer bit, a pseudo-infinity
    // or a pseudo-NaN.
    put(cx, mc::bt_ri(Size::Q, R11, 63));
    branch_cond(cx, Cond::Ae, invalid)?;
    put(cx, mc::mov_rr(Size::Q, RDX, R11));
    put(cx, mc::shl_ri(Size::Q, RDX, 1));
    branch_cond(cx, Cond::E, inf)?;
    // A NaN: 1 (quiet) when bit 62 is set, else 0 (signalling).
    put(cx, mc::mov_rr(Size::Q, RDX, R11));
    put(cx, mc::shr_ri(Size::Q, RDX, 62));
    put(cx, mc::and_ri(Size::D, RDX, 1));
    branch(cx, done)?;
    cx.bind(invalid)?;
    mov_imm(cx, RDX, 10, 32);
    branch(cx, done)?;
    cx.bind(inf)?;
    mov_imm(cx, RDX, 9, 32);
    cx.bind(signed)?;
    // A negative value's index is `11 - i` (zero 6 to 5, ..., infinity
    // 9 to 2).
    put(cx, mc::test_ri(Size::D, RAX, 0x8000));
    branch_cond(cx, Cond::E, done)?;
    put(cx, mc::neg_r(Size::D, RDX));
    put(cx, mc::add_ri(Size::D, RDX, 11));
    cx.bind(done)?;
    let bits = (test & 0x3ff) | u64::from(invalid_in(test)) << 10;
    mov_imm(cx, R10, bits, 32);
    put(cx, mc::bt_rr(Size::D, R10, RDX));
    put(cx, mc::setcc_r(Cond::B, d));
    put(cx, mc::movzx_rr(Size::D, d, Size::B, d));
    cx.finish(inst, rd)
}

/// Whether `llvm.is.fpclass` with `test` holds for an unnormal, a
/// pseudo-zero, a pseudo-infinity or a pseudo-NaN, as `llc` 19 and 22
/// expand it (measured over all 1024 masks): when the test has both NaN
/// classes (they count as NaNs, as for `glibc`), or is everything but
/// one NaN class (`llc` tests the other class and inverts).
pub const fn invalid_in(test: u64) -> bool {
    let test = test & 0x3ff;
    test & 3 == 3 || test == 0x3fd || test == 0x3fe
}

/// The value of an integer constant operand.
fn const_int(cx: &FnCx<'_, X64>, value: Value) -> Option<u64> {
    match cx.module.consts.get(value.as_const()?) {
        Const::Int { value, .. } => Some(value),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// `frexp x` into its `{ x86_fp80, i32 }` result: `frexpl` with the
/// exponent's address in `rdi`, a slot of the call's stack area, then the
/// fraction popped to the result's slot and the exponent copied to its
/// field.
fn frexp<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, x: Value) -> CgResult<()> {
    let ty = inst.ty();
    let fields = cx.types().struct_fields(ty);
    let pair = fields.is_some_and(|fs| {
        fs.len() == 2 && is_x86_fp80(cx, fs[0]) && cx.types().int_bits(fs[1]) == Some(32)
    });
    if !pair {
        return unsupported(INTRINSIC);
    }
    let exp_at = cx.types().field_offset(ty, 1) as u32;
    let r = cx.def_mem(inst)?;
    libcall(cx, RuntimeFn::Frexpl, &[x], Extra::Ptr(exp_at), Some(r))
}

/// What [`libcall`] passes besides the `x86_fp80` arguments.
#[derive(Clone, Copy)]
enum Extra {
    /// Nothing.
    None,
    /// An `i32` in `edi`.
    Int(Value),
    /// The address of 16 more bytes of the stack area in `rdi`, whose
    /// first 4 are copied to the result at this offset after the call
    /// (`frexpl`'s exponent).
    Ptr(u32),
}

/// Calls `func`, a `long double` routine of libm, with the `x86_fp80`
/// arguments `xs` (and `extra`), by the System V sequence for class
/// `X87`: every register value displaced (the call clobbers the vector
/// registers the pool has), a stack area of 16 bytes per argument taken
/// (`rsp` stays 16-aligned), each argument's 10 bytes copied there,
/// `call`, the call site recorded, then the `st0` result popped to `out`
/// (or, without `out`, left in `rax`), and the area given back.
fn libcall(
    cx: &mut FnCx<'_, X64>,
    func: RuntimeFn,
    xs: &[Value],
    extra: Extra,
    out: Option<FrameRef>,
) -> CgResult<()> {
    cx.displace_all()?;
    let mut places = [FrameRef::Local(0); 3];
    for (place, &x) in places.iter_mut().zip(xs) {
        *place = cx.use_mem(x)?;
    }
    if let Extra::Int(n) = extra {
        cx.put(n, crate::reg::RDI)?;
    }
    let ptr_at = 16 * xs.len() as u32;
    let area = ptr_at + if matches!(extra, Extra::Ptr(_)) { 16 } else { 0 };
    put(cx, mc::lea(Size::Q, RSP, sp(-(area as i32))));
    for (i, &place) in places[..xs.len()].iter().enumerate() {
        copy_frame(cx, FrameRef::Outgoing(16 * i as u32), place, BYTES);
    }
    if matches!(extra, Extra::Ptr(_)) {
        put(cx, mc::lea(Size::Q, RDI, sp(ptr_at as i32)));
    }
    let at = cx.offset();
    crate::frame::emit_call(cx, CallTarget::Sym(SymRef::Runtime(func)), false, 0)?;
    let len = cx.offset() - at;
    cx.record_call_site(at, len);
    if let Some(out) = out {
        fstp(cx, out);
        if let Extra::Ptr(field) = extra {
            put(cx, mc::mov_rm(Size::D, RAX, sp(ptr_at as i32)));
            frame_inst(cx, out.offset_by(field), |m| mc::mov_mr(Size::D, m, RAX));
        }
    }
    put(cx, mc::lea(Size::Q, RSP, sp(area as i32)));
    Ok(())
}
