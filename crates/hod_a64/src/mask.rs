//! Vectors of `i1`, masks (`spec/codegen.md` §5.5.1 "Vectors, masks
//! (WP-081)"; design note `docs/design/0003-vectors.md` package V2):
//! `<N x i1>` with `N` up to 64 as a bit-packed general-register value,
//! the instructions that make, combine and consume one, and the
//! `llvm.vector.reduce.*` family on masks and on ordinary lanes.
//!
//! **Owner: WP-081.** Tests: `tests/mask.rs`. Hooks: the first line of
//! `Target::lower` ([`lower`]), the constant arm of
//! `Target::materialize` ([`materialize`]) and the class of a vector type
//! (`target::vector_class`).
//!
//! # The representation
//!
//! A mask of `N ≤ 64` lanes is of class `Gpr`: lane `i` is bit `i` of the
//! register, and the bits from `N` up are **unspecified**, as they are for
//! an `iN` (§2.2). This is LLVM's in-memory image of the vector (element 0
//! in the lowest bit, `⌈N/8⌉` bytes), so:
//!
//! - `bitcast` between a mask and an `iN` (or another type of `N` bits) is
//!   the copy the integer, floating-point and `Mem` groups already make;
//! - `load` is the integer load of `⌈N/8⌉` bytes (`mem.rs`), which leaves
//!   the padding bits of the last byte in the register: a load may assume
//!   nothing about them, and nothing here reads them;
//! - `store` clears the bits from `N` up to the byte boundary first
//!   ([`store`]), as `llc` does (`and w8, w0, #0x7` before the `strb` of a
//!   `<3 x i1>`); a mask of a whole number of bytes is stored as it is;
//! - `phi`, `freeze`, `select` on a scalar condition, `extractvalue` and
//!   `insertvalue` are the integer group's and `aggr`'s, unchanged.
//!
//! Everything that looks at lanes comes here first ([`lower`]), before the
//! scalar paths, which would read a mask as one integer: element-wise
//! arithmetic, `icmp` and `fcmp` of vectors (whose results are masks),
//! `select` on a vector condition, the casts between masks and lanes,
//! `extractelement`, `insertelement` and `shufflevector` of masks, stores
//! of masks and the reductions.
//!
//! A mask of more than 64 lanes stays of class `Mem` (its image in a
//! slot): loads, stores, copies and bitcasts of it are `aggr`'s, and every
//! operation on its lanes is refused as [`MASK_WIDE`]. An intrinsic of `i1`
//! lanes stays the intrinsic group's `a64:intrinsic-vector`; a mask
//! crossing a call or a return is `vcall`'s (WP-082).
//!
//! Poison: an undefined lane of a constant or of a shuffle is 0; a lane of
//! an operation that LLVM leaves poison (a shift of an `i1` by 1, a
//! division by 0) takes whatever the formula below gives.
//!
//! # Lane rules on one bit
//!
//! | operation | bits |
//! |---|---|
//! | `add`, `sub`, `xor` | `eor` |
//! | `mul`, `and` | `and` |
//! | `or` | `orr` |
//! | `shl`, `lshr`, `ashr`, `udiv`, `sdiv` | the left operand (a shift by 1 is poison, a division by 0 undefined; `-1 / -1` overflows) |
//! | `urem`, `srem` | 0 |
//! | `icmp eq` / `ne` | `eon` / `eor` |
//! | `icmp ugt`, `slt` | `a & ~b` (`bic`); `ult`, `sgt`: `b & ~a` |
//! | `icmp uge`, `sle` | `a \| ~b` (`orn`); `ule`, `sge`: `b \| ~a` |
//! | `select` | `(c & t) \| (f & ~c)` |
//! | `reduce.or`, `umax`, `smin` | any bit of the `N` set |
//! | `reduce.and`, `umin`, `smax`, `mul` | all `N` bits set |
//! | `reduce.xor`, `add` | the parity of the `N` bits |
//!
//! # Registers
//!
//! `x16`, `x17`, `v30`, `v31`, and what the scalar kernels a loop calls
//! write (each says); the operands' and the result's registers, and at
//! most two pool temporaries.

use hod_cg::{CgResult, Class, FnCx, FrameRef, lanes};
use hod_ir::{
    Const, ConstId, FloatPredicate, Inst, InstKind, IntPredicate, Intrinsic, Opcode, TypeId,
    TypeKind, TypeTable, Value, ValueKind,
};
use hod_mc_a64 as mc;
use hod_mc_a64::{Cond, FpFormat, FpSize, MemSize, Reg, RegZr, Shift, Size, VMemSize};

use crate::common::{
    Cursor, V30, V31, X16, X17, deferred_first, load_fpr_at, load_gpr_at, materialize_gpr_const,
    mov_imm, put, store_gpr_at, store_gpr_bytes, unsupported, value_ty, vr, xr, zero_bytes,
};
use crate::target::A64;
use crate::vector::{self, Elem, Index, Lanes, MASK_WIDE, SCRATCH_IMAGES};
use crate::{float, int, intrinsic};

