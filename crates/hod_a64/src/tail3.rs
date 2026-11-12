//! The short tail, third part (WP-101, `spec/codegen.md` §5.5.1
//! "WP-101"): first refusals of the strict libc++ run and of the corpus.
//! Each is reached by one call from the module that refused it.
//!
//! - Vectors of 16-byte elements, `i128` and `fp128` ("wide lanes",
//!   [`lower`], from the top of `Target::lower`, before the masks):
//!   `insertelement`, `extractelement` of `fp128` lanes (`i128` lanes are
//!   WP-095's `tail2`), `shufflevector`, element-wise arithmetic, `fneg`,
//!   `icmp` and `fcmp` into masks, `select` on a mask, and the casts
//!   between masks or integer lanes and `i128` lanes.
//!
//! # Wide lanes
//!
//! A vector of `N` 16-byte elements is of class `Mem` (its image in a
//! slot, lane `i` at `16 · i`), except `<1 x i128>` and `<1 x fp128>`,
//! which are of class `Fpr` (one `Q` register). Every operation works on
//! images: a register operand is stored into a scratch image of `vector`
//! (`vector::scratch`), a register result is computed in one and loaded
//! at the end. A lane of `i128` is two limbs, computed through `x16`,
//! `x17` (and two pool temporaries for `mul`) as `aggr` computes an
//! `i128`; a lane of `fp128` is computed by the runtime call `llc` makes
//! for the scalar (`__addtf3`, `__eqtf2`, ...), one per lane, so a mask
//! built by `fcmp` is kept in a frame slot across the calls.
//!
//! - Vectors of `bfloat` ([`lower`] too): arithmetic, `fneg`, `fcmp`,
//!   and `fpext`/`fptrunc` with `float` and `double` lanes, lane by lane
//!   as the scalar `bfloat` of WP-095 (`tail2::widen`, `tail2::narrow`):
//!   hod chooses no `FEAT_BF16` instruction.
//!
//! - The `bfloat` NEON intrinsics `bfcvt`, `bfcvtn`, `bfcvtn2`, `bfdot`,
//!   `bfmlalb`, `bfmlalt`, `bfmmla` ([`bf16_intrinsic`], from
//!   `intrinsic.rs`): one instruction each, as `llc` selects them (the
//!   call is the program's own request for `FEAT_BF16`).
//!
//! A lane index is `vector`'s rule: a constant out of range writes
//! nothing, a variable one is masked (`N` a power of two) or clamped to
//! lane 0. An undefined lane of a shuffle is zero.

use hod_cg::{CallArg, CgResult, Class, FnCx, FrameRef, PReg, RuntimeFn, Target};
use hod_ir::{
    FloatPredicate, Inst, InstKind, IntPredicate, Intrinsic, Opcode, TypeId, TypeKind, TypeTable,
    Value,
};
use hod_mc_a64 as mc;
use hod_mc_a64::{
    Cond, Dot, FpFormat, FpSize, Load, MemSize, Reg, RegZr, Shift, Size, VMemSize, VReg, VecBits,
};

use crate::aggr::{ld, st};
use crate::common::{
    Cursor, V30, V31, X16, X17, copy_bytes, deferred_first, load_fpr_at, load_gpr_at, mov_imm, put,
    store_fpr_at, store_gpr_at, unsupported, value_ty, vr, xr, zero_bytes,
};
use crate::mask::{MAX_MASK, low_bits, mask_lanes};
use crate::target::A64;
use crate::vector::{self, Elem, Index, Lanes, MAX_LANES, SCRATCH_IMAGES, Shape};
use crate::{float, tail2};

// ---- refusals ----

/// A shift, division or remainder on `i128` lanes (`llc` expands each
/// lane as it expands the scalar; hod lowers `add`, `sub`, `mul` and the
/// bitwise operations of such lanes).
pub const I128_OP: &str = "a64:vector-i128-op";
/// A conversion with `fp128` lanes on one side, or between `i128` lanes
/// and floating-point lanes (hod lowers the integer casts between masks,
/// integer lanes and `i128` lanes).
pub const WIDE_CAST: &str = "a64:vector-wide-cast";

/// A conversion between `bfloat` lanes and integer, `half` or `fp128`
/// lanes (hod lowers `fpext` and `fptrunc` with `float` and `double`
/// lanes).
pub const BFLOAT_CAST: &str = "a64:vector-bfloat-cast";

/// Every refusal this module names.
pub const REASONS: [&str; 3] = [I128_OP, WIDE_CAST, BFLOAT_CAST];

// ---- the hook ----

/// Whether `ty` is a fixed vector of `i128` or `fp128`.
fn is_wide(types: &TypeTable, ty: TypeId) -> bool {
    match types.kind(ty) {
        TypeKind::Vector { elem, .. } => {
            matches!(types.kind(elem), TypeKind::Int(128) | TypeKind::Fp128)
        }
        _ => false,
    }
}

