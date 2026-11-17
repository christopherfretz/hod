//! `llvm.is.fpclass` and `llvm.modf` of `float`, `double` and `x86_fp80`
//! (WP-337, `spec/codegen-x86.md` "Floating-point classes and `modf`").
//!
//! # `is.fpclass`
//!
//! Integer tests of the operand's home, whatever computes the type (the
//! x87, or SSE / SSE2 in `xmm`): a home holds the value rounded to its
//! type (the x87's 80-bit copy of a run is never read here), which is
//! what `llc` classifies (it stores the value at its type and tests the
//! bits, or for the masks "NaN" and "not NaN" compares the value with
//! itself, which rounding does not change). The class's index in the
//! test mask (`LangRef`'s bits: 0 signalling NaN, 1 quiet NaN, 2
//! negative infinity, ... 9 positive infinity) is computed into `ecx`,
//! then `bt` of the mask in `edx`. An `x86_fp80` encoding only the x87
//! has (a non-zero exponent without the integer bit: an unnormal, a
//! pseudo-infinity, a pseudo-NaN) gets index 10, whose bit is
//! [`invalid_in`]; a pseudo-denormal (exponent 0, integer bit set) 11
//! (positive) or 12 (negative), whose bits are [`pseudo_denormal_in`]
//! (`llc` 22's answer). Writes `eax`, `ecx`, `edx`; the result is the only pool
//! register. Branches inside the lowering only: `llc` has none (no
//! barrier, measured over all 1024 masks).
//!
//! # `modf`
//!
//! `modff` / `modf` / `modfl` with the operand copied to the outgoing
//! area at its width and the address of the result's second field (the
//! integral part) after it; the fractional part arrives in `st0` and is
//! popped to the first field at the type's width (exact: a fractional
//! part is representable in its type). A call: an x87 barrier
//! (`x87::barrier`).

use hod_cg::{CallTarget, CgResult, FnCx, FrameRef, RuntimeFn, SymRef};
use hod_ir::{Const, Inst, InstKind, IntPredicate, Intrinsic, TypeId, TypeKind, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Size, X87Size};

use crate::common::{
    EAX, ECX, EDX, ESP, branch, branch_cond, const_int, frame_address, frame_inst, gp, mov_imm,
    put, unsupported, value_ty,
};
use crate::target::X86;

/// A floating intrinsic hod does not lower here (as `call.rs`'s).
const INTRINSIC: &str = crate::call::INTRINSIC;

/// The scalar floating types of this module.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    F32,
    F64,
    F80,
}

impl Ty {
    fn of(cx: &FnCx<'_, X86>, ty: TypeId) -> Option<Ty> {
        match cx.types().kind(ty) {
            TypeKind::Float => Some(Ty::F32),
            TypeKind::Double => Some(Ty::F64),
            TypeKind::X86Fp80 => Some(Ty::F80),
            _ => None,
        }
    }

    /// The bytes of the encoding (`x86_fp80`'s image).
    const fn bytes(self) -> u32 {
        match self {
            Ty::F32 => 4,
            Ty::F64 => 8,
            Ty::F80 => 10,
        }
    }

    /// The bytes an argument of the type takes on the stack.
    const fn arg_bytes(self) -> u32 {
        match self {
            Ty::F32 => 4,
            Ty::F64 => 8,
            Ty::F80 => 12,
        }
    }
}

/// Whether `inst` is this module's: `is.fpclass` of a scalar `float`,
/// `double` or `x86_fp80`; `modf` of one of them. A vector, `half` and
/// the other types stay with their modules' refusals. Also `icmp` of an
/// `i80` with zero ([`i80_sign_test`]).
pub fn owns(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    let site = match inst.kind() {
        InstKind::Call(site) => site,
        InstKind::ICmp { lhs, rhs, .. } => return is_i80(cx, lhs) && is_zero(cx, rhs),
        _ => return false,
    };
    match (site.intrinsic(), site.args()) {
        (Some(Intrinsic::IsFpClass | Intrinsic::Modf), &[x, ..]) => {
            Ty::of(cx, value_ty(cx, x)).is_some()
        }
        _ => false,
    }
}

/// Lowers an instruction [`owns`] takes.
///
/// # Errors
/// [`INTRINSIC`] for a mask that is not a constant or a `modf` result
/// that is not the pair of the operand's type; what the operands report.
pub fn lower<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
    let site = match inst.kind() {
        InstKind::Call(site) => site,
        InstKind::ICmp { pred, lhs, .. } => return i80_sign_test(cx, inst, pred, lhs),
        _ => return cx.bug("fpclass::lower of no call"),
    };
    match (site.intrinsic(), site.args()) {
        (Some(Intrinsic::IsFpClass), &[x, mask]) => is_fpclass(cx, inst, x, mask),
        (Some(Intrinsic::Modf), &[x]) => modf(cx, inst, x),
        _ => unsupported(INTRINSIC),
    }
}

