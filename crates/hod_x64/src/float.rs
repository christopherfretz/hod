//! Floating-point instructions on values of class `Fpr` (SSE2): binary
//! operations, `fneg`, `freeze`, casts with a floating-point side,
//! `fcmp` (deferred and fused into `br` and `select`), `select`, and the
//! constants of class `Fpr` (`spec/codegen.md` §5.6.7, WP-127).
//!
//! **Owner: WP-127.** Tests: `tests/float.rs`. The decisions:
//!
//! - `float` and `double` use the scalar SSE2 instructions, two-operand:
//!   the result is computed in the left operand's register. `frem` calls
//!   `fmodf`/`fmod`. No load is folded; no instruction above SSE2 is
//!   emitted (no `roundss`, no `cvttps2udq`, no AVX).
//! - `half` is computed in `float` through the shared runtime routines,
//!   as `llc` does without F16C: `__extendhfsf2` in, the operation,
//!   `__truncsfhf2` out (`fadd` .. `fdiv` round once: single precision has
//!   more than twice half's significand plus two bits). A `half` travels
//!   in the low 16 bits of an `xmm` register, as an argument too.
//! - `bfloat` has no runtime routine in the table (`llc` calls
//!   `__truncsfbf2`): it is widened by a shift (exact) and narrowed inline,
//!   to nearest even on the single-precision bits, a NaN kept a NaN with
//!   its quiet bit set (what `__truncsfbf2` returns). `double` and the
//!   integers of more than 16 bits are first rounded to odd in single
//!   precision, so that every conversion rounds once. Conversions between
//!   `bfloat` and `fp128` are [`BFLOAT_FP128`].
//! - `fp128` is runtime calls throughout: `__addtf3` .. `__divtf3`,
//!   `fmodf128`, the `__*tf2` compares and conversions; its constants
//!   come from a data island.
//! - The default floating-point environment: no instruction reads or
//!   writes `MXCSR`. A NaN an operation computes is what the hardware
//!   produces (the first operand's, quieted, or the default NaN), which
//!   `LangRef` allows (`spec/interp.md` §5.1).
//! - Out-of-range `fptoui`/`fptosi` are poison (LangRef): the integer
//!   indefinite `cvtts2si` gives is as good as any value.
//! - Vectors of class `Fpr` are moves here (`freeze`, `select`,
//!   `bitcast`, constants); their arithmetic is WP-132's
//!   ([`crate::vector::VECTOR`]).
//!
//! # Registers
//!
//! Besides the operands' registers, a lowering here writes `rax`, `rcx`,
//! `rdx`, `r11`, `xmm14` and `xmm15`. [`materialize`] (glue) writes
//! `r11` only.

use hod_cg::sym::{
    FpOp, FpType, ZeroCmp, fp_ext_routine, fp_routine, fp_to_int_routine, fp_trunc_routine,
    fp128_compare, int_to_fp_routine,
};
use hod_cg::{CallArg, CgResult, Class, FnCx, FrameRef, Label, PReg, RuntimeFn, Target};
use hod_ir::{
    Arch, BlockId, Const, FloatPredicate, Inst, InstKind, IntPredicate, Opcode, TypeId, TypeKind,
    Value, ValueKind,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Mem, Reg, Size, Xmm};

use crate::common::{
    R11, REL32, XMM14, XMM15, branch_cond, deferred_first, gp, load_fpr, load_gpr, mov_imm, put,
    store_fpr, store_gpr, unsupported, value_ty, xm,
};
use crate::int::{Ext, extend_to};
use crate::target::X64;

/// A binary opcode that is no floating-point operation on a scalar
/// (invalid IR).
pub const BINARY: &str = "x64:fp-binary";
/// A cast with a floating-point side that is no conversion between these
/// types (invalid IR), or an integer side wider than 64 bits.
pub const CAST: &str = "x64:fp-cast";
/// A conversion between `bfloat` and `fp128`: no routine in the table and
/// no single rounding inline (`llc` 22 calls `__trunctfbf2`).
pub const BFLOAT_FP128: &str = "x64:bfloat-fp128";

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;

/// The bits of `2^63` as a `float`.
const TWO_63_F32: u64 = 0x5f00_0000;
/// The bits of `2^63` as a `double`.
const TWO_63_F64: u64 = 0x43e0_0000_0000_0000;

// ---- shapes ----

/// What a type of class `Fpr` is, for the lowering.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// `half`.
    Half,
    /// `bfloat`.
    BFloat,
    /// `float`.
    Float,
    /// `double`.
    Double,
    /// `fp128`.
    Quad,
    /// A vector of class `Fpr` (64 or 128 bits).
    Vector,
}

impl Shape {
    /// The SSE size an operation on this shape computes in: `float` for
    /// `half` and `bfloat` (widened first), `double` for `double`.
    pub fn size(self) -> FpSize {
        if self == Shape::Double { FpSize::D } else { FpSize::S }
    }

    /// The type, as `hod_cg::sym`'s routine tables key it.
    fn fp_type(self) -> FpType {
        match self {
            Shape::Half => FpType::Half,
            Shape::Double => FpType::Double,
            Shape::Quad => FpType::Fp128,
            _ => FpType::Float,
        }
    }
}

/// The shape of `ty`, a type of class `Fpr`.
pub fn shape(cx: &FnCx<'_, X64>, ty: TypeId) -> Shape {
    match cx.types().kind(ty) {
        TypeKind::Half => Shape::Half,
        TypeKind::BFloat => Shape::BFloat,
        TypeKind::Float => Shape::Float,
        TypeKind::Double => Shape::Double,
        TypeKind::Fp128 => Shape::Quad,
        _ => Shape::Vector,
    }
}

/// The class of `ty` on this target.
fn class_of_ty(cx: &FnCx<'_, X64>, ty: TypeId) -> Class {
    X64.class_of(cx.types(), ty)
}

// ---- small helpers ----

/// Releases a consumed operand's register before the instruction ends.
pub(crate) fn give_back(cx: &mut FnCx<'_, X64>, operand: Value, reg: PReg) {
    cx.release(operand);
    cx.free_temp(reg);
}

/// Calls `func` and returns the register of its result (class `Gpr` or
/// `Fpr`).
pub(crate) fn runtime(cx: &mut FnCx<'_, X64>, func: RuntimeFn, args: &[CallArg]) -> CgResult<PReg> {
    match cx.call_runtime(func, args, None)? {
        Some(reg) => Ok(reg),
        None => cx.bug("a runtime function returned nothing"),
    }
}

