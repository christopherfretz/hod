//! Vectors of `i1`, masks (`spec/codegen.md` §5.6.13 "Vectors
//! (WP-132)"): `<N x i1>` with `N` up to 64 as a bit-packed
//! general-register value, the instructions that make, combine and
//! consume one, and the `llvm.vector.reduce.*` family on masks. The
//! representation and the rules are AArch64's (`hod_a64::mask`).
//!
//! **Owner: WP-132.** Tests: `tests/mask.rs`. Hook: the first step of
//! `Target::lower` ([`lower`]); the class of a vector type
//! (`target::vector_class`).
//!
//! # The representation
//!
//! A mask of `N ≤ 64` lanes is of class `Gpr`: lane `i` is bit `i` of the
//! register, and the bits from `N` up are **unspecified**, as they are
//! for an `iN`. This is LLVM's in-memory image of the vector (element 0
//! in the lowest bit, `⌈N/8⌉` bytes), so `bitcast`, `load`, `phi`,
//! `freeze`, `select` on a scalar condition, `extractvalue` and
//! `insertvalue` are the integer, memory and aggregate groups', unchanged
//! ([`lower`] returns `None` for them); `store` clears the bits from `N`
//! up to the byte boundary first ([`store`]), as `llc` does.
//!
//! Everything that looks at lanes comes here first: element-wise
//! arithmetic, `icmp` and `fcmp` of vectors (whose results are masks),
//! `select` on a vector condition, the casts between masks and lanes,
//! `extractelement`, `insertelement` and `shufflevector` of masks, stores
//! of masks and the reductions of masks. A mask of more than 64 lanes is
//! of class `Mem`, and every operation on its lanes is refused as
//! [`vector::MASK_WIDE`]; an intrinsic of `i1` lanes other than a
//! reduction as [`MASK`]; a mask crossing a call is `vcall`'s.
//!
//! A constant mask operand of these lowerings is put in a register by its
//! bits (undefined lanes 0). Elsewhere (a `phi` incoming, a call
//! argument, a returned constant) a constant mask other than zero is the
//! integer group's `x64:todo:constant` (`common::materialize_gpr_const`).
//!
//! # Lane rules on one bit
//!
//! | operation | bits |
//! |---|---|
//! | `add`, `sub`, `xor` | `xor` |
//! | `mul`, `and` | `and` |
//! | `or` | `or` |
//! | `shl`, `lshr`, `ashr`, `udiv`, `sdiv` | the left operand |
//! | `urem`, `srem` | 0 |
//! | `icmp eq` / `ne` | `~(a ^ b)` / `a ^ b` |
//! | `icmp ugt`, `slt` | `a & ~b`; `ult`, `sgt`: `b & ~a` |
//! | `icmp uge`, `sle` | `a \| ~b`; `ule`, `sge`: `b \| ~a` |
//! | `select` | `(c & t) \| (f & ~c)` |
//! | `reduce.or`, `umax`, `smin` | any bit of the `N` set |
//! | `reduce.and`, `umin`, `smax`, `mul` | all `N` bits set |
//! | `reduce.xor`, `add` | the parity of the `N` bits |
//!
//! # Compares of lanes
//!
//! `icmp`: per lane, both lanes zero-extended (sign-extended for a signed
//! predicate) to 64 bits in `rax`/`rcx`, `cmp`, `setcc`, the bit or-ed
//! into the result. `fcmp`: per lane, the lane's **class** is found, one
//! of unordered, less, equal, greater (one-hot: 8, 4, 1, 2, LLVM's
//! predicate bits), and the result bit is whether the predicate's code
//! has it. `float`/`double` lanes: `ucomiss`/`ucomisd` and `setcc`;
//! `half`/`bfloat` lanes without any conversion: NaN when the magnitude
//! is above the infinity's, else the order of the sign-magnitude bits as
//! a signed key (`+0 = -0`).
//!
//! # Registers
//!
//! `rax`, `rcx`, `rdx`, `r11`, `xmm14`, `xmm15`, the flags; the
//! operands' and the result's registers, and pool temporaries for
//! constant operands.

use hod_cg::{CgResult, Class, FnCx, FrameRef, PReg, lanes};
use hod_ir::{
    Const, ConstId, FloatPredicate, Inst, InstKind, IntPredicate, Intrinsic, Opcode, TypeId,
    TypeKind, TypeTable, Value, ValueKind,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Mem, Reg, Size};

use crate::common::{
    R11, XMM14, XMM15, deferred_first, fpr_load, frame_inst, gp, gpr_load, mov_imm, put,
    unsupported, value_ty,
};
use crate::target::X64;
use crate::vector::{self, Elem, Index, Lanes};

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;

/// An operation on the lanes of a mask that no lowering takes: an
/// intrinsic of `i1` lanes other than a reduction.
pub const MASK: &str = "x64:vector-mask";

/// A reduction (`llvm.vector.reduce.*`) of a mask whose operand list is
/// not the intrinsic's, or a floating-point fold of one.
pub const REDUCE: &str = crate::lanes::REDUCE;