/// The hook at the top of `Target::lower` (before `mask::lower`): an
/// instruction on the lanes of a vector of `i128` or `fp128` is lowered
/// here; `None` for every other one.
pub fn lower<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> Option<CgResult<()>> {
    let types = cx.types();
    let wide_result = is_wide(types, inst.ty());
    Some(match inst.kind() {
        InstKind::Binary { lhs, rhs } if wide_result => binary(cx, inst, lhs, rhs),
        InstKind::Unary { arg } if wide_result && inst.opcode() == Opcode::FNeg => {
            fneg(cx, inst, arg)
        }
        InstKind::Cast { arg }
            if inst.opcode() != Opcode::BitCast
                && (wide_result || is_wide(types, value_ty(cx, arg))) =>
        {
            cast(cx, inst, arg)
        }
        InstKind::ICmp { pred, lhs, rhs } if is_wide(types, value_ty(cx, lhs)) => {
            icmp(cx, inst, pred, lhs, rhs)
        }
        InstKind::FCmp { pred, lhs, rhs } if is_wide(types, value_ty(cx, lhs)) => {
            fcmp(cx, inst, pred, lhs, rhs)
        }
        InstKind::Select { cond, if_true, if_false }
            if wide_result && types.is_vector(value_ty(cx, cond)) =>
        {
            select(cx, inst, cond, if_true, if_false)
        }
        InstKind::InsertElement { vector, value, index } if wide_result => {
            insert_element(cx, inst, vector, value, index)
        }
        InstKind::ExtractElement { vector, index } if is_wide(types, value_ty(cx, vector)) => {
            extract_element(cx, inst, vector, index)
        }
        InstKind::ShuffleVector { first, second, mask } if wide_result => {
            shuffle_vector(cx, inst, first, second, mask)
        }
        InstKind::Binary { lhs, rhs } if is_bf(types, inst.ty()) => bf_binary(cx, inst, lhs, rhs),
        InstKind::Unary { arg } if inst.opcode() == Opcode::FNeg && is_bf(types, inst.ty()) => {
            bf_fneg(cx, inst, arg)
        }
        InstKind::Cast { arg }
            if inst.opcode() != Opcode::BitCast
                && (is_bf(types, inst.ty()) || is_bf(types, value_ty(cx, arg))) =>
        {
            bf_cast(cx, inst, arg)
        }
        InstKind::FCmp { pred, lhs, rhs } if is_bf(types, value_ty(cx, lhs)) => {
            bf_fcmp(cx, inst, pred, lhs, rhs)
        }
        _ => return None,
    })
}

// ---- images ----

/// The lanes of the wide vector type `ty`.
fn lanes_of(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<Lanes> {
    match Shape::of(cx.types(), ty) {
        Some(shape) if matches!(shape.elem, Elem::Int(128) | Elem::Fp128) => {
            if shape.lanes > MAX_LANES {
                return unsupported(vector::LENGTH);
            }
            Ok(Lanes { shape, log2: 4 })
        }
        _ => cx.bug("a wide-lane operation on another type"),
    }
}

/// Lane `lane` of the image at `at` (16-byte lanes).
fn lane(at: FrameRef, lane: u32) -> FrameRef {
    at.offset_by(16 * lane)
}

/// Where the image of vector operand `value` is: the slot of a `Mem`
/// vector (a constant written to a slot of its own), or, for a register
/// vector, scratch image `*k` (then `*k` moves on) with the register
/// stored into it and given back.
fn image(cx: &mut FnCx<'_, A64>, value: Value, k: &mut u32) -> CgResult<FrameRef> {
    if cx.class_of(value) != Class::Fpr {
        return cx.use_mem(value);
    }
    let shape = match Shape::of(cx.types(), value_ty(cx, value)) {
        Some(shape) => shape,
        None => return cx.bug("the image of something that is no vector"),
    };
    let r = cx.use_reg(value)?;
    let at = vector::spill(cx, vr(r), &shape, *k)?;
    *k += 1;
    cx.release(value);
    cx.free_temp(r);
    Ok(at)
}

/// The images of `values` (an operand repeated shares its image).
fn images<const K: usize>(cx: &mut FnCx<'_, A64>, values: [Value; K]) -> CgResult<[FrameRef; K]> {
    let mut out = [FrameRef::Local(0); K];
    let mut k = 0;
    for i in 0..K {
        out[i] = match values[..i].iter().position(|&w| w == values[i]) {
            Some(j) => out[j],
            None => image(cx, values[i], &mut k)?,
        };
    }
    Ok(out)
}

/// Where the result of `inst` is written: its slot (`Mem`) or the last
/// scratch image (a register result, loaded by [`finish`]).
fn result_image<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> CgResult<FrameRef> {
    if A64.class_of(cx.types(), inst.ty()) == Class::Fpr {
        vector::scratch(cx, SCRATCH_IMAGES - 1)
    } else {
        cx.def_mem(inst)
    }
}

/// The end of a lowering that wrote the result's image `out`: a register
/// result is loaded from it.
fn finish<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, out: FrameRef) -> CgResult<()> {
    if A64.class_of(cx.types(), inst.ty()) != Class::Fpr {
        return Ok(());
    }
    let view = if cx.types().size_of(inst.ty()) > 8 { VMemSize::Q } else { VMemSize::D };
    let rd = cx.def_reg(inst)?;
    let mut at = Cursor::frame(cx, out, X16);
    load_fpr_at(cx, view, vr(rd), &mut at);
    cx.finish(inst, rd)
}

/// Copies the 16 bytes at `src` to `dst` (through `v31`).
fn copy16(cx: &mut FnCx<'_, A64>, dst: FrameRef, src: FrameRef) {
    let mut from = Cursor::frame(cx, src, X16);
    let mut to = Cursor::frame(cx, dst, X17);
    copy_bytes(cx, &mut to, &mut from, 16);
}

// ---- element access ----