/// Puts the low `bits` bits of `value` in `x` through `r11` (`mov` +
/// `movd`/`movq`): the bits above them in `x` are zero. Flags are kept.
pub fn xmm_imm(cx: &mut FnCx<'_, X64>, x: Xmm, value: u64, bits: u32) {
    mov_imm(cx, R11, value, bits);
    let size = if bits <= 32 { Size::D } else { Size::Q };
    put(cx, mc::movd_xr(size, x, R11));
}

/// Emits `inst`, whose memory operand is `[rip + disp32]`, with the
/// field fixed up to `label` (a data island).
fn put_rip(cx: &mut FnCx<'_, X64>, inst: mc::Inst, label: Label) -> CgResult<()> {
    let Some(site) = inst.site() else {
        return cx.bug("a RIP-relative access without a displacement field");
    };
    let at = cx.offset();
    put(cx, inst);
    cx.fixup(at + site.offset as u32, REL32, label)
}

/// `x = x <op> y` in `size`, for `fadd` .. `fdiv`.
fn arith(cx: &mut FnCx<'_, X64>, op: Opcode, size: FpSize, x: Xmm, y: Xmm) -> CgResult<()> {
    let inst = match op {
        Opcode::FAdd => mc::adds_rr(size, x, y),
        Opcode::FSub => mc::subs_rr(size, x, y),
        Opcode::FMul => mc::muls_rr(size, x, y),
        Opcode::FDiv => mc::divs_rr(size, x, y),
        _ => return unsupported(BINARY),
    };
    put(cx, inst);
    Ok(())
}

// ---- half and bfloat through float ----

/// `half` operand `value` widened to `float` by `__extendhfsf2`: the
/// result's register (`xmm0`, a pinned temporary until the next call).
pub(crate) fn half_in(cx: &mut FnCx<'_, X64>, value: Value) -> CgResult<PReg> {
    let Some(func) = fp_ext_routine(FpType::Half, FpType::Float, Arch::X86_64) else {
        return cx.bug("no half extension routine on x86-64");
    };
    runtime(cx, func, &[CallArg::Value(value)])
}

/// The `float` in `reg` (a temporary, or the result register of a call)
/// narrowed to `half` by `__truncsfhf2`: the result's register. The value
/// goes through a pool temporary first: a call argument must be in one.
pub(crate) fn half_out(cx: &mut FnCx<'_, X64>, reg: PReg) -> CgResult<PReg> {
    let t = cx.temp(Class::Fpr)?;
    put(cx, mc::movap_rr(FpSize::S, xm(t), xm(reg)));
    cx.free_temp(reg);
    narrow_call(cx, Shape::Float, CallArg::Reg(t))
}

/// `arg` of `from` (`float` or `double`) narrowed to `half` by
/// `__truncsfhf2`/`__truncdfhf2`: the result's register.
pub(crate) fn narrow_call(cx: &mut FnCx<'_, X64>, from: Shape, arg: CallArg) -> CgResult<PReg> {
    let Some(func) = fp_trunc_routine(from.fp_type(), FpType::Half, Arch::X86_64) else {
        return cx.bug("no half truncation routine on x86-64");
    };
    runtime(cx, func, &[arg])
}

/// Both `half` operands widened to `float` (two calls; the first result
/// kept in a frame slot across the second): the left one in `xmm15`, the
/// right one in the returned register.
fn half_pair(cx: &mut FnCx<'_, X64>, lhs: Value, rhs: Value) -> CgResult<PReg> {
    let a = half_in(cx, lhs)?;
    if rhs == lhs {
        put(cx, mc::movap_rr(FpSize::S, XMM15, xm(a)));
        return Ok(a);
    }
    let slot = cx.alloc_slot(4, 4)?;
    store_fpr(cx, xm(a), slot, 4);
    let b = half_in(cx, rhs)?;
    load_fpr(cx, XMM15, slot, 4);
    Ok(b)
}

/// The `bfloat` in the low 16 bits of `x` widened to `float` in `x`: its
/// bits shifted to the top (exact). Writes `rax`.
pub fn bfloat_widen(cx: &mut FnCx<'_, X64>, x: Xmm) {
    put(cx, mc::movd_rx(Size::D, RAX, x));
    put(cx, mc::shl_ri(Size::D, RAX, 16));
    put(cx, mc::movd_xr(Size::D, x, RAX));
}

/// The `float` in `x` rounded to a `bfloat` in `x`'s low 16 bits: `x +
/// 0x7fff + bit 16 of x`, the upper half taken; a NaN keeps its upper
/// bits with the quiet bit set. Writes `rax`, `rcx`, `rdx`, `r11` and the
/// flags.
pub fn bfloat_narrow(cx: &mut FnCx<'_, X64>, x: Xmm) {
    put(cx, mc::movd_rx(Size::D, RAX, x));
    put(cx, mc::mov_rr(Size::D, RCX, RAX));
    put(cx, mc::shr_ri(Size::D, RCX, 16));
    put(cx, mc::and_ri(Size::D, RCX, 1));
    put(cx, mc::add_rr(Size::D, RCX, RAX));
    put(cx, mc::add_ri(Size::D, RCX, 0x7fff));
    put(cx, mc::mov_rr(Size::D, RDX, RAX));
    put(cx, mc::or_ri(Size::D, RDX, 0x40_0000));
    put(cx, mc::mov_rr(Size::D, R11, RAX));
    put(cx, mc::and_ri(Size::D, R11, 0x7fff_ffff));
    put(cx, mc::cmp_ri(Size::D, R11, 0x7f80_0000));
    put(cx, mc::cmovcc_rr(Cond::A, Size::D, RCX, RDX));
    put(cx, mc::shr_ri(Size::D, RCX, 16));
    put(cx, mc::movd_xr(Size::D, x, RCX));
}