/// The most lanes of a mask in a register.
pub const MAX_MASK: u32 = 64;

/// A reduction (`llvm.vector.reduce.*`) of a kind this module does not
/// lower: `fmax`/`fmin` of a vector of `half` or `bfloat`, or of an
/// operand of a shape the reductions do not take.
pub const REDUCE: &str = "a64:vector-reduce";

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

/// The lanes of the mask type `ty`, or [`MASK_WIDE`] past [`MAX_MASK`].
fn mask_of(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<u32> {
    match mask_lanes(cx.types(), ty) {
        Some(n) if (1..=MAX_MASK).contains(&n) => Ok(n),
        Some(_) => unsupported(MASK_WIDE),
        None => cx.bug("a mask operation on a type that is no vector of i1"),
    }
}

/// Whether `value` is of a mask type.
fn is_mask(cx: &FnCx<'_, A64>, value: Value) -> bool {
    mask_lanes(cx.types(), value_ty(cx, value)).is_some()
}

// ---- the dispatch hook ----

/// The hook at the top of `Target::lower`: an instruction that makes,
/// combines or consumes the lanes of a mask, or a reduction, is lowered
/// here; `None` for every other instruction, which the groups lower.
pub fn lower<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> Option<CgResult<()>> {
    let result_mask = mask_lanes(cx.types(), inst.ty()).is_some();
    Some(match inst.kind() {
        InstKind::Binary { lhs, rhs } if result_mask => binary(cx, inst, lhs, rhs),
        InstKind::ICmp { pred, lhs, rhs } if result_mask => icmp(cx, inst, pred, lhs, rhs),
        InstKind::FCmp { pred, lhs, rhs } if result_mask => fcmp(cx, inst, pred, lhs, rhs),
        InstKind::Select { cond, if_true, if_false } if is_mask(cx, cond) => {
            select(cx, inst, cond, if_true, if_false)
        }
        InstKind::Cast { arg }
            if inst.opcode() != Opcode::BitCast && (result_mask || is_mask(cx, arg)) =>
        {
            cast(cx, inst, arg)
        }
        InstKind::ExtractElement { vector, index } if is_mask(cx, vector) => {
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
                && is_mask(cx, value) =>
        {
            store(cx, inst, value, ptr)
        }
        InstKind::Call(site) => {
            let which = site.intrinsic()?;
            reduction_kind(which)?;
            reduce(cx, inst, which, site.args())
        }
        _ => return None,
    })
}

// ---- constants ----

/// The bits of the mask constant `id` of `n` lanes, if it has them: an
/// undefined lane is 0.
fn const_bits(cx: &FnCx<'_, A64>, id: ConstId, n: u32) -> Option<u64> {
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

/// The hook in `Target::materialize` for a constant in a general
/// register: a mask constant by its bits (undefined lanes 0, the bits
/// above `N` clear), anything else by `common::materialize_gpr_const`.
///
/// # Errors
/// `common::CONSTANT` for a mask constant without bits (an expression);
/// what `materialize_gpr_const` reports.
pub fn materialize(cx: &mut FnCx<'_, A64>, rd: Reg, id: ConstId) -> CgResult<()> {
    let ty = cx.module.consts.ty(id);
    let Some(n @ 1..=MAX_MASK) = mask_lanes(cx.types(), ty) else {
        return materialize_gpr_const(cx, rd, id);
    };
    match const_bits(cx, id, n) {
        Some(bits) => {
            mov_imm(cx, Size::X, rd, bits);
            Ok(())
        }
        None => unsupported(crate::common::CONSTANT),
    }
}

/// The mask operand `value` (of `n` lanes) as an immediate, if it is a
/// constant with bits.
fn const_operand(cx: &FnCx<'_, A64>, value: Value, n: u32) -> Option<u64> {
    match value.kind() {
        ValueKind::Const(id) => const_bits(cx, id, n),
        _ => None,
    }
}

// ---- element-wise ----

/// `%r = <op> <N x i1> %lhs, %rhs`: the bit rules of the module docs.
///
/// # Errors
/// [`MASK_WIDE`]; what asking for the operands reports.
pub fn binary<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    use Opcode as O;
    mask_of(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    let op = inst.opcode();
    let a = cx.use_reg(lhs)?;
    let ra = xr(a).zr();
    let word = match op {
        // The lane is the left operand: a shift by 1 is poison, a
        // division by 0 undefined, `-1 sdiv -1` overflows.
        O::Shl | O::LShr | O::AShr | O::UDiv | O::SDiv => return cx.finish(inst, a),
        O::URem | O::SRem => mc::mov(Size::X, ra, RegZr::ZR),
        _ => {
            let rb = if rhs == lhs { ra } else { xr(cx.use_reg(rhs)?).zr() };
            match op {
                O::Add | O::Sub | O::Xor => mc::eor(Size::X, ra, ra, rb),
                O::Mul | O::And => mc::and(Size::X, ra, ra, rb),
                O::Or => mc::orr(Size::X, ra, ra, rb),
                _ => return cx.bug("a binary opcode on masks that is none"),
            }
        }
    };
    put(cx, word);
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

/// The images of the vector operands `values` (at most
/// [`SCRATCH_IMAGES`]), checked to have `n` lanes; an operand repeated
/// shares its image. The shapes are checked before anything is emitted.
fn images<const K: usize>(
    cx: &mut FnCx<'_, A64>,
    values: [Value; K],
    n: u32,
) -> CgResult<[Image; K]> {
    let mut shapes: [Option<Lanes>; K] = [None; K];
    for (slot, &v) in shapes.iter_mut().zip(&values) {
        let l = lanes_of(cx, value_ty(cx, v))?;
        if l.count() != n {
            return cx.bug("a mask operation on vectors of different lengths");
        }
        *slot = Some(l);
    }
    if K == 0 {
        return cx.bug("a lane loop without operands");
    }
    // An `Fpr` operand is spilled to the next scratch image, its register
    // given back (the minimal pool has two vector registers):
    // `hod_cg::lanes::images`.
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

/// The lanes of the vector type `ty`: [`vector::lanes`], with `bfloat`
/// refused as [`vector::BFLOAT`] where the lanes are computed on.
fn lanes_of(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<Lanes> {
    vector::lanes(cx, ty)
}

/// A cursor at lane `lane` of `image`, rebasing into `tmp`.
fn lane_at(cx: &mut FnCx<'_, A64>, image: &Image, lane: u32, tmp: Reg) -> Cursor {
    Cursor::frame(cx, vector::offset(image.at, image.lanes.offset(lane)), tmp)
}

/// Loads lane `lane` of `image` into general register `rt`
/// (zero-extended), rebasing into `rt` itself.
fn load_gpr_lane(cx: &mut FnCx<'_, A64>, image: &Image, lane: u32, rt: Reg) {
    let mut at = lane_at(cx, image, lane, rt);
    load_gpr_at(cx, image.lanes.gpr_load(), rt, &mut at);
}

/// Sets lane `lane` of the mask being built in `rd` from bit 0 of `bit`:
/// lane 0 clears every other bit (`and`), the others insert (`bfi`).
fn put_bit(cx: &mut FnCx<'_, A64>, rd: Reg, bit: Reg, lane: u32) {
    if lane == 0 {
        put(cx, mc::and_imm(Size::X, rd.sp(), bit.zr(), 1));
    } else {
        put(cx, mc::bfi(Size::X, rd.zr(), bit.zr(), lane, 1));
    }
}

/// The condition after `cmp a, b` under which `pred` holds of `(a, b)`.
fn int_cond(pred: IntPredicate) -> Cond {
    int::cond_of(pred)
}

/// `%r = icmp <pred> <N x T> %lhs, %rhs`: of masks, the bit rule of the
/// module docs; of lanes (`Fpr` or `Mem`), one `cmp` + `cset` per lane
/// on the lanes extended to 64 bits by the predicate's signedness, each
/// result bit inserted into the mask.
///
/// # Errors
/// [`MASK_WIDE`]; what [`vector::lanes`] refuses for the operands; what
/// asking for them reports.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    use IntPredicate as P;
    let n = mask_of(cx, inst.ty())?;
    if is_mask(cx, lhs) {
        deferred_first(cx, inst, None)?;
        let a = cx.use_reg(lhs)?;
        let ra = xr(a).zr();
        let rb = if rhs == lhs { ra } else { xr(cx.use_reg(rhs)?).zr() };
        let word = match pred {
            P::Eq => mc::eon(Size::X, ra, ra, rb),
            P::Ne => mc::eor(Size::X, ra, ra, rb),
            P::Ugt | P::Slt => mc::bic(Size::X, ra, ra, rb),
            P::Ult | P::Sgt => mc::bic(Size::X, ra, rb, ra),
            P::Uge | P::Sle => mc::orn(Size::X, ra, ra, rb),
            P::Ule | P::Sge => mc::orn(Size::X, ra, rb, ra),
        };
        put(cx, word);
        return cx.finish(inst, a);
    }
    lanes_of(cx, value_ty(cx, lhs))?;
    deferred_first(cx, inst, None)?;
    let [a, b] = images(cx, [lhs, rhs], n)?;
    let rd = cx.def_reg(inst)?;
    let r = xr(rd);
    let signed = matches!(pred, P::Sgt | P::Sge | P::Slt | P::Sle);
    let bits = a.lanes.shape.elem.bits();
    let cond = int_cond(pred);
    for lane in 0..n {
        load_gpr_lane(cx, &a, lane, X16);
        load_gpr_lane(cx, &b, lane, X17);
        if signed && bits < 64 {
            put(cx, mc::sbfx(Size::X, X16.zr(), X16.zr(), 0, bits));
            put(cx, mc::sbfx(Size::X, X17.zr(), X17.zr(), 0, bits));
        }
        put(cx, mc::cmp(Size::X, X16.zr(), X17.zr()));
        set_lane(cx, r, lane, cond, None);
    }
    cx.finish(inst, rd)
}

/// Sets lane `lane` of the mask in `r` to whether `first` (or `second`)
/// holds: `cset` (and `csinc`) into `r` for lane 0, into `x16` and an
/// `orr` at the lane's bit for the others.
fn set_lane(cx: &mut FnCx<'_, A64>, r: Reg, lane: u32, first: Cond, second: Option<Cond>) {
    let t = if lane == 0 { r } else { X16 };
    put(cx, mc::cset(Size::X, t.zr(), first));
    if let Some(second) = second {
        // `t = second ? 1 : t`.
        put(cx, mc::csinc(Size::X, t.zr(), t.zr(), RegZr::ZR, second.invert()));
    }
    if lane != 0 {
        put(cx, mc::orr_shifted(Size::X, r.zr(), r.zr(), X16.zr(), Shift::Lsl, lane));
    }
}

/// `%r = fcmp <pred> <N x T> %lhs, %rhs` of floating-point lanes (`half`,
/// `float`, `double`; `Fpr` or `Mem`): per lane, the lanes into `v30` and
/// `v31` (`half` widened to `float`), `fcmp` and the predicate's
/// condition(s) as the scalar `fcmp` reads them (`float::conditions`).
/// `true` and `false` are all ones and zero.
///
/// # Errors
/// [`MASK_WIDE`]; [`vector::BFLOAT`]; what [`vector::lanes`] refuses;
/// what asking for the operands reports.
#[allow(clippy::many_single_char_names, reason = "registers named as in the instructions")]
pub fn fcmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let n = mask_of(cx, inst.ty())?;
    let l = lanes_of(cx, value_ty(cx, lhs))?;
    let fp = match l.shape.elem {
        Elem::Half | Elem::Float => FpSize::S,
        Elem::Double => FpSize::D,
        Elem::BFloat => return unsupported(vector::BFLOAT),
        _ => return cx.bug("an fcmp of lanes that are no floating-point type"),
    };
    deferred_first(cx, inst, None)?;
    if matches!(pred, FloatPredicate::True | FloatPredicate::False) {
        let rd = cx.def_reg(inst)?;
        let bits = if pred == FloatPredicate::True { low_bits(n) } else { 0 };
        mov_imm(cx, Size::X, xr(rd), bits);
        return cx.finish(inst, rd);
    }
    let [a, b] = images(cx, [lhs, rhs], n)?;
    let rd = cx.def_reg(inst)?;
    let r = xr(rd);
    let cond = float::conditions(pred);
    let view = l.fpr_view();
    for lane in 0..n {
        let mut at = lane_at(cx, &a, lane, X16);
        load_fpr_at(cx, view, V30, &mut at);
        let mut at = lane_at(cx, &b, lane, X16);
        load_fpr_at(cx, view, V31, &mut at);
        if l.shape.elem == Elem::Half {
            put(cx, mc::fcvt(FpFormat::S, V30, FpFormat::H, V30));
            put(cx, mc::fcvt(FpFormat::S, V31, FpFormat::H, V31));
        }
        put(cx, mc::fcmp(fp, V30, V31));
        set_lane(cx, r, lane, cond.first, cond.second);
    }
    cx.finish(inst, rd)
}

// ---- select ----

/// `%r = select <N x i1> %cond, %if_true, %if_false`: of masks,
/// `(c & t) | (f & ~c)`; of lanes (`Fpr` or `Mem`), per lane both lanes
/// into `x16`/`x17`, `tst` of the condition's bit and `csel`, the chosen
/// lane stored into the result's image (an `Fpr` result's is a scratch
/// image, loaded at the end).
///
/// # Errors
/// [`MASK_WIDE`]; what [`vector::lanes`] refuses for the values; what
/// asking for the operands reports.
pub fn select<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let n = mask_of(cx, value_ty(cx, cond))?;
    if is_mask(cx, if_true) {
        deferred_first(cx, inst, None)?;
        // The three may be one value twice (`select %c, %c, %x`): each
        // arm is put into a temporary of its own, not asked for.
        let c = xr(cx.use_reg(cond)?);
        let t = cx.temp(Class::Gpr)?;
        cx.put(if_true, t)?;
        let f = cx.temp(Class::Gpr)?;
        cx.put(if_false, f)?;
        let (rt, rf) = (xr(t).zr(), xr(f).zr());
        put(cx, mc::and(Size::X, rt, rt, c.zr()));
        put(cx, mc::bic(Size::X, rf, rf, c.zr()));
        put(cx, mc::orr(Size::X, rt, rt, rf));
        return cx.finish(inst, t);
    }
    let result = lanes_of(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    let [t, f] = images(cx, [if_true, if_false], n)?;
    let c = xr(cx.use_reg(cond)?);
    let dst = if result.shape.class == Class::Fpr {
        vector::scratch(cx, SCRATCH_IMAGES - 1)?
    } else {
        cx.def_mem(inst)?
    };
    let out = Image { at: dst, lanes: result };
    for lane in 0..n {
        load_gpr_lane(cx, &t, lane, X16);
        load_gpr_lane(cx, &f, lane, X17);
        put(cx, mc::tst_imm(Size::X, c.zr(), 1u64 << lane));
        put(cx, mc::csel(Size::X, X16.zr(), X16.zr(), X17.zr(), Cond::Ne));
        let mut at = lane_at(cx, &out, lane, X17);
        store_gpr_at(cx, result.gpr_store(), X16.zr(), &mut at);
    }
    finish_image(cx, inst, &out)
}

/// The end of a loop that wrote the image `out` of the result: an `Fpr`
/// result is loaded into its register.
fn finish_image<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, out: &Image) -> CgResult<()> {
    lanes::finish(cx, inst, &out.lanes, out.at)
}

/// Where the image of a vector result of lanes `result` is written: its
/// slot (`Mem`) or the last scratch image (`Fpr`).
fn result_image<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, result: Lanes) -> CgResult<Image> {
    let at = if result.shape.class == Class::Fpr {
        vector::scratch(cx, SCRATCH_IMAGES - 1)?
    } else {
        cx.def_mem(inst)?
    };
    Ok(Image { at, lanes: result })
}

// ---- casts ----

/// `%r = <cast> %arg` with a mask on one side (not `bitcast`, which is a
/// copy): `trunc` of lanes to a mask (bit 0 of each lane), `fptoui` and
/// `fptosi` to a mask (each lane converted to `i1` by
/// `float::lane_convert`); `zext`/`sext` of a mask to lanes (`ubfx` /
/// `sbfx` of the lane's bit), `uitofp`/`sitofp` of a mask (the bit
/// extended, then `float::lane_convert`).
///
/// # Errors
/// [`MASK_WIDE`]; what [`vector::lanes`] refuses for the lanes' side;
/// `float::CAST` for a conversion it refuses; `common`'s cast name for
/// another opcode.
#[allow(clippy::many_single_char_names, reason = "registers named as in the instructions")]
pub fn cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    use Opcode as O;
    let op = inst.opcode();
    if is_mask(cx, arg) {
        let n = mask_of(cx, value_ty(cx, arg))?;
        let to = lanes_of(cx, inst.ty())?;
        if !matches!(op, O::ZExt | O::SExt | O::UIToFP | O::SIToFP) {
            return unsupported(int::CAST);
        }
        if to.shape.elem == Elem::BFloat {
            return unsupported(vector::BFLOAT);
        }
        deferred_first(cx, inst, None)?;
        let c = xr(cx.use_reg(arg)?);
        let out = result_image(cx, inst, to)?;
        let signed = matches!(op, O::SExt | O::SIToFP);
        for lane in 0..n {
            let extract = if signed { mc::sbfx } else { mc::ubfx };
            if matches!(op, O::ZExt | O::SExt) {
                put(cx, extract(Size::X, X16.zr(), c.zr(), lane, 1));
                let mut at = lane_at(cx, &out, lane, X17);
                store_gpr_at(cx, to.gpr_store(), X16.zr(), &mut at);
            } else {
                let t = cx.temp(Class::Gpr)?;
                put(cx, extract(Size::X, xr(t).zr(), c.zr(), lane, 1));
                let d = cx.temp(Class::Fpr)?;
                float::lane_convert(cx, op, TypeId::I1, to.shape.elem_ty, t, d)?;
                let mut at = lane_at(cx, &out, lane, X16);
                vector::store_lane(cx, &to, d, &mut at);
                cx.free_temp(d);
                cx.free_temp(t);
            }
        }
        return finish_image(cx, inst, &out);
    }
    let n = mask_of(cx, inst.ty())?;
    let from = lanes_of(cx, value_ty(cx, arg))?;
    if !matches!(op, O::Trunc | O::FPToUI | O::FPToSI) {
        return unsupported(int::CAST);
    }
    if from.shape.elem == Elem::BFloat {
        return unsupported(vector::BFLOAT);
    }
    deferred_first(cx, inst, None)?;
    let [a] = images(cx, [arg], n)?;
    let rd = cx.def_reg(inst)?;
    let r = xr(rd);
    for lane in 0..n {
        if op == O::Trunc {
            load_gpr_lane(cx, &a, lane, X16);
            put_bit(cx, r, X16, lane);
        } else {
            let s = cx.temp(Class::Fpr)?;
            let mut at = lane_at(cx, &a, lane, X16);
            vector::load_lane(cx, &from, s, &mut at);
            let d = cx.temp(Class::Gpr)?;
            float::lane_convert(cx, op, from.shape.elem_ty, TypeId::I1, s, d)?;
            put_bit(cx, r, xr(d), lane);
            cx.free_temp(d);
            cx.free_temp(s);
        }
    }
    cx.finish(inst, rd)
}

// ---- element access ----

/// `%r = extractelement <N x i1> %vector, %index`: the lane's bit, by
/// `ubfx` (a constant index; lane 0 for one out of range) or `lsr` by the
/// lane number (a variable index, masked or clamped as
/// `vector::lane_number` does).
///
/// # Errors
/// [`MASK_WIDE`]; [`vector::INDEX`]; what asking for the operands
/// reports.
pub fn extract_element<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    vector: Value,
    at: Value,
) -> CgResult<()> {
    let n = mask_of(cx, value_ty(cx, vector))?;
    deferred_first(cx, inst, None)?;
    let idx = mask_index(cx, at, n)?;
    let c = cx.use_reg(vector)?;
    let rc = xr(c).zr();
    match idx {
        Index::Lane(lane) => put(cx, mc::ubfx(Size::X, rc, rc, lane, 1)),
        Index::Poison => {}
        Index::Reg { reg, bits } => {
            vector::lane_number(cx, X17, reg, bits, n);
            put(cx, mc::lsrv(Size::X, rc, rc, X17.zr()));
        }
    }
    cx.finish(inst, c)
}