/// The longest mask of class `Gpr`: one bit per element in 64 bits.
pub const MAX_MASK: u32 = 64;

/// Whether `ty` is a mask of class `Gpr`.
pub fn is_mask(types: &TypeTable, ty: TypeId) -> bool {
    mask_lanes(types, ty).is_some_and(|n| n <= MAX_MASK)
}

/// The lane count of `ty` if it is a fixed vector of `i1`.
pub fn mask_lanes(types: &TypeTable, ty: TypeId) -> Option<u32> {
    match types.kind(ty) {
        TypeKind::Vector { elem: TypeId::I1, len } => Some(len),
        _ => None,
    }
}

/// The low `n` bits set (`n` from 1 to 64).
pub fn low_bits(n: u32) -> u64 {
    if n >= 64 { u64::MAX } else { (1u64 << n) - 1 }
}

/// The lanes of the mask type `ty`, or [`vector::MASK_WIDE`] past
/// [`MAX_MASK`].
fn mask_of(cx: &FnCx<'_, X64>, ty: TypeId) -> CgResult<u32> {
    match mask_lanes(cx.types(), ty) {
        Some(n) if (1..=MAX_MASK).contains(&n) => Ok(n),
        Some(_) => unsupported(vector::MASK_WIDE),
        None => cx.bug("a mask operation on a type that is no vector of i1"),
    }
}

/// Whether `value` is of a vector-of-`i1` type (any length).
fn is_i1_vector(cx: &FnCx<'_, X64>, value: Value) -> bool {
    mask_lanes(cx.types(), value_ty(cx, value)).is_some()
}

// ---- the dispatch hook ----

/// The first step of `Target::lower`: an instruction that makes,
/// combines or consumes the lanes of a mask, or a reduction of a mask,
/// is lowered here; `None` for every other instruction, which the groups
/// lower.
pub fn lower<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> Option<CgResult<()>> {
    let result_mask = mask_lanes(cx.types(), inst.ty()).is_some();
    Some(match inst.kind() {
        InstKind::Binary { lhs, rhs } if result_mask => binary(cx, inst, lhs, rhs),
        InstKind::ICmp { pred, lhs, rhs } if result_mask => icmp(cx, inst, pred, lhs, rhs),
        InstKind::FCmp { pred, lhs, rhs } if result_mask => fcmp(cx, inst, pred, lhs, rhs),
        InstKind::Select { cond, if_true, if_false } if is_i1_vector(cx, cond) => {
            select(cx, inst, cond, if_true, if_false)
        }
        InstKind::Cast { arg }
            if inst.opcode() != Opcode::BitCast && (result_mask || is_i1_vector(cx, arg)) =>
        {
            cast(cx, inst, arg)
        }
        InstKind::ExtractElement { vector, index } if is_i1_vector(cx, vector) => {
            extract_element(cx, inst, vector, index)
        }
        InstKind::InsertElement { vector, value, index } if result_mask => {
            insert_element(cx, inst, vector, value, index)
        }
        InstKind::ShuffleVector { first, second, mask } if result_mask => {
            shuffle_vector(cx, inst, first, second, mask)
        }
        InstKind::Store { value, ptr, mem }
            if mem.ordering().is_none()
                && cx.class_of(value) == Class::Gpr
                && is_i1_vector(cx, value) =>
        {
            store(cx, inst, value, ptr)
        }
        InstKind::Call(site) => {
            let which = site.intrinsic()?;
            let fold = reduction_kind(which)?;
            let vec = *site.args().last()?;
            if !is_i1_vector(cx, vec) {
                return None;
            }
            reduce(cx, inst, fold, site.args())
        }
        _ => return None,
    })
}

// ---- constants and operands ----

/// The bits of the mask constant `id` of `n` lanes, if it has them: an
/// undefined lane is 0.
pub fn const_bits(cx: &FnCx<'_, X64>, id: ConstId, n: u32) -> Option<u64> {
    let bit = |id: ConstId| match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(value & 1),
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    };
    match cx.module.consts.get(id) {
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        Const::Splat { elem, .. } => Some(if bit(elem)? == 1 { low_bits(n) } else { 0 }),
        Const::Aggregate { elems, .. } => {
            let mut bits = 0;
            for (i, &e) in elems.iter().enumerate().take(n as usize) {
                bits |= bit(e)? << i;
            }
            Some(bits)
        }
        _ => None,
    }
}

/// The mask operand `value` of `n` lanes in a register the lowering may
/// write: a constant's bits in a new temporary, else the value's
/// register ([`FnCx::use_reg`]).
///
/// # Errors
/// `common::CONSTANT` for a constant without bits; what asking for the
/// operand reports.
fn operand(cx: &mut FnCx<'_, X64>, value: Value, n: u32) -> CgResult<PReg> {
    if let ValueKind::Const(id) = value.kind() {
        let Some(bits) = const_bits(cx, id, n) else {
            return unsupported(crate::common::CONSTANT);
        };
        let t = cx.temp(Class::Gpr)?;
        mov_imm(cx, gp(t), bits, 64);
        return Ok(t);
    }
    cx.use_reg(value)
}