/// `%r = insertelement %vector, %value, %index` of wide lanes. A `Mem`
/// vector is copied to the result's slot and the lane written there (16
/// bytes from the `i128`'s slot, or `str q` of the `fp128`); for
/// `<1 x i128>` and `<1 x fp128>` the result is the inserted value itself
/// (any index in range names lane 0), or the vector for a constant index
/// out of range.
///
/// # Errors
/// [`vector::LENGTH`] beyond [`MAX_LANES`] lanes; what [`vector::index`]
/// and asking for the operands report.
pub fn insert_element<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    vector: Value,
    value: Value,
    at: Value,
) -> CgResult<()> {
    let lanes = lanes_of(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    let idx = vector::index(cx, at, &lanes)?;
    let is_int = lanes.shape.elem == Elem::Int(128);
    if lanes.shape.class == Class::Fpr {
        if idx == Index::Poison {
            let v = cx.use_reg(vector)?;
            return cx.finish(inst, v);
        }
        if !is_int {
            let e = cx.use_reg(value)?;
            return cx.finish(inst, e);
        }
        let src = cx.use_mem(value)?;
        let rd = cx.def_reg(inst)?;
        let mut from = Cursor::frame(cx, src, X17);
        load_fpr_at(cx, VMemSize::Q, vr(rd), &mut from);
        return cx.finish(inst, rd);
    }
    // The element first (asking may emit), then the copy of the vector,
    // then the lane (`lane_cursor` writes `x16` and `x17`).
    let elem = if is_int { Err(cx.use_mem(value)?) } else { Ok(cx.use_reg(value)?) };
    let r = cx.def_mem(inst)?;
    vector::write_mem(cx, r, vector)?;
    if idx == Index::Poison {
        return Ok(());
    }
    let mut dst = vector::lane_cursor(cx, r, &lanes, idx);
    match elem {
        Ok(e) => store_fpr_at(cx, VMemSize::Q, vr(e), &mut dst),
        Err(src) => {
            let mut from = Cursor::frame(cx, src, X17);
            copy_bytes(cx, &mut dst, &mut from, 16);
        }
    }
    Ok(())
}

/// `%r = extractelement %vector, %index` of wide lanes: `fp128` lanes
/// here (`ldr q` of the lane; `<1 x fp128>` gives its register), `i128`
/// lanes to [`crate::tail2::extract_element`].
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
    let lanes = lanes_of(cx, value_ty(cx, vector))?;
    if lanes.shape.elem != Elem::Fp128 {
        return tail2::extract_element(cx, inst, vector, at);
    }
    deferred_first(cx, inst, None)?;
    let idx = vector::index(cx, at, &lanes)?;
    if lanes.shape.class == Class::Fpr {
        let v = cx.use_reg(vector)?;
        return cx.finish(inst, v);
    }
    let image = cx.use_mem(vector)?;
    let rd = cx.def_reg(inst)?;
    let idx = if idx == Index::Poison { Index::Lane(0) } else { idx };
    let mut src = vector::lane_cursor(cx, image, &lanes, idx);
    load_fpr_at(cx, VMemSize::Q, vr(rd), &mut src);
    cx.finish(inst, rd)
}

/// `%r = shufflevector %first, %second, <mask>` of wide lanes: each
/// result lane copied from the source lane its entry names, an undefined
/// one zero.
///
/// # Errors
/// [`vector::LENGTH`]; what asking for the operands reports.
pub fn shuffle_vector<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    first: Value,
    second: Value,
    mask: &[u32],
) -> CgResult<()> {
    let n = lanes_of(cx, value_ty(cx, first))?.count();
    lanes_of(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    let source = |entry: u32| -> Option<(usize, u32)> {
        if entry == hod_ir::SHUFFLE_UNDEF || entry >= 2 * n {
            None
        } else if entry < n {
            Some((0, entry))
        } else {
            Some((1, entry - n))
        }
    };
    let mut used = [false; 2];
    for &entry in mask {
        if let Some((which, _)) = source(entry) {
            used[which] = true;
        }
    }
    let mut srcs = [None; 2];
    let mut k = 0;
    for (which, value) in [first, second].into_iter().enumerate() {
        if !used[which] {
            continue;
        }
        srcs[which] = Some(if which == 1 && second == first && srcs[0].is_some() {
            srcs[0].unwrap_or(FrameRef::Local(0))
        } else {
            image(cx, value, &mut k)?
        });
    }
    let out = result_image(cx, inst)?;
    for (i, &entry) in (0u32..).zip(mask) {
        match source(entry).and_then(|(which, l)| Some((srcs[which]?, l))) {
            Some((src, l)) => copy16(cx, lane(out, i), lane(src, l)),
            None => {
                let mut to = Cursor::frame(cx, lane(out, i), X17);
                zero_bytes(cx, &mut to, 16);
            }
        }
    }
    finish(cx, inst, out)
}

// ---- arithmetic ----

/// `%r = <op> %lhs, %rhs` element-wise on wide lanes: `add`, `sub` (a
/// carry chain), `and`, `or`, `xor` and `mul` of `i128` lanes as `aggr`
/// computes the scalar; `fadd` .. `frem` of `fp128` lanes as the runtime
/// call of the scalar (`__addtf3`, `__subtf3`, `__multf3`, `__divtf3`,
/// `fmodl`), one per lane.
///
/// # Errors
/// [`I128_OP`] for the shifts, divisions and remainders; what the
/// operands and the calls report.
pub fn binary<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    use Opcode as O;
    let lanes = lanes_of(cx, inst.ty())?;
    let op = inst.opcode();
    let func = match op {
        O::Add | O::Sub | O::And | O::Or | O::Xor | O::Mul => None,
        O::FAdd => Some(RuntimeFn::AddTf3),
        O::FSub => Some(RuntimeFn::SubTf3),
        O::FMul => Some(RuntimeFn::MulTf3),
        O::FDiv => Some(RuntimeFn::DivTf3),
        O::FRem => Some(RuntimeFn::Fmodl),
        _ => return unsupported(I128_OP),
    };
    deferred_first(cx, inst, None)?;
    let [a, b] = images(cx, [lhs, rhs])?;
    let out = result_image(cx, inst)?;
    let n = lanes.count();
    match func {
        Some(func) => {
            for l in 0..n {
                let t1 = cx.temp(Class::Fpr)?;
                let t2 = cx.temp(Class::Fpr)?;
                load_q(cx, t1, lane(a, l));
                load_q(cx, t2, lane(b, l));
                let Some(r) = cx.call_runtime(func, &[CallArg::Reg(t1), CallArg::Reg(t2)], None)?
                else {
                    return cx.bug("a runtime function returned nothing");
                };
                let mut to = Cursor::frame(cx, lane(out, l), X16);
                store_fpr_at(cx, VMemSize::Q, vr(r), &mut to);
                cx.free_temp(r);
            }
        }
        None if op == O::Mul => {
            let t0 = xr(cx.temp(Class::Gpr)?);
            let t1 = xr(cx.temp(Class::Gpr)?);
            for l in 0..n {
                mul128(cx, [t0, t1], lane(out, l), lane(a, l), lane(b, l));
            }
        }
        None => {
            for l in 0..n {
                limbwise(cx, op, lane(out, l), lane(a, l), lane(b, l));
            }
        }
    }
    finish(cx, inst, out)
}

