//! The short tail, second part (WP-095, `spec/codegen.md` §5.5.1
//! "WP-095"): first refusals that each stopped one to three corpus
//! modules. Each is reached by one call from the module that refused it.
//!
//! - Scalar `bfloat` arithmetic, compares and conversions ([`bfloat_cast`]
//!   from `float::cast`, [`bfloat_binary`] from `float::binary`,
//!   [`bfloat_flags`] from `float::flags`), as `llc` lowers them without
//!   `FEAT_BF16`: a `bfloat` is widened to single precision by a shift
//!   (exact), computed there, and narrowed by `llc`'s integer rounding
//!   sequence (to nearest, ties to even, on the single-precision bits).
//!   A conversion from `double` or from an integer of more than 16 bits
//!   rounds to odd first (`FCVTXN`), so that the value is rounded once.
//!   Conversions with an `fp128` side are [`BFLOAT_FP128`]; with an
//!   integer wider than 64 bits, `aggr`'s refusal.
//! - `extractelement` from a vector of `i128` ([`extract_element`], from
//!   the dispatch in `target.rs`): the lane's 16 bytes copied into the
//!   result's slot.
//! - `preserve_mostcc`, as callee and as caller: assigned as `ccc`
//!   ([`assign_preserve_most`], from `Target::assign`); a function of that
//!   convention saves and restores `x9`..`x15` with their unwind rules
//!   ([`save_preserved`], [`preserved_rows`], [`restore_preserved`], from
//!   `frame.rs`). A caller needs nothing: hod keeps no value in a register
//!   across a call.