/// The mask operand `value` copied into a new temporary (the lowering
/// writes it; the value may be an operand twice).
fn operand_copy(cx: &mut FnCx<'_, X64>, value: Value, n: u32) -> CgResult<PReg> {
    if matches!(value.kind(), ValueKind::Const(_)) {
        return operand(cx, value, n);
    }
    let t = cx.temp(Class::Gpr)?;
    cx.put(value, t)?;
    Ok(t)
}

/// `and rd, imm` for any 64-bit `imm`, through `rdx` when it is not a
/// sign-extended 32-bit immediate.
fn and_imm(cx: &mut FnCx<'_, X64>, rd: Reg, imm: u64) {
    if let Ok(small) = i32::try_from(imm as i64) {
        put(cx, mc::and_ri(Size::Q, rd, i64::from(small)));
    } else {
        mov_imm(cx, RDX, imm, 64);
        put(cx, mc::and_rr(Size::Q, rd, RDX));
    }
}

/// `or`s bit 0 of `bit` (0 or 1, the rest clear) into bit `lane` of
/// `rd`; writes `bit`.
fn or_bit(cx: &mut FnCx<'_, X64>, rd: Reg, bit: Reg, lane: u32) {
    if lane != 0 {
        put(cx, mc::shl_ri(Size::Q, bit, lane as u8));
    }
    put(cx, mc::or_rr(Size::Q, rd, bit));
}

// ---- element-wise ----

/// `%r = <op> <N x i1> %lhs, %rhs`: the bit rules of the module docs.
///
/// # Errors
/// [`vector::MASK_WIDE`]; what asking for the operands reports.
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    use Opcode as O;
    let n = mask_of(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    let op = inst.opcode();
    let a = operand(cx, lhs, n)?;
    let ra = gp(a);
    match op {
        // The lane is the left operand: a shift by 1 is poison, a
        // division by 0 undefined, `-1 sdiv -1` overflows.
        O::Shl | O::LShr | O::AShr | O::UDiv | O::SDiv => {}
        O::URem | O::SRem => mov_imm(cx, ra, 0, 32),
        _ => {
            let rb = if rhs == lhs { ra } else { gp(operand(cx, rhs, n)?) };
            match op {
                O::Add | O::Sub | O::Xor => put(cx, mc::xor_rr(Size::Q, ra, rb)),
                O::Mul | O::And => put(cx, mc::and_rr(Size::Q, ra, rb)),
                O::Or => put(cx, mc::or_rr(Size::Q, ra, rb)),
                _ => return cx.bug("a binary opcode on masks that is none"),
            }
        }
    }
    cx.finish(inst, a)
}

// ---- compares ----

/// An operand of a lane loop: the image of a vector of lanes.
#[derive(Clone, Copy)]
struct Image {
    /// Where the image is.
    at: FrameRef,
    /// Its lanes.
    lanes: Lanes,
}

/// The images of the vector operands `values`, checked to have `n`
/// lanes; an operand repeated shares its image (`hod_cg::lanes::images`,
/// which gives an `Fpr` operand's register back at once).
fn images<const K: usize>(
    cx: &mut FnCx<'_, X64>,
    values: [Value; K],
    n: u32,
) -> CgResult<[Image; K]> {
    let mut shapes: [Option<Lanes>; K] = [None; K];
    for (slot, &v) in shapes.iter_mut().zip(&values) {
        let l = vector::lanes(cx, value_ty(cx, v))?;
        if l.count() != n {
            return cx.bug("a mask operation on vectors of different lengths");
        }
        *slot = Some(l);
    }
    let mut out = [lanes::Arg::Scalar(Value::NONE); K];
    lanes::images(cx, &values, &shapes, &mut out)?;
    let mut images = [None; K];
    for (image, arg) in images.iter_mut().zip(out) {
        let lanes::Arg::Vector(at, lanes) = arg else {
            return cx.bug("a lane operand without lanes");
        };
        *image = Some(Image { at, lanes });
    }
    Ok(images.map(|i| i.expect("every operand has an image")))
}

/// Loads lane `lane` of `image` into `rd`, zero-extended.
fn load_gpr_lane(cx: &mut FnCx<'_, X64>, image: &Image, lane: u32, rd: Reg) {
    let bytes = image.lanes.bytes();
    frame_inst(cx, lane_place(image, lane), |m| gpr_load(rd, m, bytes));
}

/// The frame place of lane `lane` of `image`.
fn lane_place(image: &Image, lane: u32) -> FrameRef {
    image.at.offset_by(image.lanes.offset(lane))
}

/// Stores the low bytes of `rs` as lane `lane` of `image`.
fn store_gpr_lane(cx: &mut FnCx<'_, X64>, image: &Image, lane: u32, rs: Reg) {
    let size = image.lanes.size();
    frame_inst(cx, lane_place(image, lane), |m| mc::mov_mr(size, m, rs));
}