/// The lane index operand `at` of a mask of `n` lanes ([`vector::index`]
/// over the mask's lane count).
fn mask_index(cx: &mut FnCx<'_, A64>, at: Value, n: u32) -> CgResult<Index> {
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
    Ok(Index::Reg { reg: xr(cx.use_reg(at)?), bits })
}

/// `%r = insertelement <N x i1> %vector, i1 %value, %index`: `bfi` of
/// bit 0 of the value at the lane (a constant index; nothing for one out
/// of range), or, for a variable index, the lane's bit cleared and set
/// from the value shifted to it (`lslv` by the lane number).
///
/// # Errors
/// [`MASK_WIDE`]; [`vector::INDEX`]; what asking for the operands
/// reports.
pub fn insert_element<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    vector: Value,
    value: Value,
    at: Value,
) -> CgResult<()> {
    let n = mask_of(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    let c = cx.use_reg(vector)?;
    let rc = xr(c).zr();
    let e = xr(cx.use_reg(value)?);
    // An index that is the value itself (`i1` both) is read from its
    // register before the value is shifted in it.
    let idx = if at == value { Index::Reg { reg: e, bits: 1 } } else { mask_index(cx, at, n)? };
    match idx {
        Index::Lane(lane) => put(cx, mc::bfi(Size::X, rc, e.zr(), lane, 1)),
        Index::Poison => {}
        Index::Reg { reg, bits } => {
            vector::lane_number(cx, X17, reg, bits, n);
            mov_imm(cx, Size::X, X16, 1);
            put(cx, mc::lslv(Size::X, X16.zr(), X16.zr(), X17.zr()));
            put(cx, mc::bic(Size::X, rc, rc, X16.zr()));
            put(cx, mc::and_imm(Size::X, X16.sp(), e.zr(), 1));
            put(cx, mc::lslv(Size::X, X16.zr(), X16.zr(), X17.zr()));
            put(cx, mc::orr(Size::X, rc, rc, X16.zr()));
        }
    }
    cx.finish(inst, c)
}

/// Where lane `entry` of a shuffle's concatenated sources of `n` lanes
/// each is: source 0 or 1 and the lane; `None` when undefined.
fn source(entry: u32, n: u32) -> Option<(usize, u32)> {
    if entry == hod_ir::SHUFFLE_UNDEF || entry >= 2 * n {
        None
    } else if entry < n {
        Some((0, entry))
    } else {
        Some((1, entry - n))
    }
}

/// `%r = shufflevector <N x i1> %first, %second, <mask>`: the result's
/// lanes in runs of consecutive lanes of one source, each run one `ubfx`
/// (to bit 0) and one `bfi` (to its place); undefined lanes 0. A source
/// no entry names is not asked for.
///
/// # Errors
/// [`MASK_WIDE`] for either type; what asking for the operands reports.
pub fn shuffle_vector<'a>(
    cx: &mut FnCx<'a, A64>,
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
        if let Some((which, _)) = source(entry, n) {
            used[which] = true;
        }
    }
    let mut regs = [RegZr::ZR; 2];
    if used[0] {
        regs[0] = xr(cx.use_reg(first)?).zr();
    }
    if used[1] {
        regs[1] = if second == first && used[0] { regs[0] } else { xr(cx.use_reg(second)?).zr() };
    }
    let rd = cx.def_reg(inst)?;
    let r = xr(rd).zr();
    put(cx, mc::mov(Size::X, r, RegZr::ZR));
    let mut k = 0usize;
    while k < mask.len() {
        let Some((which, lane)) = source(mask[k], n) else {
            k += 1;
            continue;
        };
        let mut len = 1usize;
        while k + len < mask.len() && source(mask[k + len], n) == Some((which, lane + len as u32)) {
            len += 1;
        }
        let (at, width) = (k as u32, len as u32);
        let src = regs[which];
        if lane == 0 {
            put(cx, mc::bfi(Size::X, r, src, at, width));
        } else {
            put(cx, mc::ubfx(Size::X, X16.zr(), src, lane, width));
            put(cx, mc::bfi(Size::X, r, X16.zr(), at, width));
        }
        k += len;
    }
    cx.finish(inst, rd)
}