use hod_cg::abi::{AbiUnsupported, Assignment, Sig};
use hod_cg::{CallArg, CfiOp, CgResult, Class, FnCx, FrameRef, PReg, RuntimeFn};
use hod_ir::{CallConv, FloatPredicate, Inst, Opcode, SymbolKind, TypeKind, TypeTable, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{Cond, FpFormat, FpSize, Load, MemSize, Reg, RegZr, Size, VMemSize, VReg};

use crate::common::{
    Cursor, FP, V31, X16, X17, copy_bytes, deferred_first, put, size_for_bits, store_fpr_at,
    unsupported, value_ty, vr, xr,
};
use crate::float::{self, FpCond};
use crate::target::A64;
use crate::vector::{self, Elem, Index, Lanes, MAX_LANES, Shape};

// ---- refusals ----

/// A conversion between `bfloat` and `fp128` (`llc` 19 cannot select
/// `fptrunc fp128 to bfloat`; `llc` 22 calls five `fp128` routines).
pub const BFLOAT_FP128: &str = "a64:bfloat-fp128";

/// Every refusal this module names.
pub const REASONS: [&str; 1] = [BFLOAT_FP128];

// ---- bfloat ----

/// `FCVTXN <Sd>, <Dn>`: FCVTXN (scalar), double to single precision,
/// rounding to odd. Not in `hod_mc_a64` (WP-095 encodes it here; the
/// tests disassemble it).
pub const fn fcvtxn(rd: VReg, rn: VReg) -> u32 {
    0x7e61_6800 | (rn.index() as u32) << 5 | rd.index() as u32
}

/// The `bfloat` in the low 16 bits of `v` widened to single precision in
/// `v`: its bits shifted to the top of the `S` view (exact). Writes `x16`.
pub fn widen(cx: &mut FnCx<'_, A64>, v: VReg) {
    put(cx, mc::fmov_fp_to_gpr(Size::W, X16.zr(), v));
    put(cx, mc::lsl_imm(Size::W, X16.zr(), X16.zr(), 16));
    put(cx, mc::fmov_gpr_to_fp(Size::W, v, X16.zr()));
}

/// The single-precision value in `v` rounded to a `bfloat` in `v`'s low
/// 16 bits, as `llc` does it: `x + 0x7fff + bit 16 of x`, the upper half
/// taken. With `quiet`, a NaN is kept a NaN by setting its quiet bit
/// first (`llc`'s `fptrunc float`; its arithmetic omits that). Writes
/// `x16`, `x17` and the flags.
pub fn narrow(cx: &mut FnCx<'_, A64>, v: VReg, quiet: bool) {
    let (w16, w17) = (X16.zr(), X17.zr());
    put(cx, mc::fmov_fp_to_gpr(Size::W, w16, v));
    put(cx, mc::ubfx(Size::W, w17, w16, 16, 1));
    put(cx, mc::add(Size::W, w17, w17, w16));
    put(cx, mc::add_imm(Size::W, X17.sp(), X17.sp(), 0x7000));
    put(cx, mc::add_imm(Size::W, X17.sp(), X17.sp(), 0xfff));
    if quiet {
        put(cx, mc::orr_imm(Size::W, X16.sp(), w16, 0x40_0000));
        put(cx, mc::fcmp(FpSize::S, v, v));
        put(cx, mc::csel(Size::W, w17, w16, w17, Cond::Vs));
    }
    put(cx, mc::lsr_imm(Size::W, w17, w17, 16));
    put(cx, mc::fmov_gpr_to_fp(Size::W, v, w17));
}

/// `%r = <cast> %arg` with a `bfloat` side and no vector side
/// (`float::cast`, after its `bitcast` arm).
///
/// # Errors
/// [`BFLOAT_FP128`]; [`float::CAST`] for a cast `llc` does not have
/// between these types; `aggr::WIDE_FP_CAST` for an integer wider than
/// 64 bits.
pub fn bfloat_cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let op = inst.opcode();
    let (from_ty, to_ty) = (value_ty(cx, arg), inst.ty());
    let (from, to) = (cx.types().kind(from_ty), cx.types().kind(to_ty));
    if matches!(from, TypeKind::Fp128) || matches!(to, TypeKind::Fp128) {
        return unsupported(BFLOAT_FP128);
    }
    let int_bits = |kind: TypeKind| match kind {
        TypeKind::Int(bits) if bits <= 64 => Ok(bits),
        TypeKind::Int(_) => unsupported(crate::aggr::WIDE_FP_CAST),
        _ => unsupported(float::CAST),
    };
    match op {
        Opcode::FPExt => {
            let double = match to {
                TypeKind::Float => false,
                TypeKind::Double => true,
                _ => return unsupported(float::CAST),
            };
            deferred_first(cx, inst, None)?;
            let r = cx.use_reg(arg)?;
            widen(cx, vr(r));
            if double {
                put(cx, mc::fcvt(FpFormat::D, vr(r), FpFormat::S, vr(r)));
            }
            cx.finish(inst, r)
        }
        Opcode::FPTrunc => {
            let double = match from {
                TypeKind::Float => false,
                TypeKind::Double => true,
                _ => return unsupported(float::CAST),
            };
            deferred_first(cx, inst, None)?;
            let r = cx.use_reg(arg)?;
            if double {
                // As `llc`: rounded to odd, then to nearest without the
                // NaN arm (`FCVTXN` keeps a NaN a NaN).
                put(cx, fcvtxn(vr(r), vr(r)));
            }
            narrow(cx, vr(r), !double);
            cx.finish(inst, r)
        }
        Opcode::FPToSI | Opcode::FPToUI => {
            let bits = int_bits(to)?;
            deferred_first(cx, inst, None)?;
            let a = cx.use_reg(arg)?;
            let rd = cx.def_reg(inst)?;
            widen(cx, vr(a));
            // Out of range is poison: either instruction's result does.
            let word = if op == Opcode::FPToSI { mc::fcvtzs } else { mc::fcvtzu };
            put(cx, word(size_for_bits(bits), xr(rd).zr(), FpSize::S, vr(a)));
            cx.finish(inst, rd)
        }
        Opcode::SIToFP | Opcode::UIToFP => {
            let bits = int_bits(from)?;
            let signed = op == Opcode::SIToFP;
            deferred_first(cx, inst, None)?;
            let a = cx.use_reg(arg)?;
            let rd = cx.def_reg(inst)?;
            int_to_bfloat(cx, xr(a), bits, signed, vr(rd));
            cx.finish(inst, rd)
        }
        _ => unsupported(float::CAST),
    }
}