/// `%r = icmp <pred> <N x T> %lhs, %rhs`: of masks, the bit rule of the
/// module docs; of lanes, one `cmp` + `setcc` per lane on the lanes
/// extended to 64 bits by the predicate's signedness.
///
/// # Errors
/// [`vector::MASK_WIDE`]; what [`vector::lanes`] refuses for the
/// operands; what asking for them reports.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    use IntPredicate as P;
    let n = mask_of(cx, inst.ty())?;
    if is_i1_vector(cx, lhs) {
        deferred_first(cx, inst, None)?;
        let a = operand_copy(cx, lhs, n)?;
        let b = operand(cx, rhs, n)?;
        let (ra, rb) = (gp(a), gp(b));
        // `r11` = the operand complemented where the rule needs it.
        match pred {
            P::Eq => {
                put(cx, mc::xor_rr(Size::Q, ra, rb));
                put(cx, mc::not_r(Size::Q, ra));
            }
            P::Ne => put(cx, mc::xor_rr(Size::Q, ra, rb)),
            P::Ugt | P::Slt | P::Uge | P::Sle => {
                put(cx, mc::mov_rr(Size::Q, R11, rb));
                put(cx, mc::not_r(Size::Q, R11));
                let op = if matches!(pred, P::Ugt | P::Slt) { mc::and_rr } else { mc::or_rr };
                put(cx, op(Size::Q, ra, R11));
            }
            P::Ult | P::Sgt | P::Ule | P::Sge => {
                put(cx, mc::not_r(Size::Q, ra));
                let op = if matches!(pred, P::Ult | P::Sgt) { mc::and_rr } else { mc::or_rr };
                put(cx, op(Size::Q, ra, rb));
            }
        }
        return cx.finish(inst, a);
    }
    vector::lanes(cx, value_ty(cx, lhs))?;
    deferred_first(cx, inst, None)?;
    let [a, b] = images(cx, [lhs, rhs], n)?;
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    let signed = matches!(pred, P::Sgt | P::Sge | P::Slt | P::Sle);
    let bits = a.lanes.shape.elem.bits();
    let cond = crate::int::cond_of(pred);
    mov_imm(cx, r, 0, 32);
    for lane in 0..n {
        load_gpr_lane(cx, &a, lane, RAX);
        load_gpr_lane(cx, &b, lane, RCX);
        if signed && bits < 64 {
            crate::int::extend_to(cx, RAX, bits, crate::int::Ext::Sign, Size::Q);
            crate::int::extend_to(cx, RCX, bits, crate::int::Ext::Sign, Size::Q);
        }
        put(cx, mc::cmp_rr(Size::Q, RAX, RCX));
        put(cx, mc::setcc_r(cond, RAX));
        put(cx, mc::movzx_rr(Size::D, RAX, Size::B, RAX));
        or_bit(cx, r, RAX, lane);
    }
    cx.finish(inst, rd)
}

/// LLVM's code of `pred`: bit 0 equal, 1 greater, 2 less, 3 unordered.
fn fcmp_code(pred: FloatPredicate) -> u8 {
    use FloatPredicate as F;
    match pred {
        F::False => 0,
        F::Oeq => 1,
        F::Ogt => 2,
        F::Oge => 3,
        F::Olt => 4,
        F::Ole => 5,
        F::One => 6,
        F::Ord => 7,
        F::Uno => 8,
        F::Ueq => 9,
        F::Ugt => 10,
        F::Uge => 11,
        F::Ult => 12,
        F::Ule => 13,
        F::Une => 14,
        F::True => 15,
    }
}

/// `%r = fcmp <pred> <N x T> %lhs, %rhs` of floating-point lanes: per
/// lane its class (module docs) in `rcx`, `test` against the predicate's
/// code, the bit or-ed into the result. `true` and `false` are all ones
/// and zero.
///
/// # Errors
/// [`vector::MASK_WIDE`]; what [`vector::lanes`] refuses; what asking
/// for the operands reports.
pub fn fcmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let n = mask_of(cx, inst.ty())?;
    let lanes = vector::lanes(cx, value_ty(cx, lhs))?;
    if !lanes.shape.elem.is_float() {
        return cx.bug("an fcmp of lanes that are no floating-point type");
    }
    deferred_first(cx, inst, None)?;
    let code = fcmp_code(pred);
    if code == 0 || code == 15 {
        let rd = cx.def_reg(inst)?;
        mov_imm(cx, gp(rd), if code == 15 { low_bits(n) } else { 0 }, 64);
        return cx.finish(inst, rd);
    }
    let [a, b] = images(cx, [lhs, rhs], n)?;
    let rd = cx.def_reg(inst)?;
    let bits = gp(rd);
    mov_imm(cx, bits, 0, 32);
    for lane in 0..n {
        match lanes.shape.elem {
            Elem::Float | Elem::Double => float_class(cx, &a, &b, lane),
            Elem::Half => small_class(cx, &a, &b, lane, 0x7c00),
            _ => small_class(cx, &a, &b, lane, 0x7f80),
        }
        put(cx, mc::test_ri(Size::D, RCX, i64::from(code)));
        put(cx, mc::setcc_r(Cond::Ne, RCX));
        put(cx, mc::movzx_rr(Size::D, RCX, Size::B, RCX));
        or_bit(cx, bits, RCX, lane);
    }
    cx.finish(inst, rd)
}