// ---- stores ----

/// `store <N x i1> %value, ptr %ptr`: the bits from `N` up to the byte
/// boundary cleared (`and`, in the value's register: they are
/// unspecified there), then the `⌈N/8⌉` bytes stored at the pointer (an
/// odd size through `v31`/`v30`, as glue stores a value).
///
/// # Errors
/// What asking for the operands reports.
pub fn store<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, value: Value, ptr: Value) -> CgResult<()> {
    let n = mask_of(cx, value_ty(cx, value))?;
    deferred_first(cx, inst, None)?;
    let bytes = n.div_ceil(8);
    let src = if const_operand(cx, value, n) == Some(0) {
        None
    } else {
        let r = xr(cx.use_reg(value)?);
        if n % 8 != 0 {
            put(cx, mc::and_imm(Size::X, r.sp(), r.zr(), low_bits(n)));
        }
        Some(r)
    };
    let p = xr(cx.use_reg(ptr)?);
    let mut at = Cursor::at_reg(p, 0, X16);
    let ms = match bytes {
        1 => MemSize::B,
        2 => MemSize::H,
        4 => MemSize::W,
        8 => MemSize::X,
        _ => {
            match src {
                Some(r) => store_gpr_bytes(cx, r, bytes, &mut at),
                None => zero_bytes(cx, &mut at, bytes),
            }
            return Ok(());
        }
    };
    store_gpr_at(cx, ms, src.map_or(RegZr::ZR, Reg::zr), &mut at);
    Ok(())
}