/// The integer of `bits` bits in `ra` (the bits above unspecified)
/// converted to a `bfloat` in `vd`, rounded once: up to 16 bits through
/// single precision (exact), up to 32 through double (exact) and
/// `FCVTXN`; wider as `llc` does it, the conversion to double made to
/// round to odd by hand (the bits below the 53rd folded into a sticky
/// bit). Writes `ra`, `x16`, `x17` and the flags.
fn int_to_bfloat(cx: &mut FnCx<'_, A64>, ra: Reg, bits: u32, signed: bool, vd: VReg) {
    let sz = size_for_bits(bits);
    let r = ra.zr();
    if bits != 32 && bits != 64 {
        let extend = if signed { mc::sbfx } else { mc::ubfx };
        put(cx, extend(sz, r, r, 0, bits));
    }
    let convert = if signed { mc::scvtf } else { mc::ucvtf };
    if bits <= 16 {
        put(cx, convert(FpSize::S, vd, Size::W, r));
        narrow(cx, vd, false);
        return;
    }
    if bits <= 32 {
        put(cx, convert(FpSize::D, vd, Size::W, r));
        put(cx, fcvtxn(vd, vd));
        narrow(cx, vd, false);
        return;
    }
    let (x16, x17) = (X16.zr(), X17.zr());
    // `x17` = the magnitude (signed) or the value.
    if signed {
        put(cx, mc::cmp_imm(Size::X, ra.sp(), 0));
        put(cx, mc::cneg(Size::X, x17, r, Cond::Mi));
    } else {
        put(cx, mc::mov(Size::X, x17, r));
    }
    // At 2^53 or more the low 12 bits are dropped (what is left converts
    // exactly) and kept aside for the sticky bit.
    put(cx, mc::tst_imm(Size::X, x17, 0xffe0_0000_0000_0000));
    put(cx, mc::and_imm(Size::X, X16.sp(), x17, 0xffff_ffff_ffff_f000));
    put(cx, mc::csel(Size::X, x16, x16, x17, Cond::Ne));
    put(cx, mc::and_imm(Size::X, X17.sp(), x17, 0xfff));
    put(cx, mc::csel(Size::X, x17, x17, RegZr::ZR, Cond::Ne));
    put(cx, convert(FpSize::D, vd, Size::X, x16));
    put(cx, mc::fmov_fp_to_gpr(Size::X, x16, vd));
    put(cx, mc::cmp_imm(Size::X, X17.sp(), 0));
    put(cx, mc::cset(Size::X, x17, Cond::Ne));
    put(cx, mc::orr(Size::X, x16, x16, x17));
    if signed {
        put(cx, mc::and_imm(Size::X, X17.sp(), r, 1 << 63));
        put(cx, mc::orr(Size::X, x16, x16, x17));
    }
    put(cx, mc::fmov_gpr_to_fp(Size::X, vd, x16));
    put(cx, fcvtxn(vd, vd));
    narrow(cx, vd, false);
}

/// `%r = <op> bfloat %lhs, %rhs` (`float::binary`): both widened, the
/// single-precision operation (`frem`: `fmodf`), the result narrowed
/// without the NaN arm, as `llc`.
///
/// # Errors
/// [`float::BINARY`] for an integer opcode (invalid IR); what the call
/// reports.
pub fn bfloat_binary<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let word = match inst.opcode() {
        Opcode::FAdd => mc::fadd,
        Opcode::FSub => mc::fsub,
        Opcode::FMul => mc::fmul,
        Opcode::FDiv => mc::fdiv,
        Opcode::FRem => return bfloat_frem(cx, inst, lhs, rhs),
        _ => return unsupported(float::BINARY),
    };
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(lhs)?;
    let b = cx.use_reg(rhs)?;
    widen(cx, vr(a));
    if b != a {
        widen(cx, vr(b));
    }
    put(cx, word(FpSize::S, vr(a), vr(a), vr(b)));
    narrow(cx, vr(a), false);
    cx.finish(inst, a)
}