/// The class of lane `lane` of `float`/`double` images `a`, `b` in `rcx`
/// (`ucomis`; unordered sets ZF, PF and CF).
fn float_class(cx: &mut FnCx<'_, X64>, a: &Image, b: &Image, lane: u32) {
    let bytes = a.lanes.bytes();
    let size = if bytes == 8 { FpSize::D } else { FpSize::S };
    frame_inst(cx, lane_place(a, lane), |m| fpr_load(XMM14, m, bytes));
    frame_inst(cx, lane_place(b, lane), |m| fpr_load(XMM15, m, bytes));
    put(cx, mc::ucomis_rr(size, XMM14, XMM15));
    put(cx, mc::setcc_r(Cond::P, RAX));
    put(cx, mc::setcc_r(Cond::B, RCX));
    put(cx, mc::setcc_r(Cond::E, RDX));
    put(cx, mc::setcc_r(Cond::A, R11));
    ordered_code(cx, RAX, R11);
}

/// `rcx` = less·4 | greater·2 | equal from the low bytes of `rcx`,
/// `greater`, `rdx` (each 0 or 1), replaced by 8 when the low byte of
/// `unordered` is set.
fn ordered_code(cx: &mut FnCx<'_, X64>, unordered: Reg, greater: Reg) {
    for reg in [unordered, RCX, RDX, greater] {
        put(cx, mc::movzx_rr(Size::D, reg, Size::B, reg));
    }
    put(cx, mc::shl_ri(Size::D, RCX, 2));
    put(cx, mc::shl_ri(Size::D, greater, 1));
    put(cx, mc::or_rr(Size::D, RCX, greater));
    put(cx, mc::or_rr(Size::D, RCX, RDX));
    put(cx, mc::test_rr(Size::D, unordered, unordered));
    put(cx, mc::mov_ri(Size::D, unordered, 8));
    put(cx, mc::cmovcc_rr(Cond::Ne, Size::D, RCX, unordered));
}

/// The class of lane `lane` of `half`/`bfloat` images `a`, `b` in `rcx`,
/// on the bits (`inf` the infinity's magnitude): NaN when a magnitude is
/// above it; else the signed compare of the sign-magnitude keys.
fn small_class(cx: &mut FnCx<'_, X64>, a: &Image, b: &Image, lane: u32, inf: i64) {
    load_gpr_lane(cx, a, lane, RAX);
    load_gpr_lane(cx, b, lane, RCX);
    // `r11` = either is NaN.
    for (k, x) in [(0, RAX), (1, RCX)] {
        put(cx, mc::mov_rr(Size::D, RDX, x));
        put(cx, mc::and_ri(Size::D, RDX, 0x7fff));
        put(cx, mc::cmp_ri(Size::D, RDX, inf));
        put(cx, mc::setcc_r(Cond::A, RDX));
        put(cx, mc::movzx_rr(Size::D, RDX, Size::B, RDX));
        if k == 0 {
            put(cx, mc::mov_rr(Size::D, R11, RDX));
        } else {
            put(cx, mc::or_rr(Size::D, R11, RDX));
        }
    }
    // The keys: `rdx` of `a`, `rax` of `b`.
    key(cx, RDX, RAX);
    key(cx, RAX, RCX);
    put(cx, mc::cmp_rr(Size::D, RDX, RAX));
    put(cx, mc::setcc_r(Cond::L, RCX));
    put(cx, mc::setcc_r(Cond::E, RDX));
    put(cx, mc::setcc_r(Cond::G, RAX));
    ordered_code(cx, R11, RAX);
}

/// `dst` = the signed key of the 16-bit sign-magnitude number in `src`
/// (written): the magnitude, negated when the sign is set.
fn key(cx: &mut FnCx<'_, X64>, dst: Reg, src: Reg) {
    put(cx, mc::mov_rr(Size::D, dst, src));
    put(cx, mc::and_ri(Size::D, dst, 0x7fff));
    put(cx, mc::shr_ri(Size::D, src, 15));
    put(cx, mc::neg_r(Size::D, src));
    put(cx, mc::xor_rr(Size::D, dst, src));
    put(cx, mc::sub_rr(Size::D, dst, src));
}

// ---- select ----