/// The `double` in `x` converted to `float` in `x`, rounded to odd (an
/// inexact result has its last bit set, after truncation toward zero), so
/// that a later rounding to `bfloat` rounds once. A NaN is converted as
/// `cvtsd2ss` does. Writes `rax`, `rcx`, `rdx`, `xmm14`, `xmm15` and the
/// flags.
pub fn double_to_float_odd(cx: &mut FnCx<'_, X64>, x: Xmm) -> CgResult<()> {
    let done = cx.new_label();
    put(cx, mc::movap_rr(FpSize::D, XMM15, x));
    put(cx, mc::cvtsd2ss_rr(x, x));
    put(cx, mc::ucomis_rr(FpSize::D, XMM15, XMM15));
    branch_cond(cx, Cond::P, done)?;
    put(cx, mc::cvtss2sd_rr(XMM14, x));
    put(cx, mc::movd_rx(Size::Q, RAX, XMM14));
    put(cx, mc::shl_ri(Size::Q, RAX, 1));
    put(cx, mc::movd_rx(Size::Q, RCX, XMM15));
    put(cx, mc::shl_ri(Size::Q, RCX, 1));
    put(cx, mc::cmp_rr(Size::Q, RAX, RCX));
    branch_cond(cx, Cond::E, done)?;
    // Inexact: one step toward zero if it was rounded away, then odd.
    put(cx, mc::movd_rx(Size::D, RDX, x));
    put(cx, mc::setcc_r(Cond::A, RAX));
    put(cx, mc::movzx_rr(Size::D, RAX, Size::B, RAX));
    put(cx, mc::sub_rr(Size::D, RDX, RAX));
    put(cx, mc::or_ri(Size::D, RDX, 1));
    put(cx, mc::movd_xr(Size::D, x, RDX));
    cx.bind(done)
}

// ---- conditions (§4.6) ----

/// The condition(s) under which an `fcmp` holds once [`flags`] has set
/// the flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpCond {
    /// When the condition holds.
    One(Cond),
    /// When both hold (`oeq`: equal and ordered).
    All(Cond, Cond),
    /// When either holds (`une`: not equal or unordered).
    Any(Cond, Cond),
}

/// After `ucomis x, y` (unordered sets `ZF`, `PF` and `CF`; less than
/// sets `CF`; equal sets `ZF`): whether `pred` compares the operands
/// swapped, and the condition(s) under which it holds. `ogt`, `oge` are
/// `A`, `AE` (unordered sets `CF`); `olt`, `ole` are those swapped; the
/// unordered-or predicates are the complements. `true` and `false` are
/// `E` and `NE` after a `cmp eax, eax` in place of the `ucomis`.
pub fn ucomis_conditions(pred: FloatPredicate) -> (bool, FpCond) {
    use FloatPredicate as P;
    match pred {
        P::Oeq => (false, FpCond::All(Cond::E, Cond::Np)),
        P::Une => (false, FpCond::Any(Cond::Ne, Cond::P)),
        P::Ogt => (false, FpCond::One(Cond::A)),
        P::Oge => (false, FpCond::One(Cond::Ae)),
        P::Olt => (true, FpCond::One(Cond::A)),
        P::Ole => (true, FpCond::One(Cond::Ae)),
        P::Ugt => (true, FpCond::One(Cond::B)),
        P::Uge => (true, FpCond::One(Cond::Be)),
        P::Ult => (false, FpCond::One(Cond::B)),
        P::Ule => (false, FpCond::One(Cond::Be)),
        // `true` and `false` after `cmp eax, eax`, which sets `ZF`.
        P::Ueq | P::True => (false, FpCond::One(Cond::E)),
        P::One | P::False => (false, FpCond::One(Cond::Ne)),
        P::Ord => (false, FpCond::One(Cond::Np)),
        P::Uno => (false, FpCond::One(Cond::P)),
    }
}

/// Performs the scalar `fcmp` `compare` (deferred, or being lowered)
/// into the flags and returns the condition(s) under which it holds. Its
/// operands are consumed: their registers are free again when this
/// returns, so the caller may ask for two more vector registers. Nothing
/// between this and the reader of the flags may write them: moves, loads
/// of spilled values and constants do not.
///
/// A `half` or `fp128` compare is calls (every register is displaced and
/// every temporary ends): the caller asks for nothing before this and for
/// what it needs after it.
///
/// # Errors
/// [`crate::vector::VECTOR`] for vector operands; `Internal` if `compare`
/// is not an `fcmp`.
pub fn flags<'a>(cx: &mut FnCx<'a, X64>, compare: Inst<'a>) -> CgResult<FpCond> {
    let InstKind::FCmp { pred, lhs, rhs } = compare.kind() else {
        return cx.bug("flags of an instruction that is not an fcmp");
    };
    let shape = compare_shape(cx, lhs)?;
    deferred_first(cx, compare, None)?;
    let (swap, cond) = ucomis_conditions(pred);
    if matches!(pred, FloatPredicate::True | FloatPredicate::False) {
        put(cx, mc::cmp_rr(Size::D, RAX, RAX));
        return Ok(cond);
    }
    match shape {
        Shape::Quad => quad_flags(cx, pred, lhs, rhs),
        Shape::Half => {
            let b = half_pair(cx, lhs, rhs)?;
            let (x, y) = if swap { (xm(b), XMM15) } else { (XMM15, xm(b)) };
            put(cx, mc::ucomis_rr(FpSize::S, x, y));
            cx.free_temp(b);
            Ok(cond)
        }
        _ => {
            let a = cx.use_reg(lhs)?;
            let b = cx.use_reg(rhs)?;
            if shape == Shape::BFloat {
                bfloat_widen(cx, xm(a));
                if b != a {
                    bfloat_widen(cx, xm(b));
                }
            }
            let (x, y) = if swap { (xm(b), xm(a)) } else { (xm(a), xm(b)) };
            put(cx, mc::ucomis_rr(shape.size(), x, y));
            give_back(cx, lhs, a);
            give_back(cx, rhs, b);
            Ok(cond)
        }
    }
}

/// The shape of an `fcmp` operand, or the refusal of a vector.
fn compare_shape(cx: &FnCx<'_, X64>, operand: Value) -> CgResult<Shape> {
    match shape(cx, value_ty(cx, operand)) {
        Shape::Vector => unsupported(crate::vector::VECTOR),
        s => Ok(s),
    }
}

/// The flags of `fcmp <pred> fp128 %lhs, %rhs` (`pred` neither `true`
/// nor `false`), as `softenSetCCOperands` computes them: one call whose
/// `int` result is compared with 0 (`hod_cg::sym::fp128_compare`); `ueq`
/// and `one` need two ([`quad_two_calls`]).
fn quad_flags(
    cx: &mut FnCx<'_, X64>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<FpCond> {
    let Some(plan) = fp128_compare(pred) else {
        return match pred {
            FloatPredicate::Ueq | FloatPredicate::One => quad_two_calls(cx, pred, lhs, rhs),
            _ => cx.bug("a constant fcmp predicate at the call"),
        };
    };
    let cond = match plan.result {
        ZeroCmp::Eq => Cond::E,
        ZeroCmp::Ne => Cond::Ne,
        ZeroCmp::Lt => Cond::L,
        ZeroCmp::Ge => Cond::Ge,
        ZeroCmp::Le => Cond::Le,
        ZeroCmp::Gt => Cond::G,
    };
    let (x, y) = if plan.swap { (rhs, lhs) } else { (lhs, rhs) };
    let r = runtime(cx, plan.func, &[CallArg::Value(x), CallArg::Value(y)])?;
    put(cx, mc::cmp_ri(Size::D, gp(r), 0));
    cx.free_temp(r);
    Ok(FpCond::One(cond))
}