/// `ldr q` of the 16 bytes at `at` into `rt`.
fn load_q(cx: &mut FnCx<'_, A64>, rt: PReg, at: FrameRef) {
    let mut from = Cursor::frame(cx, at, X16);
    load_fpr_at(cx, VMemSize::Q, vr(rt), &mut from);
}

/// `add`, `sub` and the bitwise operations of the `i128` at `a` and `b`
/// into `r`, limb by limb (`aggr`'s carry chain).
fn limbwise(cx: &mut FnCx<'_, A64>, op: Opcode, r: FrameRef, a: FrameRef, b: FrameRef) {
    let (x, y) = (X16.zr(), X17.zr());
    for limb in 0..2 {
        ld(cx, X16, a, limb);
        ld(cx, X17, b, limb);
        let first = limb == 0;
        let word = match op {
            Opcode::Add if first => mc::adds(Size::X, x, x, y),
            Opcode::Add => mc::adc(Size::X, x, x, y),
            Opcode::Sub if first => mc::subs(Size::X, x, x, y),
            Opcode::Sub => mc::sbc(Size::X, x, x, y),
            Opcode::And => mc::and(Size::X, x, x, y),
            Opcode::Or => mc::orr(Size::X, x, x, y),
            _ => mc::eor(Size::X, x, x, y),
        };
        put(cx, word);
        st(cx, x, X17, r, limb);
    }
}

/// `mul` of the `i128` at `a` and `b` into `r` modulo 2^128: `lo =
/// a0·b0`, `hi = umulh(a0, b0) + a0·b1 + a1·b0` (`aggr`'s two-limb
/// product), with the two pool temporaries `t`.
fn mul128(cx: &mut FnCx<'_, A64>, t: [Reg; 2], r: FrameRef, a: FrameRef, b: FrameRef) {
    let (x, y) = (X16.zr(), X17.zr());
    let [t0, t1] = t;
    ld(cx, t0, a, 0);
    ld(cx, t1, b, 0);
    ld(cx, X16, a, 1);
    ld(cx, X17, b, 1);
    put(cx, mc::mul(Size::X, y, t0.zr(), y)); // a0·b1
    put(cx, mc::madd(Size::X, x, x, t1.zr(), y)); // a1·b0 + a0·b1
    put(cx, mc::umulh(y, t0.zr(), t1.zr()));
    put(cx, mc::add(Size::X, x, x, y));
    put(cx, mc::mul(Size::X, t0.zr(), t0.zr(), t1.zr()));
    st(cx, t0.zr(), X17, r, 0);
    st(cx, x, X17, r, 1);
}

/// `%r = fneg %arg` of `fp128` lanes: bit 127 of each lane flipped,
/// nothing else (NaNs included), as the scalar.
///
/// # Errors
/// What the operand reports; `Internal` for `i128` lanes (never valid IR).
pub fn fneg<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let lanes = lanes_of(cx, inst.ty())?;
    if lanes.shape.elem != Elem::Fp128 {
        return cx.bug("fneg of integer lanes");
    }
    deferred_first(cx, inst, None)?;
    let [a] = images(cx, [arg])?;
    let out = result_image(cx, inst)?;
    for l in 0..lanes.count() {
        ld(cx, X16, lane(a, l), 0);
        st(cx, X16.zr(), X17, lane(out, l), 0);
        ld(cx, X16, lane(a, l), 1);
        put(cx, mc::eor_imm(Size::X, X16.sp(), X16.zr(), 1 << 63));
        st(cx, X16.zr(), X17, lane(out, l), 1);
    }
    finish(cx, inst, out)
}

// ---- compares and select ----

/// The number of lanes of the mask type `ty`, if it is one that lives in
/// a general register.
fn mask_of(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<u32> {
    match mask_lanes(cx.types(), ty) {
        Some(n @ 1..=MAX_MASK) => Ok(n),
        Some(_) => unsupported(vector::MASK_WIDE),
        None => cx.bug("a mask operation on something that is no mask"),
    }
}

/// Sets lane `lane` of the mask in `r` to whether `cond` holds: `cset`
/// into `r` for lane 0, into `x16` and an `orr` at the lane's bit for
/// the others.
fn set_lane(cx: &mut FnCx<'_, A64>, r: Reg, lane: u32, cond: Cond) {
    let t = if lane == 0 { r } else { X16 };
    put(cx, mc::cset(Size::X, t.zr(), cond));
    if lane != 0 {
        put(cx, mc::orr_shifted(Size::X, r.zr(), r.zr(), X16.zr(), Shift::Lsl, lane));
    }
}

/// `%r = icmp <pred> <N x i128> %lhs, %rhs`: per lane, `eq`/`ne` by `cmp`
/// of the low limbs and `ccmp` of the high ones, the orderings by `cmp` +
/// `sbcs` (the flags of the 128-bit difference), and the lane's bit set
/// in the mask.
///
/// # Errors
/// [`vector::MASK_WIDE`] beyond 64 lanes; what the operands report.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    use IntPredicate as P;
    let n = mask_of(cx, inst.ty())?;
    lanes_of(cx, value_ty(cx, lhs))?;
    deferred_first(cx, inst, None)?;
    let [a, b] = images(cx, [lhs, rhs])?;
    let rd = cx.def_reg(inst)?;
    let r = xr(rd);
    let (swap, cond) = match pred {
        P::Eq => (false, Cond::Eq),
        P::Ne => (false, Cond::Ne),
        P::Ult => (false, Cond::Lo),
        P::Uge => (false, Cond::Hs),
        P::Ugt => (true, Cond::Lo),
        P::Ule => (true, Cond::Hs),
        P::Slt => (false, Cond::Lt),
        P::Sge => (false, Cond::Ge),
        P::Sgt => (true, Cond::Lt),
        P::Sle => (true, Cond::Ge),
    };
    let (x, y) = (X16.zr(), X17.zr());
    for l in 0..n {
        let (p, q) = if swap { (lane(b, l), lane(a, l)) } else { (lane(a, l), lane(b, l)) };
        ld(cx, X16, p, 0);
        ld(cx, X17, q, 0);
        put(cx, mc::cmp(Size::X, x, y));
        ld(cx, X16, p, 1);
        ld(cx, X17, q, 1);
        if matches!(pred, P::Eq | P::Ne) {
            // Equal low limbs: compare the high ones; else "not equal".
            put(cx, mc::ccmp(Size::X, x, y, 0, Cond::Eq));
        } else {
            put(cx, mc::sbcs(Size::X, RegZr::ZR, x, y));
        }
        set_lane(cx, r, l, cond);
    }
    cx.finish(inst, rd)
}