/// Whether `llvm.is.fpclass` with `test` holds for an `x86_fp80`
/// unnormal, pseudo-infinity or pseudo-NaN, as `llc` 19 and 22 expand it
/// (measured over all 1024 masks on i686, as on x86-64): when the test
/// has both NaN classes, or is everything but one NaN class.
pub const fn invalid_in(test: u32) -> bool {
    let test = test & 0x3ff;
    test & 3 == 3 || test == 0x3fd || test == 0x3fe
}

/// The masks `llc` 22 tests with an x87 compare of the value rather than
/// integer tests of its bits (measured over all 1024; `llc` 19 has only
/// 3, 99, 924, 1020).
const COMPARED: [u32; 10] = [3, 7, 99, 243, 508, 515, 780, 924, 1016, 1020];

/// The tests `llc` expands as the negation of their complement
/// (`invertFPClassTestIfSimpler`): one class, one class of one sign,
/// the finite classes (of one sign), zero or NaN, subnormal or zero,
/// subnormal or zero or NaN.
const INVERTED: [u32; 21] = [
    0x003, 0x001, 0x002, 0x204, 0x200, 0x004, 0x108, 0x100, 0x008, 0x090, 0x080, 0x010, 0x060,
    0x040, 0x020, 0x1f8, 0x1c0, 0x038, 0x063, 0x0f0, 0x0f3,
];

/// Whether `llvm.is.fpclass` with `test` holds for an `x86_fp80`
/// pseudo-denormal (exponent 0, integer bit set) of sign `negative`, as
/// `llc` 22 lowers it (measured over all 1024 masks): a compared mask
/// ([`COMPARED`]) sees its value, a normal number; the integer tests
/// count it as a NaN when the test has both NaN classes (as for
/// `glibc`) and as zero-or-subnormal when it has all four of those
/// classes, the test inverted as [`INVERTED`] says. `llc` 19 compares
/// fewer masks: it differs at 7, 243, 515 and 780.
pub fn pseudo_denormal_in(test: u32, negative: bool) -> bool {
    let test = test & 0x3ff;
    if COMPARED.contains(&test) {
        return test & if negative { 0x008 } else { 0x100 } != 0;
    }
    let inverted = INVERTED.contains(&(!test & 0x3ff));
    let t = if inverted { !test & 0x3ff } else { test };
    (t & 3 == 3 || t & 0xf0 == 0xf0) != inverted
}