/// `fcmp ueq` and `fcmp one` of `fp128`: `__eqtf2` and `__unordtf2` of
/// copies of the operands kept in a frame slot (a call consumes its
/// arguments), the first result kept there too; then `ZF` clear exactly
/// when the operands are unordered or equal: `ueq` is `NE`, `one` is `E`.
fn quad_two_calls(
    cx: &mut FnCx<'_, X64>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<FpCond> {
    let slot = cx.alloc_slot(48, 16)?;
    let a = cx.use_reg(lhs)?;
    let b = cx.use_reg(rhs)?;
    store_fpr(cx, xm(a), slot, 16);
    store_fpr(cx, xm(b), slot.offset_by(16), 16);
    give_back(cx, lhs, a);
    give_back(cx, rhs, b);
    let eq = quad_call_from(cx, slot, RuntimeFn::EqTf2)?;
    store_gpr(cx, gp(eq), slot.offset_by(32), 4);
    cx.free_temp(eq);
    let unord = quad_call_from(cx, slot, RuntimeFn::UnordTf2)?;
    // `al` = unordered, `cl` = equal; `ZF` = neither.
    put(cx, mc::test_rr(Size::D, gp(unord), gp(unord)));
    put(cx, mc::setcc_r(Cond::Ne, RAX));
    load_gpr(cx, RCX, slot.offset_by(32), 4);
    put(cx, mc::test_rr(Size::D, RCX, RCX));
    put(cx, mc::setcc_r(Cond::E, RCX));
    put(cx, mc::or_rr(Size::B, RAX, RCX));
    cx.free_temp(unord);
    Ok(FpCond::One(if pred == FloatPredicate::Ueq { Cond::Ne } else { Cond::E }))
}

/// Calls `func` on the two `fp128` values at `slot` and `slot + 16`,
/// loaded into temporaries.
fn quad_call_from(cx: &mut FnCx<'_, X64>, slot: FrameRef, func: RuntimeFn) -> CgResult<PReg> {
    let t1 = cx.temp(Class::Fpr)?;
    let t2 = cx.temp(Class::Fpr)?;
    load_fpr(cx, xm(t1), slot, 16);
    load_fpr(cx, xm(t2), slot.offset_by(16), 16);
    runtime(cx, func, &[CallArg::Reg(t1), CallArg::Reg(t2)])
}

// ---- entry points ----

/// `%r = <op> %lhs, %rhs` of class `Fpr`.
///
/// # Errors
/// [`BINARY`] for an integer opcode on a scalar (invalid IR);
/// [`crate::vector::VECTOR`] for a vector; what a call reports.
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let op = inst.opcode();
    let shape = shape(cx, inst.ty());
    if shape == Shape::Vector {
        return crate::lanes::binary(cx, inst, lhs, rhs);
    }
    if !matches!(op, Opcode::FAdd | Opcode::FSub | Opcode::FMul | Opcode::FDiv | Opcode::FRem) {
        return unsupported(if shape == Shape::Vector { crate::vector::VECTOR } else { BINARY });
    }
    match shape {
        Shape::Vector => unsupported(crate::vector::VECTOR),
        Shape::Quad => {
            let fp_op = match op {
                Opcode::FAdd => FpOp::Add,
                Opcode::FSub => FpOp::Sub,
                Opcode::FMul => FpOp::Mul,
                Opcode::FDiv => FpOp::Div,
                _ => FpOp::Rem,
            };
            let Some(func) = fp_routine(fp_op, FpType::Fp128, Arch::X86_64) else {
                return cx.bug("an fp128 operation without a routine");
            };
            let r = runtime(cx, func, &[CallArg::Value(lhs), CallArg::Value(rhs)])?;
            cx.finish(inst, r)
        }
        Shape::Float | Shape::Double if op == Opcode::FRem => {
            let func = fmod_of(shape.fp_type());
            let r = runtime(cx, func, &[CallArg::Value(lhs), CallArg::Value(rhs)])?;
            cx.finish(inst, r)
        }
        Shape::Float | Shape::Double => {
            deferred_first(cx, inst, None)?;
            let a = cx.use_reg(lhs)?;
            let b = cx.use_reg(rhs)?;
            arith(cx, op, shape.size(), xm(a), xm(b))?;
            cx.finish(inst, a)
        }
        Shape::Half => {
            deferred_first(cx, inst, None)?;
            let b = half_pair(cx, lhs, rhs)?;
            let r = if op == Opcode::FRem {
                put(cx, mc::movap_rr(FpSize::S, XMM14, xm(b)));
                cx.free_temp(b);
                fmod_scratch(cx)?
            } else {
                arith(cx, op, FpSize::S, XMM15, xm(b))?;
                put(cx, mc::movap_rr(FpSize::S, xm(b), XMM15));
                b
            };
            let r = half_out(cx, r)?;
            cx.finish(inst, r)
        }
        Shape::BFloat => {
            deferred_first(cx, inst, None)?;
            let a = cx.use_reg(lhs)?;
            let b = cx.use_reg(rhs)?;
            bfloat_widen(cx, xm(a));
            if b != a {
                bfloat_widen(cx, xm(b));
            }
            let r = if op == Opcode::FRem {
                put(cx, mc::movap_rr(FpSize::S, XMM15, xm(a)));
                put(cx, mc::movap_rr(FpSize::S, XMM14, xm(b)));
                give_back(cx, lhs, a);
                give_back(cx, rhs, b);
                fmod_scratch(cx)?
            } else {
                arith(cx, op, FpSize::S, xm(a), xm(b))?;
                a
            };
            bfloat_narrow(cx, xm(r));
            cx.finish(inst, r)
        }
    }
}