/// `%r = select <N x i1> %cond, %if_true, %if_false`: of masks,
/// `(c & t) | (f & ~c)`; of lanes, per lane both lanes into `rax`/`rcx`,
/// `bt` of the condition's bit and `cmovae`, the chosen lane stored into
/// the result's image (an `Fpr` result's is a scratch image, loaded at
/// the end).
///
/// # Errors
/// [`vector::MASK_WIDE`]; what [`vector::lanes`] refuses for the values;
/// what asking for the operands reports.
pub fn select<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let n = mask_of(cx, value_ty(cx, cond))?;
    if is_i1_vector(cx, if_true) {
        deferred_first(cx, inst, None)?;
        // The three may be one value twice: each arm is copied.
        let t = operand_copy(cx, if_true, n)?;
        let f = operand_copy(cx, if_false, n)?;
        let c = gp(operand(cx, cond, n)?);
        let (rt, rf) = (gp(t), gp(f));
        put(cx, mc::and_rr(Size::Q, rt, c));
        put(cx, mc::mov_rr(Size::Q, R11, c));
        put(cx, mc::not_r(Size::Q, R11));
        put(cx, mc::and_rr(Size::Q, rf, R11));
        put(cx, mc::or_rr(Size::Q, rt, rf));
        return cx.finish(inst, t);
    }
    let result = vector::lanes(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    let [t, f] = images(cx, [if_true, if_false], n)?;
    let c = gp(operand(cx, cond, n)?);
    let out = result_image(cx, inst, result)?;
    for lane in 0..n {
        load_gpr_lane(cx, &t, lane, RAX);
        load_gpr_lane(cx, &f, lane, RCX);
        put(cx, mc::bt_ri(Size::Q, c, lane as u8));
        put(cx, mc::cmovcc_rr(Cond::Ae, Size::Q, RAX, RCX));
        store_gpr_lane(cx, &out, lane, RAX);
    }
    lanes::finish(cx, inst, &out.lanes, out.at)
}

/// Where the image of a vector result of lanes `result` is written: its
/// slot (`Mem`) or a scratch image (`Fpr`).
fn result_image<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, result: Lanes) -> CgResult<Image> {
    let at = if result.shape.class == Class::Fpr {
        vector::scratch(cx, vector::SCRATCH_IMAGES - 1)?
    } else {
        cx.def_mem(inst)?
    };
    Ok(Image { at, lanes: result })
}

// ---- casts ----

/// The bits of `1.0` in `elem`, or of `-1.0` when `negative`.
fn one(elem: Elem, negative: bool) -> u64 {
    let (bits, sign) = match elem {
        Elem::Half => (0x3c00, 15),
        Elem::BFloat => (0x3f80, 15),
        Elem::Float => (0x3f80_0000, 31),
        _ => (0x3ff0_0000_0000_0000, 63),
    };
    if negative { bits | 1 << sign } else { bits }
}

/// `%r = <cast> %arg` with a mask on one side (not `bitcast`): `zext` /
/// `sext` of a mask to lanes (0 or 1 / all ones), `uitofp` / `sitofp` of
/// a mask (0 or `1.0` / `-1.0`, no conversion); `trunc` of lanes to a
/// mask (bit 0 of each lane), `fptoui` / `fptosi` to a mask (whether the
/// magnitude is at least 1: the lanes for which the conversion is
/// defined are those whose truncation is 0 or ±1).
///
/// # Errors
/// [`vector::MASK_WIDE`]; what [`vector::lanes`] refuses for the lanes'
/// side; `int::CAST` for another opcode.
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    use Opcode as O;
    let op = inst.opcode();
    if is_i1_vector(cx, arg) {
        let n = mask_of(cx, value_ty(cx, arg))?;
        if !matches!(op, O::ZExt | O::SExt | O::UIToFP | O::SIToFP) {
            return unsupported(crate::int::CAST);
        }
        let to = vector::lanes(cx, inst.ty())?;
        deferred_first(cx, inst, None)?;
        let c = gp(operand(cx, arg, n)?);
        let out = result_image(cx, inst, to)?;
        let signed = matches!(op, O::SExt | O::SIToFP);
        let fp = matches!(op, O::UIToFP | O::SIToFP);
        if fp {
            mov_imm(cx, R11, one(to.shape.elem, signed), 64);
        }
        for lane in 0..n {
            put(cx, mc::mov_rr(Size::Q, RAX, c));
            if lane != 0 {
                put(cx, mc::shr_ri(Size::Q, RAX, lane as u8));
            }
            put(cx, mc::and_ri(Size::D, RAX, 1));
            if signed || fp {
                put(cx, mc::neg_r(Size::Q, RAX));
            }
            if fp {
                put(cx, mc::and_rr(Size::Q, RAX, R11));
            }
            store_gpr_lane(cx, &out, lane, RAX);
        }
        return lanes::finish(cx, inst, &out.lanes, out.at);
    }
    let n = mask_of(cx, inst.ty())?;
    if !matches!(op, O::Trunc | O::FPToUI | O::FPToSI) {
        return unsupported(crate::int::CAST);
    }
    let from = vector::lanes(cx, value_ty(cx, arg))?;
    deferred_first(cx, inst, None)?;
    let [a] = images(cx, [arg], n)?;
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    mov_imm(cx, r, 0, 32);
    let elem = from.shape.elem;
    for lane in 0..n {
        load_gpr_lane(cx, &a, lane, RAX);
        if op == O::Trunc {
            put(cx, mc::and_ri(Size::D, RAX, 1));
        } else {
            let sign = elem.bits() - 1;
            put(cx, mc::btr_ri(Size::Q, RAX, sign as u8));
            mov_imm(cx, R11, one(elem, false), 64);
            put(cx, mc::cmp_rr(Size::Q, RAX, R11));
            put(cx, mc::setcc_r(Cond::Ae, RAX));
            put(cx, mc::movzx_rr(Size::D, RAX, Size::B, RAX));
        }
        or_bit(cx, r, RAX, lane);
    }
    cx.finish(inst, rd)
}