/// `%r = fcmp <pred> <N x fp128> %lhs, %rhs`: per lane, the scalar's
/// runtime call(s) as `float` makes them (`__eqtf2`, `__lttf2`,
/// `__letf2`, `__unordtf2`; both of `__eqtf2` and `__unordtf2` for `ueq`
/// and `one`), the result compared with 0 and the lane's bit or-ed into
/// the mask, which is kept in a frame slot across the calls. `true` and
/// `false` are all ones and zero.
///
/// # Errors
/// [`vector::MASK_WIDE`] beyond 64 lanes; what the operands and the calls
/// report.
pub fn fcmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    use FloatPredicate as P;
    use RuntimeFn::{EqTf2, LeTf2, LtTf2, UnordTf2};
    let n = mask_of(cx, inst.ty())?;
    if lanes_of(cx, value_ty(cx, lhs))?.shape.elem != Elem::Fp128 {
        return cx.bug("an fcmp of integer lanes");
    }
    deferred_first(cx, inst, None)?;
    if matches!(pred, P::True | P::False) {
        let rd = cx.def_reg(inst)?;
        let bits = if pred == P::True { low_bits(n) } else { 0 };
        mov_imm(cx, Size::X, xr(rd), bits);
        return cx.finish(inst, rd);
    }
    // (function, operands swapped, condition on the result vs 0); `ueq`
    // and `one` are two calls.
    let (func, swap, cond) = match pred {
        P::Oeq => (EqTf2, false, Cond::Eq),
        P::Une => (EqTf2, false, Cond::Ne),
        P::Olt => (LtTf2, false, Cond::Lt),
        P::Uge => (LtTf2, false, Cond::Ge),
        P::Ogt => (LtTf2, true, Cond::Lt),
        P::Ule => (LtTf2, true, Cond::Ge),
        P::Ole => (LeTf2, false, Cond::Le),
        P::Ugt => (LeTf2, false, Cond::Gt),
        P::Oge => (LeTf2, true, Cond::Le),
        P::Ult => (LeTf2, true, Cond::Gt),
        P::Ord => (UnordTf2, false, Cond::Eq),
        P::Uno => (UnordTf2, false, Cond::Ne),
        P::Ueq => (EqTf2, false, Cond::Eq),
        P::One => (EqTf2, false, Cond::Ne),
        P::True | P::False => return cx.bug("a constant fcmp predicate"),
    };
    let two = matches!(pred, P::Ueq | P::One);
    let [a, b] = images(cx, [lhs, rhs])?;
    // The mask at `acc`, the first call's result of `ueq`/`one` at `+8`.
    let acc = cx.alloc_slot(16, 8)?;
    for l in 0..n {
        let (p, q) = if swap { (lane(b, l), lane(a, l)) } else { (lane(a, l), lane(b, l)) };
        let first = quad_call(cx, func, p, q)?;
        if two {
            store_w(cx, xr(first), acc.offset_by(8));
            cx.free_temp(first);
            let unord = quad_call(cx, UnordTf2, p, q)?;
            let mut at = Cursor::frame(cx, acc.offset_by(8), X16);
            load_gpr_at(cx, Load::W, X16, &mut at);
            // `Z` when unordered or equal: `ueq` is `EQ`, `one` is `NE`.
            put(cx, mc::cmp_imm(Size::W, xr(unord).sp(), 0));
            put(cx, mc::ccmp(Size::W, X16.zr(), RegZr::ZR, 0b0100, Cond::Eq));
            cx.free_temp(unord);
        } else {
            put(cx, mc::cmp_imm(Size::W, xr(first).sp(), 0));
            cx.free_temp(first);
        }
        put(cx, mc::cset(Size::X, X16.zr(), cond));
        if l != 0 {
            let mut at = Cursor::frame(cx, acc, X17);
            load_gpr_at(cx, Load::X, X17, &mut at);
            put(cx, mc::orr_shifted(Size::X, X16.zr(), X17.zr(), X16.zr(), Shift::Lsl, l));
        }
        let mut at = Cursor::frame(cx, acc, X17);
        store_gpr_at(cx, MemSize::X, X16.zr(), &mut at);
    }
    let rd = cx.def_reg(inst)?;
    let mut at = Cursor::frame(cx, acc, X16);
    load_gpr_at(cx, Load::X, xr(rd), &mut at);
    cx.finish(inst, rd)
}

/// Calls `func` on the `fp128` values at `p` and `q`, loaded into
/// temporaries; the result's register.
fn quad_call(cx: &mut FnCx<'_, A64>, func: RuntimeFn, p: FrameRef, q: FrameRef) -> CgResult<PReg> {
    let t1 = cx.temp(Class::Fpr)?;
    let t2 = cx.temp(Class::Fpr)?;
    load_q(cx, t1, p);
    load_q(cx, t2, q);
    match cx.call_runtime(func, &[CallArg::Reg(t1), CallArg::Reg(t2)], None)? {
        Some(r) => Ok(r),
        None => cx.bug("a runtime function returned nothing"),
    }
}