/// `fmodf` of the `float` in `xmm15` by the one in `xmm14`: both moved to
/// pool temporaries first (an argument register is no place to keep the
/// other argument in), the result's register.
fn fmod_scratch(cx: &mut FnCx<'_, X64>) -> CgResult<PReg> {
    let t1 = cx.temp(Class::Fpr)?;
    put(cx, mc::movap_rr(FpSize::S, xm(t1), XMM15));
    let t2 = cx.temp(Class::Fpr)?;
    put(cx, mc::movap_rr(FpSize::S, xm(t2), XMM14));
    runtime(cx, fmod_of(FpType::Float), &[CallArg::Reg(t1), CallArg::Reg(t2)])
}

/// The routine `frem` of `ty` (`float` or `double`) calls.
fn fmod_of(ty: FpType) -> RuntimeFn {
    match fp_routine(FpOp::Rem, ty, Arch::X86_64) {
        Some(func) => func,
        None => unreachable!("frem of float and double is a call"),
    }
}

/// `%r = fneg %arg`: the sign bit flipped, nothing else, NaNs included
/// (`xorps` with the sign mask, built through `r11` in `xmm15`).
///
/// # Errors
/// [`crate::vector::VECTOR`] for a vector.
pub fn fneg<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let shape = shape(cx, inst.ty());
    if shape == Shape::Vector {
        return crate::lanes::fneg(cx, inst, arg);
    }
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(arg)?;
    negate(cx, shape, xm(a));
    cx.finish(inst, a)
}

/// The sign of the scalar of `shape` in `x` flipped, in place. Writes
/// `r11` and `xmm15`.
pub fn negate(cx: &mut FnCx<'_, X64>, shape: Shape, x: Xmm) {
    match shape {
        Shape::Half | Shape::BFloat => xmm_imm(cx, XMM15, 0x8000, 32),
        Shape::Float | Shape::Vector => xmm_imm(cx, XMM15, 0x8000_0000, 32),
        Shape::Double => xmm_imm(cx, XMM15, 1 << 63, 64),
        Shape::Quad => {
            xmm_imm(cx, XMM15, 1 << 63, 64);
            // The low quadword to the high one: bit 127.
            put(cx, mc::shufp_rri(FpSize::S, XMM15, XMM15, 0x4e));
        }
    }
    put(cx, mc::xorp_rr(FpSize::S, x, XMM15));
}

/// `%r = freeze %arg` of class `Fpr`: a copy (`undef` and `poison` are
/// zero, §4.7).
///
/// # Errors
/// What placing the operand reports.
pub fn freeze<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(arg)?;
    cx.finish(inst, a)
}

/// `%r = <cast> %arg` with a side of class `Fpr` and none of class `Mem`.
///
/// # Errors
/// [`CAST`] for a cast that is no conversion between these types;
/// [`BFLOAT_FP128`]; [`crate::vector::VECTOR`] for a conversion of
/// vectors.
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let op = inst.opcode();
    let (from_ty, to_ty) = (value_ty(cx, arg), inst.ty());
    let from_gpr = cx.class_of(arg) == Class::Gpr;
    let to_gpr = class_of_ty(cx, to_ty) == Class::Gpr;
    if op == Opcode::BitCast {
        return bitcast(cx, inst, arg, from_gpr, to_gpr);
    }
    let from = if from_gpr { None } else { Some(shape(cx, from_ty)) };
    let to = if to_gpr { None } else { Some(shape(cx, to_ty)) };
    if from == Some(Shape::Vector) || to == Some(Shape::Vector) {
        return crate::lanes::cast(cx, inst, arg);
    }
    if from == Some(Shape::Quad) || to == Some(Shape::Quad) {
        if from == Some(Shape::BFloat) || to == Some(Shape::BFloat) {
            return unsupported(BFLOAT_FP128);
        }
        return quad_cast(cx, inst, arg, from, to);
    }
    match (op, from, to) {
        (Opcode::FPExt | Opcode::FPTrunc, Some(a), Some(b)) => fp_convert(cx, inst, arg, op, a, b),
        (Opcode::FPToSI | Opcode::FPToUI, Some(fp), None) => {
            let bits = int_width(cx, to_ty)?;
            to_int(cx, inst, arg, fp, bits, op == Opcode::FPToSI)
        }
        (Opcode::SIToFP | Opcode::UIToFP, None, Some(fp)) => {
            let bits = int_width(cx, from_ty)?;
            from_int(cx, inst, arg, bits, op == Opcode::SIToFP, fp)
        }
        _ => unsupported(CAST),
    }
}

/// The width of an integer type of class `Gpr` (1 to 64 bits).
fn int_width(cx: &FnCx<'_, X64>, ty: TypeId) -> CgResult<u32> {
    match cx.types().int_bits(ty) {
        Some(bits) if (1..=64).contains(&bits) => Ok(bits),
        _ => unsupported(CAST),
    }
}

/// `fpext`/`fptrunc` between two of `half`, `bfloat`, `float`, `double`.
fn fp_convert<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    op: Opcode,
    from: Shape,
    to: Shape,
) -> CgResult<()> {
    use Shape as S;
    let widen = op == Opcode::FPExt;
    match (widen, from, to) {
        (true, S::Float, S::Double) | (false, S::Double, S::Float) => {
            deferred_first(cx, inst, None)?;
            let r = cx.use_reg(arg)?;
            let x = xm(r);
            put(cx, if widen { mc::cvtss2sd_rr(x, x) } else { mc::cvtsd2ss_rr(x, x) });
            cx.finish(inst, r)
        }
        (true, S::Half, S::Float | S::Double) => {
            let r = half_in(cx, arg)?;
            if to == S::Double {
                put(cx, mc::cvtss2sd_rr(xm(r), xm(r)));
            }
            cx.finish(inst, r)
        }
        (false, S::Float | S::Double, S::Half) => {
            let r = narrow_call(cx, from, CallArg::Value(arg))?;
            cx.finish(inst, r)
        }
        (true, S::BFloat, S::Float | S::Double) => {
            deferred_first(cx, inst, None)?;
            let r = cx.use_reg(arg)?;
            bfloat_widen(cx, xm(r));
            if to == S::Double {
                put(cx, mc::cvtss2sd_rr(xm(r), xm(r)));
            }
            cx.finish(inst, r)
        }
        (false, S::Float | S::Double, S::BFloat) => {
            deferred_first(cx, inst, None)?;
            let r = cx.use_reg(arg)?;
            if from == S::Double {
                double_to_float_odd(cx, xm(r))?;
            }
            bfloat_narrow(cx, xm(r));
            cx.finish(inst, r)
        }
        _ => unsupported(CAST),
    }
}