// ---- element access ----

/// The lane index operand `at` of a mask of `n` lanes.
fn mask_index(cx: &mut FnCx<'_, X64>, at: Value, n: u32) -> CgResult<Index> {
    let ty = value_ty(cx, at);
    let Some(bits @ 1..=64) = cx.types().int_bits(ty) else {
        return unsupported(vector::INDEX);
    };
    if let ValueKind::Const(id) = at.kind() {
        let lane = match cx.module.consts.get(id) {
            Const::Int { value, .. } => value,
            Const::Zero(_) => 0,
            _ => return Ok(Index::Poison),
        };
        return Ok(match u32::try_from(lane) {
            Ok(lane) if lane < n => Index::Lane(lane),
            _ => Index::Poison,
        });
    }
    Ok(Index::Reg { reg: gp(cx.use_reg(at)?), bits })
}

/// `%r = extractelement <N x i1> %vector, %index`: the lane's bit, by a
/// shift (a constant index; lane 0 for one out of range) or a shift by
/// `cl`, the lane number (masked or clamped as `vector::lane_number`
/// does).
///
/// # Errors
/// [`vector::MASK_WIDE`]; [`vector::INDEX`]; what asking for the
/// operands reports.
pub fn extract_element<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    vector: Value,
    at: Value,
) -> CgResult<()> {
    let n = mask_of(cx, value_ty(cx, vector))?;
    deferred_first(cx, inst, None)?;
    let idx = mask_index(cx, at, n)?;
    let c = operand_copy(cx, vector, n)?;
    let rc = gp(c);
    match idx {
        Index::Lane(0) | Index::Poison => {}
        Index::Lane(lane) => put(cx, mc::shr_ri(Size::Q, rc, lane as u8)),
        Index::Reg { reg, bits } => {
            vector::lane_number(cx, reg, bits, n);
            put(cx, mc::mov_rr(Size::D, RCX, RAX));
            put(cx, mc::shr_rcl(Size::Q, rc));
        }
    }
    put(cx, mc::and_ri(Size::D, rc, 1));
    cx.finish(inst, c)
}

/// `%r = insertelement <N x i1> %vector, i1 %value, %index`: the lane's
/// bit cleared and set from bit 0 of the value (a constant index;
/// nothing for one out of range), or the same at the lane number in `cl`
/// (a variable index).
///
/// # Errors
/// [`vector::MASK_WIDE`]; [`vector::INDEX`]; what asking for the
/// operands reports.
pub fn insert_element<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    vector: Value,
    value: Value,
    at: Value,
) -> CgResult<()> {
    let n = mask_of(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    let c = operand_copy(cx, vector, n)?;
    let rc = gp(c);
    let e = cx.temp(Class::Gpr)?;
    cx.put(value, e)?;
    let re = gp(e);
    put(cx, mc::and_ri(Size::D, re, 1));
    // An index that is the value itself (`i1` both) is that bit.
    let idx = if at == value { Index::Reg { reg: re, bits: 1 } } else { mask_index(cx, at, n)? };
    match idx {
        Index::Lane(lane) => {
            put(cx, mc::btr_ri(Size::Q, rc, lane as u8));
            or_bit(cx, rc, re, lane);
        }
        Index::Poison => {}
        Index::Reg { reg, bits } => {
            vector::lane_number(cx, reg, bits, n);
            put(cx, mc::mov_rr(Size::D, RCX, RAX));
            put(cx, mc::btr_rr(Size::Q, rc, RCX));
            put(cx, mc::shl_rcl(Size::Q, re));
            put(cx, mc::or_rr(Size::Q, rc, re));
        }
    }
    cx.free_temp(e);
    cx.finish(inst, c)
}

/// `%r = shufflevector <N x i1> %first, %second, <mask>`: the result's
/// lanes in runs of consecutive lanes of one source, each run shifted
/// down, masked and shifted to its place in `r11`, or-ed into the
/// result; undefined lanes 0. A source no entry names is not asked for.
///
/// # Errors
/// [`vector::MASK_WIDE`] for either type; what asking for the operands
/// reports.
pub fn shuffle_vector<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    first: Value,
    second: Value,
    mask: &[u32],
) -> CgResult<()> {
    mask_of(cx, inst.ty())?;
    let n = mask_of(cx, value_ty(cx, first))?;
    deferred_first(cx, inst, None)?;
    let mut used = [false; 2];
    for &entry in mask {
        if let Some((which, _)) = vector::source(entry, n) {
            used[which] = true;
        }
    }
    let mut regs = [R11; 2];
    if used[0] {
        regs[0] = gp(operand(cx, first, n)?);
    }
    if used[1] {
        regs[1] = if second == first && used[0] { regs[0] } else { gp(operand(cx, second, n)?) };
    }
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    mov_imm(cx, r, 0, 32);
    let mut k = 0usize;
    while k < mask.len() {
        let Some((which, lane)) = vector::source(mask[k], n) else {
            k += 1;
            continue;
        };
        let mut len = 1usize;
        while k + len < mask.len()
            && vector::source(mask[k + len], n) == Some((which, lane + len as u32))
        {
            len += 1;
        }
        put(cx, mc::mov_rr(Size::Q, R11, regs[which]));
        if lane != 0 {
            put(cx, mc::shr_ri(Size::Q, R11, lane as u8));
        }
        and_imm(cx, R11, low_bits(len as u32));
        or_bit(cx, r, R11, k as u32);
        k += len;
    }
    cx.finish(inst, rd)
}