/// `frem` of `bfloat`: `fmodf` of the widened operands (`float`'s
/// `half_frem`, with the shift for `fcvt`).
fn bfloat_frem<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(lhs)?;
    let t1 = cx.temp(Class::Fpr)?;
    put(cx, mc::fmov(FpSize::S, vr(t1), vr(a)));
    widen(cx, vr(t1));
    give_back(cx, lhs, a);
    let b = cx.use_reg(rhs)?;
    put(cx, mc::fmov(FpSize::S, V31, vr(b)));
    widen(cx, V31);
    give_back(cx, rhs, b);
    let t2 = cx.temp(Class::Fpr)?;
    put(cx, mc::fmov(FpSize::S, vr(t2), V31));
    let args = [CallArg::Reg(t1), CallArg::Reg(t2)];
    let Some(r) = cx.call_runtime(RuntimeFn::Fmodf, &args, None)? else {
        return cx.bug("a runtime function returned nothing");
    };
    narrow(cx, vr(r), false);
    cx.finish(inst, r)
}

/// Releases a consumed operand's register before the instruction ends
/// (`float`'s `give_back`).
fn give_back(cx: &mut FnCx<'_, A64>, operand: Value, reg: PReg) {
    cx.release(operand);
    cx.free_temp(reg);
}

/// The flags of `fcmp <pred> bfloat %lhs, %rhs` (`float::flags`, whose
/// contract this keeps): both widened, `FCMP` in single precision.
///
/// # Errors
/// What asking for the operands reports.
pub fn bfloat_flags<'a>(
    cx: &mut FnCx<'a, A64>,
    compare: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<FpCond> {
    deferred_first(cx, compare, None)?;
    if matches!(pred, FloatPredicate::True | FloatPredicate::False) {
        put(cx, mc::cmp(Size::X, RegZr::ZR, RegZr::ZR));
        return Ok(float::conditions(pred));
    }
    let a = cx.use_reg(lhs)?;
    let b = cx.use_reg(rhs)?;
    widen(cx, vr(a));
    if b != a {
        widen(cx, vr(b));
    }
    put(cx, mc::fcmp(FpSize::S, vr(a), vr(b)));
    give_back(cx, lhs, a);
    give_back(cx, rhs, b);
    Ok(float::conditions(pred))
}

// ---- extractelement of i128 ----

/// `%r = extractelement %vector, %index` (the dispatch in `target.rs`):
/// a vector of `i128` here, every other one to
/// [`vector::extract_element`]. The lane's 16 bytes are copied into the
/// result's slot; the lane of a variable index is `vector`'s rule
/// (masked for a power-of-two length, else clamped to 0); `<1 x i128>`,
/// a value of class `Fpr`, is stored from its register.
///
/// # Errors
/// [`vector::LENGTH`] beyond [`MAX_LANES`] lanes; what [`vector::index`]
/// and asking for the operands report.
pub fn extract_element<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    vector: Value,
    at: Value,
) -> CgResult<()> {
    let shape = match Shape::of(cx.types(), value_ty(cx, vector)) {
        Some(shape) if shape.elem == Elem::Int(128) => shape,
        _ => return vector::extract_element(cx, inst, vector, at),
    };
    if shape.lanes > MAX_LANES {
        return unsupported(vector::LENGTH);
    }
    deferred_first(cx, inst, None)?;
    // Lanes of 16 bytes: `log2` 4 serves `index` and `lane_cursor`.
    let lanes = Lanes { shape, log2: 4 };
    let idx = vector::index(cx, at, &lanes)?;
    if shape.class == Class::Fpr {
        // One lane: any index reads it (an index out of range is poison).
        let v = cx.use_reg(vector)?;
        let r = cx.def_mem(inst)?;
        let mut dst = Cursor::frame(cx, r, X16);
        store_fpr_at(cx, VMemSize::Q, vr(v), &mut dst);
        return Ok(());
    }
    let image = cx.use_mem(vector)?;
    let r = cx.def_mem(inst)?;
    let idx = if idx == Index::Poison { Index::Lane(0) } else { idx };
    let mut src = vector::lane_cursor(cx, image, &lanes, idx);
    let mut dst = Cursor::frame(cx, r, X17);
    copy_bytes(cx, &mut dst, &mut src, 16);
    Ok(())
}

// ---- preserve_mostcc ----