/// `fptosi` (`signed`) or `fptoui` of a scalar of `from` to an integer of
/// `bits` bits.
fn to_int<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    from: Shape,
    bits: u32,
    signed: bool,
) -> CgResult<()> {
    let a = if from == Shape::Half {
        half_in(cx, arg)?
    } else {
        deferred_first(cx, inst, None)?;
        let a = cx.use_reg(arg)?;
        if from == Shape::BFloat {
            bfloat_widen(cx, xm(a));
        }
        a
    };
    let rd = cx.def_reg(inst)?;
    fp_to_int(cx, from.size(), xm(a), bits, signed, gp(rd));
    cx.finish(inst, rd)
}

/// The scalar of `size` in `x` truncated to an integer of `bits` bits in
/// `rd` (the bits above unspecified; out of range is poison): `cvtts2si`
/// of the 32-bit form up to 32 bits signed and 31 unsigned, of the 64-bit
/// form above; for 64 bits unsigned, `llc`'s sequence (the value less
/// `2^63` converted too, and its bits or-ed in when the first conversion
/// overflowed to the integer indefinite). Writes `rcx`, `rdx`, `r11`,
/// `xmm14`, `xmm15` and the flags.
pub fn fp_to_int(cx: &mut FnCx<'_, X64>, size: FpSize, x: Xmm, bits: u32, signed: bool, rd: Reg) {
    let narrow = if signed { bits <= 32 } else { bits <= 31 };
    if narrow || bits < 64 || signed {
        let sz = if narrow { Size::D } else { Size::Q };
        put(cx, mc::cvtts2si_rr(sz, rd, size, x));
        return;
    }
    let two_63 = if size == FpSize::D { TWO_63_F64 } else { TWO_63_F32 };
    xmm_imm(cx, XMM14, two_63, if size == FpSize::D { 64 } else { 32 });
    put(cx, mc::movap_rr(FpSize::S, XMM15, x));
    put(cx, mc::subs_rr(size, XMM15, XMM14));
    put(cx, mc::cvtts2si_rr(Size::Q, RCX, size, XMM15));
    put(cx, mc::cvtts2si_rr(Size::Q, rd, size, x));
    put(cx, mc::mov_rr(Size::Q, RDX, rd));
    put(cx, mc::sar_ri(Size::Q, RDX, 63));
    put(cx, mc::and_rr(Size::Q, RCX, RDX));
    put(cx, mc::or_rr(Size::Q, rd, RCX));
}

/// `sitofp` (`signed`) or `uitofp` of an integer of `bits` bits to a
/// scalar of `to`.
fn from_int<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    bits: u32,
    signed: bool,
    to: Shape,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(arg)?;
    let rd = cx.def_reg(inst)?;
    match to {
        Shape::BFloat => int_to_bfloat(cx, gp(a), bits, signed, xm(rd))?,
        Shape::Half => {
            int_to_fp(cx, gp(a), bits, signed, FpSize::S, xm(rd))?;
            give_back(cx, arg, a);
            let r = half_out(cx, rd)?;
            return cx.finish(inst, r);
        }
        _ => int_to_fp(cx, gp(a), bits, signed, to.size(), xm(rd))?,
    }
    cx.finish(inst, rd)
}

/// The integer of `bits` bits in `r` (the bits above unspecified)
/// extended in place for a conversion: to 32 bits (`signed`, up to 32;
/// unsigned, up to 31) or to 64; the size to convert from.
fn extend_source(cx: &mut FnCx<'_, X64>, r: Reg, bits: u32, signed: bool) -> Size {
    if signed && bits <= 32 {
        extend_to(cx, r, bits, Ext::Sign, Size::D);
        Size::D
    } else if signed {
        extend_to(cx, r, bits, Ext::Sign, Size::Q);
        Size::Q
    } else if bits < 32 {
        extend_to(cx, r, bits, Ext::Zero, Size::D);
        Size::D
    } else if bits == 32 {
        put(cx, mc::mov_rr(Size::D, r, r));
        Size::Q
    } else {
        extend_to(cx, r, bits, Ext::Zero, Size::Q);
        Size::Q
    }
}

/// The integer of `bits` bits in `r` converted to the scalar of `size` in
/// `xd`, rounded once: `cvtsi2s` of the extended value, except for 64
/// bits unsigned with the top bit set, which is halved with its last bit
/// kept sticky, converted and doubled (`llc`'s sequence). Writes `r`,
/// `rcx`, `rdx` and the flags.
pub fn int_to_fp(
    cx: &mut FnCx<'_, X64>,
    r: Reg,
    bits: u32,
    signed: bool,
    size: FpSize,
    xd: Xmm,
) -> CgResult<()> {
    if signed || bits < 64 {
        let from = extend_source(cx, r, bits, signed);
        put(cx, mc::cvtsi2s_rr(size, xd, from, r));
        return Ok(());
    }
    let done = cx.new_label();
    put(cx, mc::mov_rr(Size::Q, RCX, r));
    put(cx, mc::shr_ri(Size::Q, RCX, 1));
    put(cx, mc::mov_rr(Size::D, RDX, r));
    put(cx, mc::and_ri(Size::D, RDX, 1));
    put(cx, mc::or_rr(Size::Q, RCX, RDX));
    put(cx, mc::test_rr(Size::Q, r, r));
    put(cx, mc::cmovcc_rr(Cond::Ns, Size::Q, RCX, r));
    put(cx, mc::cvtsi2s_rr(size, xd, Size::Q, RCX));
    put(cx, mc::test_rr(Size::Q, r, r));
    branch_cond(cx, Cond::Ns, done)?;
    put(cx, mc::adds_rr(size, xd, xd));
    cx.bind(done)
}