// ---- reductions ----

/// The reductions, as the bit operation of the fold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fold {
    Add,
    Mul,
    And,
    Or,
    Xor,
    Smax,
    Smin,
    Umax,
    Umin,
    Fadd,
    Fmul,
    Fmax,
    Fmin,
}

/// The fold of a `llvm.vector.reduce.*` intrinsic.
fn reduction_kind(which: Intrinsic<'_>) -> Option<Fold> {
    use Intrinsic as I;
    Some(match which {
        I::VectorReduceAdd => Fold::Add,
        I::VectorReduceMul => Fold::Mul,
        I::VectorReduceAnd => Fold::And,
        I::VectorReduceOr => Fold::Or,
        I::VectorReduceXor => Fold::Xor,
        I::VectorReduceSmax => Fold::Smax,
        I::VectorReduceSmin => Fold::Smin,
        I::VectorReduceUmax => Fold::Umax,
        I::VectorReduceUmin => Fold::Umin,
        I::VectorReduceFadd => Fold::Fadd,
        I::VectorReduceFmul => Fold::Fmul,
        I::VectorReduceFmax => Fold::Fmax,
        I::VectorReduceFmin => Fold::Fmin,
        _ => return None,
    })
}

/// A call of `llvm.vector.reduce.*`: of a mask, the bit rules of the
/// module docs; of integer lanes, lane 0 then one scalar kernel per lane
/// (`int::lane_binary`, `intrinsic::lane_min_max`); `fadd`/`fmul` in
/// lane order from the start value (`float::lane_binary`: ordered, which
/// is right with and without `reassoc`); `fmax`/`fmin` as `maxnum` /
/// `minnum` lane after lane (`fpintr::lane_inline`).
///
/// # Errors
/// [`MASK_WIDE`]; what [`vector::lanes`] refuses; [`vector::BFLOAT`];
/// [`REDUCE`] for an operand list that is not the intrinsic's; what
/// asking for the operands reports.
pub fn reduce<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let Some(fold) = reduction_kind(which) else { return cx.bug("no reduction") };
    let vec = match (fold, args) {
        (Fold::Fadd | Fold::Fmul, &[_, v]) => v,
        (Fold::Fadd | Fold::Fmul, _) => return unsupported(REDUCE),
        (_, &[v]) => v,
        _ => return unsupported(REDUCE),
    };
    if is_mask(cx, vec) {
        return reduce_mask(cx, inst, fold, vec);
    }
    let l = lanes_of(cx, value_ty(cx, vec))?;
    if l.shape.elem == Elem::BFloat {
        return unsupported(vector::BFLOAT);
    }
    let float = l.shape.elem.is_float();
    if float != matches!(fold, Fold::Fadd | Fold::Fmul | Fold::Fmax | Fold::Fmin) {
        return unsupported(REDUCE);
    }
    deferred_first(cx, inst, None)?;
    let n = l.count();
    let [a] = images(cx, [vec], n)?;
    if float {
        return reduce_float(cx, inst, fold, args, &a);
    }
    let rd = cx.def_reg(inst)?;
    let r = xr(rd);
    let bits = l.shape.elem.bits();
    load_gpr_lane(cx, &a, 0, r);
    for lane in 1..n {
        load_gpr_lane(cx, &a, lane, X17);
        let op = match fold {
            Fold::Add => Opcode::Add,
            Fold::Mul => Opcode::Mul,
            Fold::And => Opcode::And,
            Fold::Or => Opcode::Or,
            Fold::Xor => Opcode::Xor,
            _ => {
                let min_max = match fold {
                    Fold::Smax => Intrinsic::Smax,
                    Fold::Smin => Intrinsic::Smin,
                    Fold::Umax => Intrinsic::Umax,
                    _ => Intrinsic::Umin,
                };
                intrinsic::lane_min_max(cx, min_max, r, X17, bits);
                continue;
            }
        };
        int::lane_binary(cx, op, bits, r, X17);
    }
    cx.finish(inst, rd)
}