/// `str w` of `rt` at `at`.
fn store_w(cx: &mut FnCx<'_, A64>, rt: Reg, at: FrameRef) {
    let mut to = Cursor::frame(cx, at, X16);
    store_gpr_at(cx, MemSize::W, rt.zr(), &mut to);
}

/// `%r = select <N x i1> %cond, %if_true, %if_false` of wide lanes: per
/// lane and limb, both limbs into `x16`/`x17`, `tst` of the condition's
/// bit and `csel`, the chosen limb stored.
///
/// # Errors
/// [`vector::MASK_WIDE`] beyond 64 lanes; what the operands report.
pub fn select<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let n = mask_of(cx, value_ty(cx, cond))?;
    lanes_of(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    let [t, f] = images(cx, [if_true, if_false])?;
    let c = xr(cx.use_reg(cond)?);
    let out = result_image(cx, inst)?;
    for l in 0..n {
        for limb in 0..2 {
            ld(cx, X16, lane(t, l), limb);
            ld(cx, X17, lane(f, l), limb);
            put(cx, mc::tst_imm(Size::X, c.zr(), 1u64 << l));
            put(cx, mc::csel(Size::X, X16.zr(), X16.zr(), X17.zr(), Cond::Ne));
            st(cx, X16.zr(), X17, lane(out, l), limb);
        }
    }
    finish(cx, inst, out)
}

// ---- casts ----

/// `%r = <cast> %arg` with wide lanes on at least one side (not
/// `bitcast`): `zext`/`sext` of a mask to `i128` lanes (`ubfx`/`sbfx` of
/// the lane's bit, the high limb 0 or the same), `trunc` of `i128` lanes
/// to a mask (bit 0 of each lane), `zext`/`sext` of integer lanes to
/// `i128` lanes (the high limb 0 or the sign), `trunc` of `i128` lanes to
/// integer lanes (the low bytes).
///
/// # Errors
/// [`WIDE_CAST`] for every other cast; [`vector::MASK_WIDE`]; what
/// [`vector::lanes`] refuses for the narrow side; what the operand
/// reports.
pub fn cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    use Opcode as O;
    let op = inst.opcode();
    let from_ty = value_ty(cx, arg);
    let types = cx.types();
    let (from_mask, to_mask) =
        (mask_lanes(types, from_ty).is_some(), mask_lanes(types, inst.ty()).is_some());
    let from_wide = is_wide(types, from_ty);
    let wide_ty = if from_wide { from_ty } else { inst.ty() };
    let wide = lanes_of(cx, wide_ty)?;
    if wide.shape.elem != Elem::Int(128) {
        return unsupported(WIDE_CAST);
    }
    let fits = match op {
        O::ZExt | O::SExt => !from_wide,
        O::Trunc => from_wide,
        _ => false,
    };
    if !fits {
        return unsupported(WIDE_CAST);
    }
    let signed = op == O::SExt;
    if from_mask || to_mask {
        let n = mask_of(cx, if from_mask { from_ty } else { inst.ty() })?;
        deferred_first(cx, inst, None)?;
        if from_mask {
            let c = xr(cx.use_reg(arg)?);
            let out = result_image(cx, inst)?;
            for l in 0..n {
                let extract = if signed { mc::sbfx } else { mc::ubfx };
                put(cx, extract(Size::X, X16.zr(), c.zr(), l, 1));
                st(cx, X16.zr(), X17, lane(out, l), 0);
                let high = if signed { X16.zr() } else { RegZr::ZR };
                st(cx, high, X17, lane(out, l), 1);
            }
            return finish(cx, inst, out);
        }
        let [a] = images(cx, [arg])?;
        let rd = cx.def_reg(inst)?;
        let r = xr(rd);
        for l in 0..n {
            ld(cx, X16, lane(a, l), 0);
            if l == 0 {
                put(cx, mc::and_imm(Size::X, r.sp(), X16.zr(), 1));
            } else {
                put(cx, mc::bfi(Size::X, r.zr(), X16.zr(), l, 1));
            }
        }
        return cx.finish(inst, rd);
    }
    // Integer lanes on the narrow side.
    let narrow_ty = if from_wide { inst.ty() } else { from_ty };
    let narrow = vector::lanes(cx, narrow_ty)?;
    if narrow.shape.elem.is_float() || narrow.count() != wide.count() {
        return unsupported(WIDE_CAST);
    }
    let bits = narrow.shape.elem.bits();
    deferred_first(cx, inst, None)?;
    let [a] = images(cx, [arg])?;
    let out = result_image(cx, inst)?;
    for l in 0..wide.count() {
        if from_wide {
            ld(cx, X16, lane(a, l), 0);
            let mut to = Cursor::frame(cx, out.offset_by(narrow.offset(l)), X17);
            store_gpr_at(cx, narrow.gpr_store(), X16.zr(), &mut to);
        } else {
            let mut from = Cursor::frame(cx, a.offset_by(narrow.offset(l)), X16);
            load_gpr_at(cx, narrow.gpr_load(), X16, &mut from);
            if signed && bits < 64 {
                put(cx, mc::sbfx(Size::X, X16.zr(), X16.zr(), 0, bits));
            }
            st(cx, X16.zr(), X17, lane(out, l), 0);
            if signed {
                put(cx, mc::asr_imm(Size::X, X16.zr(), X16.zr(), 63));
                st(cx, X16.zr(), X17, lane(out, l), 1);
            } else {
                st(cx, RegZr::ZR, X17, lane(out, l), 1);
            }
        }
    }
    finish(cx, inst, out)
}