/// `%r = call i1 @llvm.is.fpclass(x, mask)`.
fn is_fpclass<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, x: Value, mask: Value) -> CgResult<()> {
    let Some(ty) = Ty::of(cx, value_ty(cx, x)) else { return unsupported(INTRINSIC) };
    let Some(test) = const_int(cx, mask) else { return unsupported(INTRINSIC) };
    let a = cx.use_mem(x)?;
    let rd = cx.def_reg(inst)?;
    let (signed, done) = (cx.new_label(), cx.new_label());
    let mut bits = test & 0x3ff;
    match ty {
        Ty::F32 | Ty::F64 => {
            // The magnitude's high word in `eax` (a `double`'s low word
            // folded into bit 0: no bound below uses it).
            let (high, min_normal, inf, quiet) = match ty {
                Ty::F32 => (0, 0x0080_0000, 0x7f80_0000, 0x7fc0_0000),
                _ => (4, 0x0010_0000, 0x7ff0_0000, 0x7ff8_0000),
            };
            frame_inst(cx, a.offset_by(high), |m| mc::mov_rm(Size::D, EAX, m));
            put(cx, mc::and_ri(Size::D, EAX, 0x7fff_ffff));
            if ty == Ty::F64 {
                frame_inst(cx, a, |m| mc::cmp_mi(Size::D, m, 1));
                put(cx, mc::sbb_rr(Size::D, ECX, ECX));
                put(cx, mc::inc_r(Size::D, ECX));
                put(cx, mc::or_rr(Size::D, EAX, ECX));
            }
            mov_imm(cx, ECX, 6);
            put(cx, mc::test_rr(Size::D, EAX, EAX));
            branch_cond(cx, Cond::E, signed)?;
            mov_imm(cx, ECX, 7);
            put(cx, mc::cmp_ri(Size::D, EAX, min_normal));
            branch_cond(cx, Cond::B, signed)?;
            mov_imm(cx, ECX, 8);
            put(cx, mc::cmp_ri(Size::D, EAX, inf));
            branch_cond(cx, Cond::B, signed)?;
            mov_imm(cx, ECX, 9);
            branch_cond(cx, Cond::E, signed)?;
            // A NaN: 0 (signalling) below the quiet bit, else 1.
            mov_imm(cx, ECX, 0);
            put(cx, mc::cmp_ri(Size::D, EAX, quiet));
            branch_cond(cx, Cond::B, done)?;
            mov_imm(cx, ECX, 1);
            branch(cx, done)?;
        }
        Ty::F80 => {
            let (zero_exp, max_exp, invalid) = (cx.new_label(), cx.new_label(), cx.new_label());
            frame_inst(cx, a.offset_by(8), |m| mc::movzx_rm(Size::D, EAX, Size::W, m));
            put(cx, mc::and_ri(Size::D, EAX, 0x7fff));
            branch_cond(cx, Cond::E, zero_exp)?;
            put(cx, mc::cmp_ri(Size::D, EAX, 0x7fff));
            branch_cond(cx, Cond::E, max_exp)?;
            // A normal exponent: normal with the integer bit, else an
            // unnormal.
            frame_inst(cx, a.offset_by(7), |m| mc::test_mi(Size::B, m, 0x80));
            branch_cond(cx, Cond::E, invalid)?;
            mov_imm(cx, ECX, 8);
            branch(cx, signed)?;
            cx.bind(zero_exp)?;
            // A pseudo-denormal (the integer bit set): 11, or 12 when
            // negative.
            let pseudo_denormal = cx.new_label();
            frame_inst(cx, a.offset_by(7), |m| mc::test_mi(Size::B, m, 0x80));
            branch_cond(cx, Cond::Ne, pseudo_denormal)?;
            // Zero or a denormal.
            frame_inst(cx, a, |m| mc::mov_rm(Size::D, EAX, m));
            frame_inst(cx, a.offset_by(4), |m| mc::or_rm(Size::D, EAX, m));
            mov_imm(cx, ECX, 6);
            branch_cond(cx, Cond::E, signed)?;
            mov_imm(cx, ECX, 7);
            branch(cx, signed)?;
            cx.bind(max_exp)?;
            // An infinity or a NaN; without the integer bit a
            // pseudo-infinity or a pseudo-NaN.
            frame_inst(cx, a.offset_by(4), |m| mc::mov_rm(Size::D, EAX, m));
            put(cx, mc::test_rr(Size::D, EAX, EAX));
            branch_cond(cx, Cond::Ns, invalid)?;
            put(cx, mc::and_ri(Size::D, EAX, 0x7fff_ffff));
            frame_inst(cx, a, |m| mc::or_rm(Size::D, EAX, m));
            mov_imm(cx, ECX, 9);
            branch_cond(cx, Cond::E, signed)?;
            // A NaN: 1 (quiet) with bit 62 of the significand, else 0.
            mov_imm(cx, ECX, 0);
            frame_inst(cx, a.offset_by(7), |m| mc::test_mi(Size::B, m, 0x40));
            branch_cond(cx, Cond::E, done)?;
            mov_imm(cx, ECX, 1);
            branch(cx, done)?;
            cx.bind(pseudo_denormal)?;
            mov_imm(cx, ECX, 11);
            frame_inst(cx, a.offset_by(9), |m| mc::test_mi(Size::B, m, 0x80));
            branch_cond(cx, Cond::E, done)?;
            mov_imm(cx, ECX, 12);
            branch(cx, done)?;
            cx.bind(invalid)?;
            mov_imm(cx, ECX, 10);
            branch(cx, done)?;
            bits |= u32::from(invalid_in(test)) << 10
                | u32::from(pseudo_denormal_in(test, false)) << 11
                | u32::from(pseudo_denormal_in(test, true)) << 12;
        }
    }
    cx.bind(signed)?;
    // A negative value's index is `11 - i` (zero 6 to 5, ..., infinity 9
    // to 2).
    frame_inst(cx, a.offset_by(ty.bytes() - 1), |m| mc::test_mi(Size::B, m, 0x80));
    branch_cond(cx, Cond::E, done)?;
    put(cx, mc::neg_r(Size::D, ECX));
    put(cx, mc::add_ri(Size::D, ECX, 11));
    cx.bind(done)?;
    mov_imm(cx, EDX, bits);
    put(cx, mc::bt_rr(Size::D, EDX, ECX));
    let r = gp(rd);
    let byte = if crate::reg::has_low_byte(r) { r } else { ECX };
    put(cx, mc::setcc_r(Cond::B, byte));
    put(cx, mc::movzx_rr(Size::D, r, Size::B, byte));
    cx.finish(inst, rd)
}