/// The floating-point reductions of [`reduce`] over the image `a`.
fn reduce_float<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    fold: Fold,
    args: &'a [Value],
    a: &Image,
) -> CgResult<()> {
    let elem = a.lanes.shape.elem_ty;
    let view: VMemSize = a.lanes.fpr_view();
    let (acc, first) = match fold {
        Fold::Fadd | Fold::Fmul => (cx.use_reg(args[0])?, 0),
        _ => {
            let rd = cx.def_reg(inst)?;
            let mut at = lane_at(cx, a, 0, X16);
            load_fpr_at(cx, view, vr(rd), &mut at);
            (rd, 1)
        }
    };
    let t = cx.temp(Class::Fpr)?;
    for lane in first..a.lanes.count() {
        let mut at = lane_at(cx, a, lane, X16);
        load_fpr_at(cx, view, vr(t), &mut at);
        match fold {
            Fold::Fadd => float::lane_binary(cx, Opcode::FAdd, elem, vr(acc), vr(t))?,
            Fold::Fmul => float::lane_binary(cx, Opcode::FMul, elem, vr(acc), vr(t))?,
            _ => {
                let which = if fold == Fold::Fmax { Intrinsic::MaxNum } else { Intrinsic::MinNum };
                crate::fpintr::lane_inline(cx, which, elem, [vr(acc), vr(t), V31], X16, 0)?;
            }
        }
    }
    cx.free_temp(t);
    cx.finish(inst, acc)
}