// ---- bfloat lanes ----

/// Whether `ty` is a fixed vector of `bfloat`.
fn is_bf(types: &TypeTable, ty: TypeId) -> bool {
    match types.kind(ty) {
        TypeKind::Vector { elem, .. } => matches!(types.kind(elem), TypeKind::BFloat),
        _ => false,
    }
}

/// The shape of vector type `ty`, at most [`MAX_LANES`] lanes.
fn shape_of(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<Shape> {
    match Shape::of(cx.types(), ty) {
        Some(shape) if shape.lanes <= MAX_LANES => Ok(shape),
        Some(_) => unsupported(vector::LENGTH),
        None => cx.bug("a lane operation on something that is no vector"),
    }
}

/// `ldr <view>` of `v` from `at` (through `x16`).
fn load_v(cx: &mut FnCx<'_, A64>, view: VMemSize, v: VReg, at: FrameRef) {
    let mut from = Cursor::frame(cx, at, X16);
    load_fpr_at(cx, view, v, &mut from);
}

/// `str <view>` of `v` to `at` (through `x17`).
fn store_v(cx: &mut FnCx<'_, A64>, view: VMemSize, v: VReg, at: FrameRef) {
    let mut to = Cursor::frame(cx, at, X17);
    store_fpr_at(cx, view, v, &mut to);
}

/// `%r = <op> <N x bfloat> %lhs, %rhs`: per lane both widened, the
/// single-precision operation (`frem`: `fmodf`), the result narrowed
/// without the NaN arm, as the scalar (`tail2::bfloat_binary`).
///
/// # Errors
/// [`vector::LENGTH`]; what the operands report.
pub fn bf_binary<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let word = match inst.opcode() {
        Opcode::FAdd => Some(mc::fadd as fn(FpSize, VReg, VReg, VReg) -> u32),
        Opcode::FSub => Some(mc::fsub as fn(FpSize, VReg, VReg, VReg) -> u32),
        Opcode::FMul => Some(mc::fmul as fn(FpSize, VReg, VReg, VReg) -> u32),
        Opcode::FDiv => Some(mc::fdiv as fn(FpSize, VReg, VReg, VReg) -> u32),
        Opcode::FRem => None,
        _ => return cx.bug("integer arithmetic on bfloat lanes"),
    };
    let n = shape_of(cx, inst.ty())?.lanes;
    deferred_first(cx, inst, None)?;
    let [a, b] = images(cx, [lhs, rhs])?;
    let r = result_image(cx, inst)?;
    for l in 0..n {
        let (p, q, o) = (a.offset_by(2 * l), b.offset_by(2 * l), r.offset_by(2 * l));
        let Some(word) = word else {
            let t1 = cx.temp(Class::Fpr)?;
            let t2 = cx.temp(Class::Fpr)?;
            load_v(cx, VMemSize::H, vr(t1), p);
            tail2::widen(cx, vr(t1));
            load_v(cx, VMemSize::H, vr(t2), q);
            tail2::widen(cx, vr(t2));
            let args = [CallArg::Reg(t1), CallArg::Reg(t2)];
            let Some(res) = cx.call_runtime(RuntimeFn::Fmodf, &args, None)? else {
                return cx.bug("a runtime function returned nothing");
            };
            tail2::narrow(cx, vr(res), false);
            store_v(cx, VMemSize::H, vr(res), o);
            cx.free_temp(res);
            continue;
        };
        load_v(cx, VMemSize::H, V30, p);
        tail2::widen(cx, V30);
        load_v(cx, VMemSize::H, V31, q);
        tail2::widen(cx, V31);
        put(cx, word(FpSize::S, V30, V30, V31));
        tail2::narrow(cx, V30, false);
        store_v(cx, VMemSize::H, V30, o);
    }
    finish(cx, inst, r)
}

/// `%r = fneg <N x bfloat> %arg`: bit 15 of each lane flipped.
///
/// # Errors
/// [`vector::LENGTH`]; what the operand reports.
pub fn bf_fneg<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let n = shape_of(cx, inst.ty())?.lanes;
    deferred_first(cx, inst, None)?;
    let [a] = images(cx, [arg])?;
    let r = result_image(cx, inst)?;
    for l in 0..n {
        let mut from = Cursor::frame(cx, a.offset_by(2 * l), X16);
        load_gpr_at(cx, Load::H, X17, &mut from);
        put(cx, mc::eor_imm(Size::W, X17.sp(), X17.zr(), 0x8000));
        let mut to = Cursor::frame(cx, r.offset_by(2 * l), X16);
        store_gpr_at(cx, MemSize::H, X17.zr(), &mut to);
    }
    finish(cx, inst, r)
}

/// `%r = fcmp <pred> <N x bfloat> %lhs, %rhs`: per lane both widened,
/// `FCMP` in single precision, the lane's bit of the mask set from the
/// predicate's conditions (`float::conditions`).
///
/// # Errors
/// [`vector::MASK_WIDE`] beyond 64 lanes; what the operands report.
pub fn bf_fcmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let n = mask_of(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    if matches!(pred, FloatPredicate::True | FloatPredicate::False) {
        let rd = cx.def_reg(inst)?;
        let bits = if pred == FloatPredicate::True { low_bits(n) } else { 0 };
        mov_imm(cx, Size::X, xr(rd), bits);
        return cx.finish(inst, rd);
    }
    let cond = float::conditions(pred);
    let [a, b] = images(cx, [lhs, rhs])?;
    let rd = cx.def_reg(inst)?;
    for l in 0..n {
        load_v(cx, VMemSize::H, V30, a.offset_by(2 * l));
        tail2::widen(cx, V30);
        load_v(cx, VMemSize::H, V31, b.offset_by(2 * l));
        tail2::widen(cx, V31);
        put(cx, mc::fcmp(FpSize::S, V30, V31));
        put(cx, mc::cset(Size::X, X17.zr(), cond.first));
        if let Some(second) = cond.second {
            put(cx, mc::csinc(Size::X, X17.zr(), X17.zr(), RegZr::ZR, second.invert()));
        }
        let acc = if l == 0 { RegZr::ZR } else { xr(rd).zr() };
        put(cx, mc::orr_shifted(Size::X, xr(rd).zr(), acc, X17.zr(), Shift::Lsl, l));
    }
    cx.finish(inst, rd)
}