/// `%r = call { T, T } @llvm.modf(x)`: the libm routine of `T` with the
/// operand and the address of `%r`'s second field, the fractional part
/// popped from `st0` to the first.
fn modf<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, x: Value) -> CgResult<()> {
    let Some(ty) = Ty::of(cx, value_ty(cx, x)) else { return unsupported(INTRINSIC) };
    let pair = cx
        .types()
        .struct_fields(inst.ty())
        .is_some_and(|fs| fs.len() == 2 && fs.iter().all(|&f| f == value_ty(cx, x)));
    if !pair {
        return unsupported(INTRINSIC);
    }
    let (func, size) = match ty {
        Ty::F32 => (RuntimeFn::Modff, X87Size::S),
        Ty::F64 => (RuntimeFn::Modf, X87Size::D),
        Ty::F80 => (RuntimeFn::Modfl, X87Size::T),
    };
    let second = cx.types().field_offset(inst.ty(), 1) as u32;
    let a = cx.use_mem(x)?;
    let r = cx.def_mem(inst)?;
    let area = (ty.arg_bytes() + 4).next_multiple_of(16);
    put(cx, mc::sub_ri(Size::D, ESP, i64::from(area)));
    crate::common::copy_frame(cx, FrameRef::Outgoing(0), a, ty.bytes());
    frame_address(cx, EAX, r.offset_by(second));
    frame_inst(cx, FrameRef::Outgoing(ty.arg_bytes()), |m| mc::mov_mr(Size::D, m, EAX));
    let at = cx.offset();
    // `pic`'s call: through the PLT with the GOT pointer in `ebx` under
    // PIC (`spec/codegen-x86.md` §9.2).
    crate::pic::emit_call(cx, CallTarget::Sym(SymRef::Runtime(func)), 0)?;
    let len = cx.offset() - at;
    cx.record_call_site(at, len);
    put(cx, mc::add_ri(Size::D, ESP, i64::from(area)));
    frame_inst(cx, r, |m| mc::fstp_m(size, m));
    Ok(())
}

// ---- `i80` ----

/// Whether `value` is an `i80`: only `bitcast` from `x86_fp80` makes one
/// on i386 (`x87.rs`), into a slot whose first 10 bytes are its bits.
fn is_i80(cx: &FnCx<'_, X86>, value: Value) -> bool {
    cx.types().int_bits(value_ty(cx, value)) == Some(80)
}

/// Whether `value` is the constant zero (of any width).
fn is_zero(cx: &FnCx<'_, X86>, value: Value) -> bool {
    let ValueKind::Const(id) = value.kind() else { return false };
    match cx.module.consts.get(id) {
        Const::WideInt { words, .. } => words.iter().all(|&w| w == 0),
        Const::Int { value, .. } => value == 0,
        Const::Zero(_) => true,
        _ => false,
    }
}

/// `icmp <pred> i80 %x, 0`: what clang writes for `signbit` of a `long
/// double` (`slt`), and the other predicates against zero, from two
/// facts of the 10 bytes: negative (bit 79) in `ecx`, zero in `edx`.
/// Every other `i80` operation stays `x86:i64:wide`.
fn i80_sign_test<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    pred: IntPredicate,
    x: Value,
) -> CgResult<()> {
    use IntPredicate as P;
    let a = cx.use_mem(x)?;
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    frame_inst(cx, a.offset_by(8), |m| mc::movzx_rm(Size::D, EAX, Size::W, m));
    frame_inst(cx, a, |m| mc::or_rm(Size::D, EAX, m));
    frame_inst(cx, a.offset_by(4), |m| mc::or_rm(Size::D, EAX, m));
    put(cx, mc::setcc_r(Cond::E, EDX));
    put(cx, mc::movzx_rr(Size::D, EDX, Size::B, EDX));
    frame_inst(cx, a.offset_by(9), |m| mc::movzx_rm(Size::D, ECX, Size::B, m));
    put(cx, mc::shr_ri(Size::D, ECX, 7));
    // (the fact, or with the other, negated)
    let (fact, or_other, negate) = match pred {
        P::Eq | P::Ule => (EDX, false, false),
        P::Ne | P::Ugt => (EDX, false, true),
        P::Slt => (ECX, false, false),
        P::Sge => (ECX, false, true),
        P::Sle => (ECX, true, false),
        P::Sgt => (ECX, true, true),
        // Nothing is below zero unsigned.
        P::Ult | P::Uge => {
            mov_imm(cx, r, u32::from(pred == P::Uge));
            return cx.finish(inst, rd);
        }
    };
    put(cx, mc::mov_rr(Size::D, r, fact));
    if or_other {
        put(cx, mc::or_rr(Size::D, r, EDX));
    }
    if negate {
        put(cx, mc::xor_ri(Size::D, r, 1));
    }
    cx.finish(inst, rd)
}