/// The integer of `bits` bits in `r` (the bits above unspecified)
/// converted to a `bfloat` in `xd`, rounded once: up to 16 bits through
/// `float` (exact); up to 32 through `double` (exact), then to `float`
/// rounded to odd; wider, the magnitude converted to `double` rounded to
/// odd by hand (at `2^53` or more the low 12 bits are dropped, which
/// leaves an exact conversion, and folded into the last bit), the sign
/// put back, then as from 32. Writes `r`, `rax`, `rcx`, `rdx`, `r11`,
/// `xmm14`, `xmm15` and the flags.
fn int_to_bfloat(cx: &mut FnCx<'_, X64>, r: Reg, bits: u32, signed: bool, xd: Xmm) -> CgResult<()> {
    if bits <= 16 {
        let from = extend_source(cx, r, bits, signed);
        put(cx, mc::cvtsi2s_rr(FpSize::S, xd, from, r));
        bfloat_narrow(cx, xd);
        return Ok(());
    }
    if bits <= 32 {
        let from = extend_source(cx, r, bits, signed);
        put(cx, mc::cvtsi2s_rr(FpSize::D, xd, from, r));
    } else {
        extend_source(cx, r, bits, signed);
        // `rcx` = the magnitude (the value, unsigned).
        put(cx, mc::mov_rr(Size::Q, RCX, r));
        if signed {
            put(cx, mc::neg_r(Size::Q, RCX));
            put(cx, mc::cmovcc_rr(Cond::S, Size::Q, RCX, r));
        }
        let (small, sign) = (cx.new_label(), cx.new_label());
        put(cx, mc::mov_rr(Size::Q, RAX, RCX));
        put(cx, mc::shr_ri(Size::Q, RAX, 53));
        branch_cond(cx, Cond::E, small)?;
        // `rdx` = the dropped bits; the rest, halved, converts exactly.
        put(cx, mc::mov_rr(Size::D, RDX, RCX));
        put(cx, mc::and_ri(Size::D, RDX, 0xfff));
        put(cx, mc::and_ri(Size::Q, RCX, -0x1000));
        put(cx, mc::shr_ri(Size::Q, RCX, 1));
        put(cx, mc::cvtsi2s_rr(FpSize::D, xd, Size::Q, RCX));
        put(cx, mc::adds_rr(FpSize::D, xd, xd));
        put(cx, mc::movd_rx(Size::Q, RAX, xd));
        put(cx, mc::test_rr(Size::D, RDX, RDX));
        put(cx, mc::setcc_r(Cond::Ne, RDX));
        put(cx, mc::movzx_rr(Size::D, RDX, Size::B, RDX));
        put(cx, mc::or_rr(Size::Q, RAX, RDX));
        put(cx, mc::movd_xr(Size::Q, xd, RAX));
        crate::common::branch(cx, sign)?;
        cx.bind(small)?;
        put(cx, mc::cvtsi2s_rr(FpSize::D, xd, Size::Q, RCX));
        cx.bind(sign)?;
        if signed {
            put(cx, mc::mov_rr(Size::Q, RAX, r));
            put(cx, mc::shr_ri(Size::Q, RAX, 63));
            put(cx, mc::shl_ri(Size::Q, RAX, 63));
            put(cx, mc::movd_rx(Size::Q, RDX, xd));
            put(cx, mc::or_rr(Size::Q, RDX, RAX));
            put(cx, mc::movd_xr(Size::Q, xd, RDX));
        }
    }
    double_to_float_odd(cx, xd)?;
    bfloat_narrow(cx, xd);
    Ok(())
}

/// A conversion with an `fp128` side and no `bfloat` side: one runtime
/// call, as `llc` makes it. An integer of a width other than 32 and 64 is
/// extended first (§2.2); the `fptoui`/`fptosi` result above the width
/// is unspecified, and comes back in `rax`, moved to the result's
/// register.
fn quad_cast<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    from: Option<Shape>,
    to: Option<Shape>,
) -> CgResult<()> {
    use Shape as S;
    let op = inst.opcode();
    deferred_first(cx, inst, None)?;
    let (func, arg_of) = match (op, from, to) {
        (Opcode::FPExt, Some(s @ (S::Half | S::Float | S::Double)), Some(S::Quad)) => {
            (fp_ext_routine(s.fp_type(), FpType::Fp128, Arch::X86_64), CallArg::Value(arg))
        }
        (Opcode::FPTrunc, Some(S::Quad), Some(s @ (S::Half | S::Float | S::Double))) => {
            (fp_trunc_routine(FpType::Fp128, s.fp_type(), Arch::X86_64), CallArg::Value(arg))
        }
        (Opcode::FPToSI | Opcode::FPToUI, Some(S::Quad), None) => {
            let bits = int_width(cx, inst.ty())?;
            let signed = op == Opcode::FPToSI;
            let func = fp_to_int_routine(FpType::Fp128, signed, bits, Arch::X86_64);
            let Some(func) = func else { return cx.bug("an fp128 conversion without a routine") };
            let r = runtime(cx, func, &[CallArg::Value(arg)])?;
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mov_rr(Size::Q, gp(rd), gp(r)));
            cx.free_temp(r);
            return cx.finish(inst, rd);
        }
        (Opcode::SIToFP | Opcode::UIToFP, None, Some(S::Quad)) => {
            let bits = int_width(cx, value_ty(cx, arg))?;
            let signed = op == Opcode::SIToFP;
            let func = int_to_fp_routine(signed, bits, FpType::Fp128, Arch::X86_64);
            if bits == 32 || bits == 64 {
                (func, CallArg::Value(arg))
            } else {
                let a = cx.use_reg(arg)?;
                let t = cx.temp(Class::Gpr)?;
                put(cx, mc::mov_rr(Size::Q, gp(t), gp(a)));
                give_back(cx, arg, a);
                let ext = if signed { Ext::Sign } else { Ext::Zero };
                let size = if bits < 32 { Size::D } else { Size::Q };
                extend_to(cx, gp(t), bits, ext, size);
                (func, CallArg::Reg(t))
            }
        }
        _ => return unsupported(CAST),
    };
    let Some(func) = func else { return cx.bug("an fp128 conversion without a routine") };
    let r = runtime(cx, func, &[arg_of])?;
    cx.finish(inst, r)
}

/// `bitcast` with a side of class `Fpr`: the bits unchanged. Between two
/// `Fpr`-class types it is a copy; with a `Gpr` side, `movd`/`movq` of
/// the 32- or 64-bit view (`half` and `bfloat` in the low 16 bits; the
/// bits above are unspecified on both sides).
fn bitcast<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    from_gpr: bool,
    to_gpr: bool,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(arg)?;
    let bits = cx.types().size_in_bits_of(inst.ty());
    let size = if bits <= 32 { Size::D } else { Size::Q };
    match (from_gpr, to_gpr) {
        (false, false) => cx.finish(inst, a),
        (true, false) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::movd_xr(size, xm(rd), gp(a)));
            cx.finish(inst, rd)
        }
        (false, true) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::movd_rx(size, gp(rd), xm(a)));
            cx.finish(inst, rd)
        }
        (true, true) => cx.bug("a bitcast between two general registers in the float group"),
    }
}

/// `%r = fcmp <pred> %lhs, %rhs`: deferred when the next instruction
/// consumes it (§4.6), else `setcc` (two and an `and`/`or` for `oeq` and
/// `une`) after [`flags`].
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
    let _ = (pred, rhs);
    compare_shape(cx, lhs)?;
    if cx.may_defer(inst) {
        return cx.defer(inst);
    }
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