/// [`reduce`] of a mask `vec`: a flag from its low `N` bits, `cset`.
fn reduce_mask<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, fold: Fold, vec: Value) -> CgResult<()> {
    let n = mask_of(cx, value_ty(cx, vec))?;
    deferred_first(cx, inst, None)?;
    let c = cx.use_reg(vec)?;
    let rc = xr(c).zr();
    let all = low_bits(n);
    let cond = match fold {
        // Any lane set.
        Fold::Or | Fold::Umax | Fold::Smin => {
            if n == 64 {
                put(cx, mc::cmp(Size::X, rc, RegZr::ZR));
            } else {
                put(cx, mc::tst_imm(Size::X, rc, all));
            }
            Cond::Ne
        }
        // Every lane set.
        Fold::And | Fold::Umin | Fold::Smax | Fold::Mul => {
            put(cx, mc::mvn(Size::X, X16.zr(), rc));
            if n == 64 {
                put(cx, mc::cmp(Size::X, X16.zr(), RegZr::ZR));
            } else {
                put(cx, mc::tst_imm(Size::X, X16.zr(), all));
            }
            Cond::Eq
        }
        // The parity: the low `N` bits folded onto bit 0.
        Fold::Xor | Fold::Add => {
            let x = X16.zr();
            if n == 64 {
                put(cx, mc::mov(Size::X, x, rc));
            } else {
                put(cx, mc::and_imm(Size::X, X16.sp(), rc, all));
            }
            let mut shift = 32;
            while shift >= 1 {
                if shift < n.next_power_of_two() {
                    put(cx, mc::eor_shifted(Size::X, x, x, x, Shift::Lsr, shift));
                }
                shift /= 2;
            }
            put(cx, mc::and_imm(Size::X, xr(c).sp(), x, 1));
            return cx.finish(inst, c);
        }
        _ => return unsupported(REDUCE),
    };
    put(cx, mc::cset(Size::X, rc, cond));
    cx.finish(inst, c)
}