/// `fpext` of `bfloat` lanes to `float` or `double` lanes (exact), and
/// `fptrunc` of those to `bfloat` lanes: from `float` rounded with the
/// NaN arm, from `double` through `FCVTXN` without it, as the scalar.
///
/// # Errors
/// [`BFLOAT_CAST`] for any other conversion; [`vector::LENGTH`].
pub fn bf_cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let from = shape_of(cx, value_ty(cx, arg))?;
    let to = shape_of(cx, inst.ty())?;
    let (other, widening) = match inst.opcode() {
        Opcode::FPExt => (to.elem, true),
        Opcode::FPTrunc => (from.elem, false),
        _ => return unsupported(BFLOAT_CAST),
    };
    let (view, bytes) = match other {
        Elem::Float => (VMemSize::S, 4),
        Elem::Double => (VMemSize::D, 8),
        _ => return unsupported(BFLOAT_CAST),
    };
    deferred_first(cx, inst, None)?;
    let [a] = images(cx, [arg])?;
    let r = result_image(cx, inst)?;
    for l in 0..to.lanes {
        if widening {
            load_v(cx, VMemSize::H, V30, a.offset_by(2 * l));
            tail2::widen(cx, V30);
            if other == Elem::Double {
                put(cx, mc::fcvt(FpFormat::D, V30, FpFormat::S, V30));
            }
            store_v(cx, view, V30, r.offset_by(bytes * l));
        } else {
            load_v(cx, view, V30, a.offset_by(bytes * l));
            if other == Elem::Double {
                put(cx, tail2::fcvtxn(V30, V30));
            }
            tail2::narrow(cx, V30, other == Elem::Float);
            store_v(cx, VMemSize::H, V30, r.offset_by(2 * l));
        }
    }
    finish(cx, inst, r)
}

// ---- the `bfloat` NEON intrinsics ----

/// `%r = call @llvm.aarch64.neon.<bf16 op>(args)`: `bfcvt` (`float` to
/// `bfloat`), `bfcvtn` (`<4 x float>` to the low half of an
/// `<8 x bfloat>`, the high half zero), `bfcvtn2` (into the high half of
/// operand 0), and the accumulating `bfdot` (`<2 x float>` or
/// `<4 x float>`), `bfmlalb`, `bfmlalt`, `bfmmla` (`<4 x float>`) into
/// operand 0's register. A signature other than LLVM's is
/// [`crate::neon::TARGET`].
///
/// # Errors
/// [`crate::neon::TARGET`]; what placing the operands reports.
pub fn bf16_intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    let types = cx.types();
    let size = |v: Value| types.size_of(value_ty(cx, v));
    let ret = types.size_of(inst.ty());
    let (word, count) = match (which, args.len()) {
        (I::Aarch64NeonBfcvt, 1) if ret == 2 && size(args[0]) == 4 => (mc::bfcvt(V30, V30), 1),
        (I::Aarch64NeonBfcvtn, 1) if ret == 16 && size(args[0]) == 16 => {
            (mc::bfcvtn(VecBits::V64, V30, V30), 1)
        }
        (I::Aarch64NeonBfcvtn2, 2) if ret == 16 => (mc::bfcvtn(VecBits::V128, V30, V30), 2),
        (I::Aarch64NeonBfdot, 3) if ret == 8 || ret == 16 => {
            let bits = if ret == 8 { VecBits::V64 } else { VecBits::V128 };
            (mc::neon_dot(Dot::Bfdot, bits, V30, V30, V30), 3)
        }
        (I::Aarch64NeonBfmlalb, 3) if ret == 16 => {
            (mc::neon_dot(Dot::Bfmlalb, VecBits::V128, V30, V30, V30), 3)
        }
        (I::Aarch64NeonBfmlalt, 3) if ret == 16 => {
            (mc::neon_dot(Dot::Bfmlalt, VecBits::V128, V30, V30, V30), 3)
        }
        (I::Aarch64NeonBfmmla, 3) if ret == 16 => {
            (mc::neon_dot(Dot::Bfmmla, VecBits::V128, V30, V30, V30), 3)
        }
        _ => return unsupported(crate::neon::TARGET),
    };
    deferred_first(cx, inst, None)?;
    // Operand 0 (the accumulator, or the converted value) dies here and
    // takes the result; operand 1 in its register; operand 2 in `v31`
    // unless it repeats an earlier one.
    let d = cx.use_reg(args[0])?;
    let mut regs = [vr(d), vr(d), vr(d)];
    if count > 1 {
        regs[1] = if args[1] == args[0] { vr(d) } else { vr(cx.use_reg(args[1])?) };
    }
    if count > 2 {
        regs[2] = match args[..2].iter().position(|&a| a == args[2]) {
            Some(j) => regs[j],
            None => {
                cx.put(args[2], crate::reg::v(31))?;
                V31
            }
        };
    }
    // The register fields of `word` (`rd` 4:0, `rn` 9:5, `rm` 20:16) hold
    // `v30`: replaced by the operands.
    let clear = word & !(0x1f | 0x1f << 5 | if count == 3 { 0x1f << 16 } else { 0 });
    let (rn, rm) = match count {
        1 => (regs[0], None),
        2 => (regs[1], None),
        _ => (regs[1], Some(regs[2])),
    };
    let rm = rm.map_or(0, |r| u32::from(r.index()) << 16);
    put(cx, clear | rm | u32::from(rn.index()) << 5 | u32::from(regs[0].index()));
    cx.finish(inst, d)
}