// ---- stores ----

/// `store <N x i1> %value, ptr %ptr`: the bits from `N` up to the byte
/// boundary cleared (in `r11`), then the `⌈N/8⌉` bytes stored at the
/// pointer (an odd size in pieces).
///
/// # Errors
/// What asking for the operands reports.
pub fn store<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, value: Value, ptr: Value) -> CgResult<()> {
    let n = mask_of(cx, value_ty(cx, value))?;
    deferred_first(cx, inst, None)?;
    let v = operand(cx, value, n)?;
    put(cx, mc::mov_rr(Size::Q, R11, gp(v)));
    if n % 8 != 0 {
        and_imm(cx, R11, low_bits(n));
    }
    let p = gp(cx.use_reg(ptr)?);
    let mut done = 0;
    for (offset, size) in crate::common::pieces(n.div_ceil(8)) {
        let shift = 8 * (offset - done);
        if shift != 0 {
            put(cx, mc::shr_ri(Size::Q, R11, shift as u8));
            done = offset;
        }
        let size = crate::common::exact_size(size).unwrap_or(Size::B);
        put(cx, mc::mov_mr(size, Mem::base(p, offset as i32), R11));
    }
    Ok(())
}

// ---- reductions ----

/// The reductions, as the bit operation of the fold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fold {
    /// Any lane set.
    Any,
    /// Every lane set.
    All,
    /// The parity.
    Parity,
    /// A floating-point fold (refused on masks).
    Float,
}

/// The fold of a `llvm.vector.reduce.*` intrinsic on one bit.
fn reduction_kind(which: Intrinsic<'_>) -> Option<Fold> {
    use Intrinsic as I;
    Some(match which {
        I::VectorReduceOr | I::VectorReduceUmax | I::VectorReduceSmin => Fold::Any,
        I::VectorReduceAnd | I::VectorReduceUmin | I::VectorReduceSmax | I::VectorReduceMul => {
            Fold::All
        }
        I::VectorReduceXor | I::VectorReduceAdd => Fold::Parity,
        I::VectorReduceFadd | I::VectorReduceFmul | I::VectorReduceFmax | I::VectorReduceFmin => {
            Fold::Float
        }
        _ => return None,
    })
}

/// A reduction of a mask: a flag from its low `N` bits.
///
/// # Errors
/// [`vector::MASK_WIDE`]; [`REDUCE`] for an operand list that is not the
/// intrinsic's or a floating-point fold; what asking for the operand
/// reports.
fn reduce<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, fold: Fold, args: &[Value]) -> CgResult<()> {
    let (Fold::Any | Fold::All | Fold::Parity, &[vec]) = (fold, args) else {
        return unsupported(REDUCE);
    };
    let n = mask_of(cx, value_ty(cx, vec))?;
    deferred_first(cx, inst, None)?;
    let c = operand_copy(cx, vec, n)?;
    let rc = gp(c);
    if fold == Fold::All {
        put(cx, mc::not_r(Size::Q, rc));
    }
    if n < 64 {
        and_imm(cx, rc, low_bits(n));
    }
    if fold == Fold::Parity {
        let mut shift = 32;
        while shift >= 1 {
            if shift < n.next_power_of_two() {
                put(cx, mc::mov_rr(Size::Q, R11, rc));
                put(cx, mc::shr_ri(Size::Q, R11, shift as u8));
                put(cx, mc::xor_rr(Size::Q, rc, R11));
            }
            shift /= 2;
        }
        put(cx, mc::and_ri(Size::D, rc, 1));
        return cx.finish(inst, c);
    }
    put(cx, mc::test_rr(Size::Q, rc, rc));
    put(cx, mc::setcc_r(if fold == Fold::Any { Cond::Ne } else { Cond::E }, rc));
    put(cx, mc::movzx_rr(Size::D, rc, Size::B, rc));
    cx.finish(inst, c)
}