/// The registers `preserve_mostcc` adds to the AAPCS64 callee-saved set
/// (LLVM's `CSR_AArch64_RT_MostRegs`): `x9`..`x15`. Nothing else: `x8`,
/// `x16`..`x18` and the vector registers stay as `ccc` has them.
pub const PRESERVED: [Reg; 7] =
    [Reg::X9, Reg::X10, Reg::X11, Reg::X12, Reg::X13, Reg::X14, Reg::X15];

/// Where a `preserve_mostcc` function saves [`PRESERVED`]: the 64 bytes
/// after the frame record, `x9` lowest.
const SAVE_AREA: u32 = 16;

/// Whether the function being compiled is `preserve_mostcc`.
pub fn is_preserve_most(cx: &FnCx<'_, A64>) -> bool {
    matches!(
        cx.module.symbol(cx.body.symbol()).kind(),
        SymbolKind::Function { cc: CallConv::PRESERVE_MOST, .. }
    )
}

/// `Target::assign` for `preserve_mostcc`: arguments and results travel
/// as for `ccc`; only the callee-saved set differs. `None` for another
/// convention.
pub fn assign_preserve_most(
    types: &TypeTable,
    sig: &Sig<'_>,
    out: &mut Assignment,
) -> Option<Result<(), AbiUnsupported>> {
    (sig.cc == CallConv::PRESERVE_MOST)
        .then(|| crate::abi::assign(types, &Sig { cc: CallConv::C, ..*sig }, out))
}

/// From `frame::begin`, right after `mov x29, sp`: a `preserve_mostcc`
/// function stores [`PRESERVED`] in its save area. Every lowering may
/// write them and every call clobbers them, so all seven are saved (`llc`
/// saves those its allocator touched).
pub fn save_preserved(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    if !is_preserve_most(cx) {
        return Ok(());
    }
    if cx.alloc_slot(64, 16)? != FrameRef::Local(SAVE_AREA) {
        return cx.bug("the preserve_most save area is not after the frame record");
    }
    for (index, pair) in PRESERVED.chunks(2).enumerate() {
        let at = i64::from(SAVE_AREA) + 16 * index as i64;
        match *pair {
            [a, b] => put(cx, mc::stp_off(Size::X, a.zr(), b.zr(), FP, at)),
            [a] => put(cx, mc::str_off(MemSize::X, a.zr(), FP, at)),
            _ => {}
        }
    }
    Ok(())
}

/// The bytes [`save_preserved`] emits.
pub const SAVE_BYTES: u32 = 16;

/// The rules saying where [`PRESERVED`] are saved, given the CFA is
/// `x29 + frame`: from `at` in `end` (the offset after the stores), and
/// again in the body rows a tail exit restates.
pub fn preserved_rows(cx: &mut FnCx<'_, A64>, at: Option<u32>, frame: u32) -> CgResult<()> {
    if !is_preserve_most(cx) {
        return Ok(());
    }
    for (index, reg) in PRESERVED.iter().enumerate() {
        let saved = SAVE_AREA as i32 + 8 * index as i32;
        let op = CfiOp::Offset { reg: reg.index() as u16, cfa_offset: saved - frame as i32 };
        match at {
            Some(at) => cx.cfi_at(at, op)?,
            None => cx.cfi(op),
        }
    }
    Ok(())
}

/// From `frame::teardown`, before the frame record is reloaded:
/// [`PRESERVED`] reloaded from the save area, each rule back to "same
/// value".
pub fn restore_preserved(cx: &mut FnCx<'_, A64>) {
    if !is_preserve_most(cx) {
        return;
    }
    for (index, pair) in PRESERVED.chunks(2).enumerate() {
        let at = i64::from(SAVE_AREA) + 16 * index as i64;
        match *pair {
            [a, b] => put(cx, mc::ldp_off(Size::X, a.zr(), b.zr(), FP, at)),
            [a] => put(cx, mc::ldr_off(Load::X, a.zr(), FP, at)),
            _ => {}
        }
    }
    for reg in PRESERVED {
        cx.cfi(CfiOp::Restore { reg: reg.index() as u16 });
    }
}