/// `%r = icmp <pred> %lhs, %rhs` of vectors in vector registers.
///
/// # Errors
/// Always [`crate::vector::VECTOR`] (WP-132).
pub fn vector_icmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (cx, inst, pred, lhs, rhs);
    unsupported(crate::vector::VECTOR)
}

/// The branches of `br` on the condition(s) `c`: to `if_true` when it
/// holds, else to `if_false`; a block emitted next is not branched to.
fn branch_fp(
    cx: &mut FnCx<'_, X64>,
    c: FpCond,
    if_true: BlockId,
    if_false: BlockId,
) -> CgResult<()> {
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

/// `br i1 %cond, label %if_true, label %if_false` whose condition is the
/// deferred `fcmp` `compare` (the hook in `control::cond_br`; the targets
/// differ): [`flags`], then `jcc` on its condition(s). A compare of
/// another class (an `x86_fp80` one, should WP-131 defer it) is emitted
/// by its own lowering and its bit 0 tested.
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
    if crate::x87::compares(cx, compare) {
        return crate::x87::cond_br(cx, inst, compare, cond, if_true, if_false);
    }
    let fused =
        matches!(compare.kind(), InstKind::FCmp { lhs, .. } if cx.class_of(lhs) == Class::Fpr);
    if !fused {
        deferred_first(cx, inst, None)?;
        let r = gp(cx.use_reg(cond)?);
        put(cx, mc::test_ri(Size::B, r, 1));
        return branch_fp(cx, FpCond::One(Cond::Ne), if_true, if_false);
    }
    deferred_first(cx, inst, Some(cond))?;
    let c = crate::control::at_compare(cx, compare, |cx| flags(cx, compare))?;
    branch_fp(cx, c, if_true, if_false)
}

/// `%r = select i1 %cond, %if_true, %if_false` of class `Fpr`: the true
/// arm's register, replaced by the false arm's (`movaps`, all 128 bits)
/// unless the condition holds, which is tested by `jcc` (SSE2 has no
/// conditional move). A deferred `fcmp` condition is fused ([`flags`]);
/// otherwise bit 0 of the condition is tested (§2.2).
///
/// # Errors
/// [`crate::vector::VECTOR`] for a vector condition; what [`flags`]
/// reports.
pub fn select<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    if !matches!(cx.types().kind(value_ty(cx, cond)), TypeKind::Int(1)) {
        return unsupported(crate::vector::VECTOR);
    }
    let fused = cx.deferred(cond).filter(
        |d| matches!(d.kind(), InstKind::FCmp { lhs, .. } if cx.class_of(lhs) == Class::Fpr),
    );
    deferred_first(cx, inst, fused.map(|_| cond))?;
    let c = if let Some(compare) = fused {
        flags(cx, compare)?
    } else {
        let r = cx.use_reg(cond)?;
        put(cx, mc::test_ri(Size::B, gp(r), 1));
        give_back(cx, cond, r);
        FpCond::One(Cond::Ne)
    };
    let a = cx.use_reg(if_true)?;
    let b = cx.use_reg(if_false)?;
    let (keep, take) = (cx.new_label(), cx.new_label());
    let last = match c {
        FpCond::One(c) => c,
        FpCond::All(c1, c2) => {
            branch_cond(cx, c1.invert(), take)?;
            c2
        }
        FpCond::Any(c1, c2) => {
            branch_cond(cx, c1, keep)?;
            c2
        }
    };
    branch_cond(cx, last, keep)?;
    cx.bind(take)?;
    put(cx, mc::movap_rr(FpSize::S, xm(a), xm(b)));
    cx.bind(keep)?;
    cx.finish(inst, a)
}

/// Glue (`Target::materialize` of a vector register): the constant
/// `value` of class `Fpr` in `rd`. A scalar up to 64 bits goes through
/// `r11` (`mov` + `movd`/`movq`, which zero the bits above); zero,
/// `undef` and `poison` are `mov r11d, 0` + `movq`; an `fp128` or vector
/// constant whose upper half is not zero is loaded from a data island
/// (`movups`). Flags are kept.
///
/// # Errors
/// `x64:todo:constant` for a vector constant holding an address.
pub fn materialize(cx: &mut FnCx<'_, X64>, rd: Xmm, value: Value) -> CgResult<()> {
    let ValueKind::Const(id) = value.kind() else {
        return cx.bug("materialize of an Fpr value that is no constant");
    };
    match cx.module.consts.get(id) {
        Const::Float { ty, bits } => {
            let width = if matches!(cx.types().kind(ty), TypeKind::Double) { 64 } else { 32 };
            xmm_imm(cx, rd, bits, width);
            Ok(())
        }
        Const::WideFloat { bits, .. } => wide_const(cx, rd, &bits.to_le_bytes()),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => {
            xmm_imm(cx, rd, 0, 64);
            Ok(())
        }
        _ => {
            let mut image = Vec::new();
            let mut fixups = Vec::new();
            let flattened = cx.module.consts.flatten(cx.types(), id, &mut image, &mut fixups);
            if flattened.is_err() || !fixups.is_empty() || image.len() > 16 {
                return unsupported(crate::common::CONSTANT);
            }
            image.resize(16, 0);
            wide_const(cx, rd, &image)
        }
    }
}

/// The 16 bytes `image` in `rd`: through `r11` when the upper 8 are zero,
/// else from a data island.
fn wide_const(cx: &mut FnCx<'_, X64>, rd: Xmm, image: &[u8]) -> CgResult<()> {
    let mut low = [0u8; 8];
    low.copy_from_slice(&image[..8]);
    if image[8..].iter().all(|&b| b == 0) {
        xmm_imm(cx, rd, u64::from_le_bytes(low), 64);
        return Ok(());
    }
    let label = cx.island(image, 16)?;
    put_rip(cx, mc::movup_rm(FpSize::S, rd, Mem::rip(0)), label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_predicate_has_conditions() {
        for &pred in FloatPredicate::ALL {
            let (swap, cond) = ucomis_conditions(pred);
            // The swapped ones are the four that test "less than".
            if swap {
                assert!(matches!(
                    pred,
                    FloatPredicate::Olt
                        | FloatPredicate::Ole
                        | FloatPredicate::Ugt
                        | FloatPredicate::Uge
                ));
            }
            let _ = cond;
        }
    }
}
