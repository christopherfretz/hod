//! Vectors on i386 with SSE2 (`spec/codegen-x86.md` §17 "Vectors";
//! design 0008 I12, step C). **Owner: WP-317.** Tests:
//! `crates/hod/tests/i386_vector.rs`.
//!
//! Every vector is a value of class `Mem`: its home holds the vector's
//! memory image (lanes packed from lane 0 up; a vector of `i1` packed bit
//! by bit, lane 0 in bit 0 of the first byte, as `hod_ir`'s layout), so
//! that `load`, `store`, `bitcast` and `phi` are copies. Each operation
//! goes lane by lane, as `hod_x64`'s lane code does where it has no
//! packed instruction: a lane is loaded into `eax` / `ecx` (extended to
//! 32 bits) or `xmm6`, computed as the scalar instruction computes it,
//! and stored into the result. Nothing branches and nothing calls: no
//! construct here is an x87 barrier (`x87::barrier`).
//!
//! A result whose lanes are not read and written at the same offsets
//! (a shuffle, a cast between lane sizes, a compare, an `i1` result) is
//! built in a slot of its own and copied to its home at the end, so a
//! home shared with an operand is never read after it is written.
//!
//! The boundary (`xmm0`-`xmm2`, 16-byte stack slots) is
//! `sse::vector_place`; the glue moves are `sse::load_xmm` and
//! `sse::store_xmm`.
//!
//! Registers written: `eax`, `ecx`, `edx`, `xmm6`, the flags.

#![allow(
    clippy::many_single_char_names,
    reason = "operands `a`, `b`, result `d`, temporary `t`, shape `s`, as the spec writes them"
)]

use hod_cg::{CgResult, FnCx, FrameRef, PReg};
use hod_ir::{
    Const, FloatPredicate, Inst, InstKind, IntPredicate, Intrinsic, Opcode, SHUFFLE_UNDEF, TypeId,
    TypeKind, Value, ValueKind, X86Features,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Mem, Reg, Scale, Size, Xmm};

use crate::common::{EAX, ECX, EDX, frame_address, frame_inst, gp, put, unsupported, value_ty};
use crate::sse::FpCond;
use crate::target::X86;

/// A vector in a function without SSE2 (`pentium3`, `generic`, ...).
pub const NO_SSE2: &str = "x86:vector:no-sse2";
/// A vector of more than 16 bytes passed or returned.
pub const WIDE: &str = "x86:vector:wide";
/// A vector at the boundary that is no `xmm` vector: of `i1`, of one
/// lane, of integers of an odd width.
pub const ABI: &str = "x86:vector:abi";
/// A vector of `half`, `bfloat`, `fp128`, `x86_fp80`, of integers of a
/// width other than 1, 8, 16, 32 or 64 bits, or of pointers outside
/// address space 0.
pub const ELEMENT: &str = "x86:vector:element";
/// A scalable vector.
pub const SCALABLE: &str = "x86:vector:scalable";
/// A vector of more than [`MAX_LANES`] lanes.
pub const LENGTH: &str = "x86:vector:length";
/// An operation on `i64` lanes other than `add`, `sub`, `and`, `or`,
/// `xor`, compares, `select` and moves.
pub const I64_OP: &str = "x86:vector:i64-op";
/// `frem` of vectors (a libm call per lane).
pub const FREM: &str = "x86:vector:frem";
/// A cast of vectors from or to unsigned 32-bit or 64-bit lanes and
/// floating point, or between pointers of other widths.
pub const CAST: &str = "x86:vector:cast";
/// An intrinsic on vectors other than the integer reductions.
pub const INTRINSIC: &str = "x86:vector:intrinsic";

/// The most lanes an operation takes (its code is unrolled).
pub const MAX_LANES: u32 = 256;

/// `xmm6`.
const X: Xmm = Xmm::Xmm6;

/// A lane.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum E {
    /// `i1`, one bit.
    Mask,
    /// An integer (or pointer) of 1, 2, 4 or 8 bytes.
    Int(u32),
    /// `float`.
    F32,
    /// `double`.
    F64,
}

/// A vector type.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shape {
    pub(crate) e: E,
    pub(crate) n: u32,
    /// The store size.
    pub(crate) size: u32,
    /// The allocation size (the home's).
    pub(crate) alloc: u32,
}

impl Shape {
    /// The bytes of a lane (0 for `i1`).
    pub(crate) const fn lane(self) -> u32 {
        match self.e {
            E::Mask => 0,
            E::Int(b) => b,
            E::F32 => 4,
            E::F64 => 8,
        }
    }

    pub(crate) const fn at(self, lane: u32) -> u32 {
        lane * self.lane()
    }
}

pub(crate) fn shape(cx: &FnCx<'_, X86>, ty: TypeId) -> CgResult<Shape> {
    let types = cx.types();
    let (elem, n) = match types.kind(ty) {
        TypeKind::Vector { elem, len } => (elem, len),
        TypeKind::ScalableVector { .. } => return unsupported(SCALABLE),
        _ => return cx.bug("a vector shape of a type that is no vector"),
    };
    let e = match types.kind(elem) {
        TypeKind::Int(1) => E::Mask,
        TypeKind::Int(bits @ (8 | 16 | 32 | 64)) => E::Int(bits / 8),
        TypeKind::Ptr { addr_space: 0 } => E::Int(4),
        TypeKind::Float => E::F32,
        TypeKind::Double => E::F64,
        _ => return unsupported(ELEMENT),
    };
    if n > MAX_LANES {
        return unsupported(LENGTH);
    }
    let (Ok(size), Ok(alloc)) =
        (u32::try_from(types.size_of(ty)), u32::try_from(types.alloc_size_of(ty)))
    else {
        return unsupported(LENGTH);
    };
    Ok(Shape { e, n, size, alloc })
}

pub(crate) fn is_vec(cx: &FnCx<'_, X86>, ty: TypeId) -> bool {
    matches!(cx.types().kind(ty), TypeKind::Vector { .. } | TypeKind::ScalableVector { .. })
}

/// Whether `inst` is this module's: a vector result or operand (a call
/// only when it is an intrinsic; other calls pass vectors through
/// `abi`). The first dispatch test of `target.rs`.
pub fn owns(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    let vec = |v: Value| is_vec(cx, value_ty(cx, v));
    match inst.kind() {
        InstKind::Call(site) => {
            site.intrinsic().is_some()
                && (is_vec(cx, inst.ty()) || site.args().iter().any(|&a| vec(a)))
        }
        InstKind::Store { value, .. } => vec(value),
        InstKind::ICmp { lhs, .. } | InstKind::FCmp { lhs, .. } => vec(lhs),
        InstKind::Cast { arg } => is_vec(cx, inst.ty()) || vec(arg),
        InstKind::ExtractElement { .. }
        | InstKind::InsertElement { .. }
        | InstKind::ShuffleVector { .. } => true,
        InstKind::Binary { .. }
        | InstKind::Unary { .. }
        | InstKind::Load { .. }
        | InstKind::Select { .. } => is_vec(cx, inst.ty()),
        _ => false,
    }
}

/// Lowers an instruction [`owns`] takes.
///
/// # Errors
/// The refusals of this module; what asking for the operands reports.
pub fn lower<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
    let features = cx.target.features();
    if !features.has(X86Features::SSE2) {
        // Without any SSE: section V (`vecscalar`) takes the forms that
        // would use `xmm6` or `cmov`; the rest below uses neither.
        if features.has(X86Features::SSE) {
            return unsupported(NO_SSE2);
        }
        if let Some(done) = crate::vecscalar::lower(cx, inst) {
            return done;
        }
    }
    match inst.kind() {
        InstKind::Load { ptr, mem } => {
            if mem.ordering().is_some() {
                return unsupported(crate::mem::ATOMIC);
            }
            load(cx, inst, ptr)
        }
        InstKind::Store { value, ptr, mem } => {
            if mem.ordering().is_some() {
                return unsupported(crate::mem::ATOMIC);
            }
            store(cx, value, ptr)
        }
        InstKind::ExtractElement { vector, index } => extract(cx, inst, vector, index),
        InstKind::InsertElement { vector, value, index } => insert(cx, inst, vector, value, index),
        InstKind::ShuffleVector { first, second, mask } => shuffle(cx, inst, first, second, mask),
        InstKind::Binary { lhs, rhs } => binary(cx, inst, lhs, rhs),
        InstKind::Unary { arg } => unary(cx, inst, arg),
        InstKind::ICmp { pred, lhs, rhs } => icmp(cx, inst, pred, lhs, rhs),
        InstKind::FCmp { pred, lhs, rhs } => fcmp(cx, inst, pred, lhs, rhs),
        InstKind::Select { cond, if_true, if_false } => select(cx, inst, cond, if_true, if_false),
        InstKind::Cast { arg } => cast(cx, inst, arg),
        InstKind::Call(site) => reduce(cx, inst, site.intrinsic(), site.args()),
        _ => cx.bug("a vector instruction the vector module does not lower"),
    }
}

// ---- moves ----

fn size_of(width: u32) -> Size {
    match width {
        1 => Size::B,
        2 => Size::W,
        _ => Size::D,
    }
}

/// `r` = the `width`-byte integer at `m`, extended to 32 bits.
pub(crate) fn ld(cx: &mut FnCx<'_, X86>, r: Reg, width: u32, signed: bool, place: FrameRef) {
    frame_inst(cx, place, |m| match (width, signed) {
        (4.., _) => mc::mov_rm(Size::D, r, m),
        (_, true) => mc::movsx_rm(Size::D, r, size_of(width), m),
        (_, false) => mc::movzx_rm(Size::D, r, size_of(width), m),
    });
}

/// The low `width` bytes of `r` to `place`.
fn st(cx: &mut FnCx<'_, X86>, place: FrameRef, r: Reg, width: u32) {
    frame_inst(cx, place, |m| mc::mov_mr(size_of(width), m, r));
}

/// Copies `bytes` from `src` to `dst` through `eax` (4, 2, 1 at a
/// time); nothing when they are the same place.
pub(crate) fn copy(cx: &mut FnCx<'_, X86>, dst: FrameRef, src: FrameRef, bytes: u32) {
    if dst == src {
        return;
    }
    let mut at = 0;
    while at < bytes {
        let w = match bytes - at {
            4.. => 4,
            2 | 3 => 2,
            _ => 1,
        };
        ld(cx, EAX, w, false, src.offset_by(at));
        st(cx, dst.offset_by(at), EAX, w);
        at += w;
    }
}

/// Writes `bytes` zero bytes at `dst`.
fn zero(cx: &mut FnCx<'_, X86>, dst: FrameRef, bytes: u32) {
    put(cx, mc::xor_rr(Size::D, EAX, EAX));
    let mut at = 0;
    while at < bytes {
        let w = if bytes - at >= 4 { 4 } else { 1 };
        st(cx, dst.offset_by(at), EAX, w);
        at += w;
    }
}

/// A slot of its own for a result built lane by lane.
fn temp(cx: &mut FnCx<'_, X86>, s: Shape) -> CgResult<FrameRef> {
    cx.alloc_slot(u64::from(s.alloc.max(1)), 16)
}

/// `eax` = bit `lane` of the mask image at `image` (0 or 1).
fn bit(cx: &mut FnCx<'_, X86>, image: FrameRef, lane: u32) {
    ld(cx, EAX, 1, false, image.offset_by(lane / 8));
    if !lane.is_multiple_of(8) {
        put(cx, mc::shr_ri(Size::D, EAX, (lane % 8) as u8));
    }
    put(cx, mc::and_ri(Size::D, EAX, 1));
}

/// Ors bit 0 of `r` (the rest zero) into bit `lane` of the mask image
/// at `image`.
pub(crate) fn or_bit(cx: &mut FnCx<'_, X86>, image: FrameRef, lane: u32, r: Reg) {
    if !lane.is_multiple_of(8) {
        put(cx, mc::shl_ri(Size::D, r, (lane % 8) as u8));
    }
    frame_inst(cx, image.offset_by(lane / 8), |m| mc::or_mr(Size::B, m, r));
}

/// Lane `lane` of image `img` of shape `s` into `r` (an integer lane
/// extended by `signed`; a mask lane 0 or 1, negated when `signed`; a
/// low half of an `i64` lane).
fn lane_in(cx: &mut FnCx<'_, X86>, r: Reg, s: Shape, img: FrameRef, lane: u32, signed: bool) {
    if s.e == E::Mask {
        bit(cx, img, lane);
        if r != EAX {
            put(cx, mc::mov_rr(Size::D, r, EAX));
        }
        if signed {
            put(cx, mc::neg_r(Size::D, r));
        }
    } else {
        ld(cx, r, s.lane().min(4), signed, img.offset_by(s.at(lane)));
    }
}

/// Writes `eax` (and `edx` for the high half of an `i64` lane) as lane
/// `lane` of `img`; a mask lane takes bit 0 of `eax` (the image zeroed
/// before).
pub(crate) fn lane_out(cx: &mut FnCx<'_, X86>, s: Shape, img: FrameRef, lane: u32) {
    match s.e {
        E::Mask => {
            put(cx, mc::and_ri(Size::D, EAX, 1));
            or_bit(cx, img, lane, EAX);
        }
        E::Int(8) => {
            st(cx, img.offset_by(s.at(lane)), EAX, 4);
            st(cx, img.offset_by(s.at(lane) + 4), EDX, 4);
        }
        _ => st(cx, img.offset_by(s.at(lane)), EAX, s.lane()),
    }
}

/// Starts a result built in a slot of its own (a mask zeroed).
pub(crate) fn begin(cx: &mut FnCx<'_, X86>, s: Shape) -> CgResult<FrameRef> {
    let t = temp(cx, s)?;
    if s.e == E::Mask {
        zero(cx, t, s.alloc.max(1));
    }
    Ok(t)
}

/// Copies the result built in `t` to `inst`'s home.
pub(crate) fn end<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    t: FrameRef,
    s: Shape,
) -> CgResult<()> {
    let d = cx.def_mem(inst)?;
    copy(cx, d, t, s.alloc);
    Ok(())
}

// ---- constants ----

/// Writes the vector constant `value` to `dst` from its memory image
/// (`mov` immediates; no relocation).
///
/// # Errors
/// [`crate::common::CONSTANT`] for a constant with an address in it.
pub fn materialize_mem(cx: &mut FnCx<'_, X86>, dst: FrameRef, value: Value) -> CgResult<()> {
    let ValueKind::Const(id) = value.kind() else {
        return cx.bug("materialize_mem of something that is no constant");
    };
    let ty = cx.module.consts.ty(id);
    let s = shape(cx, ty)?;
    if let Const::Zero(_) | Const::Undef(_) | Const::Poison(_) | Const::Null(_) =
        cx.module.consts.get(id)
    {
        zero(cx, dst, s.alloc);
        return Ok(());
    }
    let (mut image, mut fixups) = (Vec::new(), Vec::new());
    if cx.module.consts.flatten(cx.types(), id, &mut image, &mut fixups).is_err()
        || !fixups.is_empty()
    {
        return unsupported(crate::common::CONSTANT);
    }
    for (k, word) in image.chunks(4).enumerate() {
        let mut bytes = [0u8; 4];
        bytes[..word.len()].copy_from_slice(word);
        let at = dst.offset_by(4 * k as u32);
        if word.len() == 4 {
            let v = i64::from(i32::from_le_bytes(bytes));
            frame_inst(cx, at, |m| mc::mov_mi(Size::D, m, v));
        } else {
            for (j, &b) in word.iter().enumerate() {
                let place = at.offset_by(j as u32);
                frame_inst(cx, place, |m| mc::mov_mi(Size::B, m, i64::from(b as i8)));
            }
        }
    }
    Ok(())
}

// ---- memory ----

fn load<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, ptr: Value) -> CgResult<()> {
    let s = shape(cx, inst.ty())?;
    let p = gp(cx.use_reg(ptr)?);
    let d = cx.def_mem(inst)?;
    crate::common::copy_from_ptr(cx, d, p, 0, s.size);
    Ok(())
}

fn store(cx: &mut FnCx<'_, X86>, value: Value, ptr: Value) -> CgResult<()> {
    let s = shape(cx, value_ty(cx, value))?;
    let src = cx.use_mem(value)?;
    let p = gp(cx.use_reg(ptr)?);
    let mut at = 0;
    while at < s.size {
        let w = match s.size - at {
            4.. => 4,
            2 | 3 => 2,
            _ => 1,
        };
        ld(cx, EAX, w, false, src.offset_by(at));
        put(cx, mc::mov_mr(size_of(w), Mem::base(p, at as i32), EAX));
        at += w;
    }
    Ok(())
}

// ---- lanes by index ----

/// Asks for index `v` of a vector of `n` lanes: `Ok` a constant lane (a
/// constant out of range, `undef` or `poison`: lane 0, the result is
/// poison), `Err` a variable index (its register, if it is of class
/// `Gpr`, else its low word already in `ecx`, and its width), which
/// [`place_index`] reduces (masked with `n - 1` for a power of two `n`,
/// else 0 when `≥ n`) and puts in `ecx`, with the image's address in
/// `edx`.
/// variable one in `ecx`.
pub(crate) fn ask_index(
    cx: &mut FnCx<'_, X86>,
    v: Value,
    n: u32,
) -> CgResult<Result<u32, (Option<PReg>, u32)>> {
    if let ValueKind::Const(id) = v.kind() {
        let lane = match cx.module.consts.get(id) {
            Const::Int { value, .. } if value < u64::from(n) => value as u32,
            _ => 0,
        };
        return Ok(Ok(lane));
    }
    let ty = value_ty(cx, v);
    let bits = cx.types().int_bits(ty).unwrap_or(32);
    if crate::abi::is_gpr(cx.types(), ty) {
        Ok(Err((Some(cx.use_reg(v)?), bits)))
    } else {
        // An index wider than 32 bits: its low word (the reduction below
        // keeps every access inside the vector).
        let home = cx.use_mem(v)?;
        ld(cx, ECX, 4, false, home);
        Ok(Err((None, 32)))
    }
}

/// `ecx` = the asked index reduced to `n` lanes; `edx` = `&image`.
pub(crate) fn place_index(
    cx: &mut FnCx<'_, X86>,
    asked: (Option<PReg>, u32),
    n: u32,
    image: FrameRef,
) {
    let (reg, bits) = asked;
    if let Some(r) = reg {
        put(cx, mc::mov_rr(Size::D, ECX, gp(r)));
    }
    if bits < 32 {
        put(cx, mc::and_ri(Size::D, ECX, i64::from(((1u64 << bits) - 1) as u32 as i32)));
    }
    if n.is_power_of_two() {
        put(cx, mc::and_ri(Size::D, ECX, i64::from(n - 1)));
    } else {
        // `ecx` < n: `eax` = -1 (CF), else 0.
        put(cx, mc::cmp_ri(Size::D, ECX, i64::from(n)));
        put(cx, mc::sbb_rr(Size::D, EAX, EAX));
        put(cx, mc::and_rr(Size::D, ECX, EAX));
    }
    frame_address(cx, EDX, image);
}

fn scale(width: u32) -> Scale {
    match width {
        1 => Scale::S1,
        2 => Scale::S2,
        4 => Scale::S4,
        _ => Scale::S8,
    }
}

/// `[edx + ecx * lane + disp]`.
pub(crate) fn var_mem(s: Shape, disp: i32) -> Mem {
    match ECX.index() {
        Some(ix) => Mem::base_index(EDX, ix, scale(s.lane()), disp),
        None => Mem::base(EDX, disp),
    }
}

/// `eax` = bit `ecx` of the mask at `[edx]`; `ecx` is consumed.
fn var_bit(cx: &mut FnCx<'_, X86>) {
    put(cx, mc::mov_rr(Size::D, EAX, ECX));
    put(cx, mc::shr_ri(Size::D, EAX, 3));
    put(cx, mc::add_rr(Size::D, EDX, EAX));
    put(cx, mc::and_ri(Size::D, ECX, 7));
    put(cx, mc::movzx_rm(Size::D, EAX, Size::B, Mem::base(EDX, 0)));
    put(cx, mc::shr_rcl(Size::D, EAX));
    put(cx, mc::and_ri(Size::D, EAX, 1));
}

fn extract<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    vector: Value,
    index: Value,
) -> CgResult<()> {
    let s = shape(cx, value_ty(cx, vector))?;
    let img = cx.use_mem(vector)?;
    let asked = ask_index(cx, index, s.n)?;
    let gpr = crate::abi::is_gpr(cx.types(), inst.ty());
    match asked {
        Ok(lane) if s.e == E::Mask => bit(cx, img, lane),
        Ok(lane) if gpr => ld(cx, EAX, s.lane(), false, img.offset_by(s.at(lane))),
        Ok(lane) => {
            let d = cx.def_mem(inst)?;
            copy(cx, d, img.offset_by(s.at(lane)), s.lane());
            return Ok(());
        }
        Err(asked) => {
            place_index(cx, asked, s.n, img);
            if s.e == E::Mask {
                var_bit(cx);
            } else if gpr {
                let m = var_mem(s, 0);
                put(
                    cx,
                    match s.lane() {
                        4 => mc::mov_rm(Size::D, EAX, m),
                        w => mc::movzx_rm(Size::D, EAX, size_of(w), m),
                    },
                );
            } else {
                put(cx, mc::lea(Size::D, EDX, var_mem(s, 0)));
                let d = cx.def_mem(inst)?;
                for k in (0..s.lane()).step_by(4) {
                    put(cx, mc::mov_rm(Size::D, EAX, Mem::base(EDX, k as i32)));
                    st(cx, d.offset_by(k), EAX, 4);
                }
                return Ok(());
            }
        }
    }
    let rd = cx.def_reg(inst)?;
    put(cx, mc::mov_rr(Size::D, gp(rd), EAX));
    cx.finish(inst, rd)
}

fn insert<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    vector: Value,
    value: Value,
    index: Value,
) -> CgResult<()> {
    let s = shape(cx, inst.ty())?;
    let img = cx.use_mem(vector)?;
    let gpr = crate::abi::is_gpr(cx.types(), value_ty(cx, value));
    let (vreg, vmem) =
        if gpr { (Some(cx.use_reg(value)?), None) } else { (None, Some(cx.use_mem(value)?)) };
    let asked = ask_index(cx, index, s.n)?;
    // The new lane in `xmm6` (8 bytes at most), before the copy.
    if let Some(m) = vmem {
        frame_inst(cx, m, |m| {
            if s.lane() == 8 { mc::movs_rm(FpSize::D, X, m) } else { mc::movd_rm(Size::D, X, m) }
        });
    }
    let d = cx.def_mem(inst)?;
    copy(cx, d, img, s.alloc);
    let lane_from_x = |m: Mem| {
        if s.lane() == 8 { mc::movs_mr(FpSize::D, m, X) } else { mc::movd_mr(Size::D, m, X) }
    };
    match asked {
        Ok(lane) => {
            if s.e == E::Mask {
                let r = gp(vreg.unwrap_or(PReg::new(0)));
                put(cx, mc::mov_rr(Size::D, EAX, r));
                put(cx, mc::and_ri(Size::D, EAX, 1));
                let byte = d.offset_by(lane / 8);
                let clear = !(1u8 << (lane % 8)) as i8;
                frame_inst(cx, byte, |m| mc::and_mi(Size::B, m, i64::from(clear)));
                or_bit(cx, d, lane, EAX);
            } else if let Some(r) = vreg {
                put(cx, mc::mov_rr(Size::D, EAX, gp(r)));
                st(cx, d.offset_by(s.at(lane)), EAX, s.lane());
            } else {
                frame_inst(cx, d.offset_by(s.at(lane)), lane_from_x);
            }
        }
        Err(asked) => {
            place_index(cx, asked, s.n, d);
            if s.e == E::Mask {
                // byte = [edx + ecx/8]; bit = ecx % 8.
                let r = gp(vreg.unwrap_or(PReg::new(0)));
                put(cx, mc::movd_xr(Size::D, X, r));
                put(cx, mc::mov_rr(Size::D, EAX, ECX));
                put(cx, mc::shr_ri(Size::D, EAX, 3));
                put(cx, mc::add_rr(Size::D, EDX, EAX));
                put(cx, mc::and_ri(Size::D, ECX, 7));
                put(cx, mc::mov_ri(Size::D, EAX, 1));
                put(cx, mc::shl_rcl(Size::D, EAX));
                put(cx, mc::not_r(Size::D, EAX));
                put(cx, mc::and_mr(Size::B, Mem::base(EDX, 0), EAX));
                put(cx, mc::movd_rx(Size::D, EAX, X));
                put(cx, mc::and_ri(Size::D, EAX, 1));
                put(cx, mc::shl_rcl(Size::D, EAX));
                put(cx, mc::or_mr(Size::B, Mem::base(EDX, 0), EAX));
            } else if let Some(r) = vreg {
                put(cx, mc::mov_rr(Size::D, EAX, gp(r)));
                put(cx, mc::mov_mr(size_of(s.lane()), var_mem(s, 0), EAX));
            } else {
                put(cx, lane_from_x(var_mem(s, 0)));
            }
        }
    }
    Ok(())
}

fn shuffle<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    first: Value,
    second: Value,
    mask: &[u32],
) -> CgResult<()> {
    let s = shape(cx, inst.ty())?;
    let a = shape(cx, value_ty(cx, first))?;
    let ia = cx.use_mem(first)?;
    let ib = cx.use_mem(second)?;
    let t = begin(cx, s)?;
    for (lane, &m) in (0..).zip(mask) {
        if m == SHUFFLE_UNDEF {
            continue;
        }
        let (img, j) = if m < a.n { (ia, m) } else { (ib, m - a.n) };
        if s.e == E::Mask {
            bit(cx, img, j);
            or_bit(cx, t, lane, EAX);
        } else {
            copy(cx, t.offset_by(s.at(lane)), img.offset_by(a.at(j)), s.lane());
        }
    }
    end(cx, inst, t, s)
}

// ---- element-wise ----

/// An ALU instruction with a memory source.
type MemOp = fn(Size, Reg, Mem) -> mc::Inst;

#[allow(clippy::too_many_lines, reason = "one match over the lane kinds")]
fn binary<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    use Opcode as O;
    let s = shape(cx, inst.ty())?;
    let op = inst.opcode();
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let d = cx.def_mem(inst)?;
    match s.e {
        E::Mask => {
            // Bit by bit over the bytes: `add`/`sub` are `xor`, `mul` is
            // `and`; a division or a shift of `i1` is its first operand
            // (by 1) or poison.
            let f: fn(Size, Reg, Reg) -> mc::Inst = match op {
                O::And | O::Mul | O::UDiv | O::SDiv => mc::and_rr,
                O::Or => mc::or_rr,
                O::Xor | O::Add | O::Sub => mc::xor_rr,
                O::URem | O::SRem => {
                    zero(cx, d, s.alloc);
                    return Ok(());
                }
                _ => {
                    copy(cx, d, a, s.alloc);
                    return Ok(());
                }
            };
            for k in 0..s.alloc {
                ld(cx, EAX, 1, false, a.offset_by(k));
                ld(cx, ECX, 1, false, b.offset_by(k));
                put(cx, f(Size::D, EAX, ECX));
                st(cx, d.offset_by(k), EAX, 1);
            }
            // The bits above the last lane stay zero.
            if s.n % 8 != 0 && matches!(op, O::Xor | O::Add | O::Sub | O::Or) {
                let mask = i64::from((1u32 << (s.n % 8)) - 1);
                frame_inst(cx, d.offset_by(s.n / 8), |m| mc::and_mi(Size::B, m, mask));
            }
        }
        E::F32 | E::F64 => {
            let fp = if s.e == E::F32 { FpSize::S } else { FpSize::D };
            let f: fn(FpSize, Xmm, Mem) -> mc::Inst = match op {
                O::FAdd => mc::adds_rm,
                O::FSub => mc::subs_rm,
                O::FMul => mc::muls_rm,
                O::FDiv => mc::divs_rm,
                O::FRem => return unsupported(FREM),
                _ => return cx.bug("an integer opcode on floating-point lanes"),
            };
            for lane in 0..s.n {
                let at = s.at(lane);
                frame_inst(cx, a.offset_by(at), |m| mc::movs_rm(fp, X, m));
                frame_inst(cx, b.offset_by(at), |m| f(fp, X, m));
                frame_inst(cx, d.offset_by(at), |m| mc::movs_mr(fp, m, X));
            }
        }
        E::Int(8) => {
            let (lo, hi): (MemOp, MemOp) = match op {
                O::Add => (mc::add_rm, mc::adc_rm),
                O::Sub => (mc::sub_rm, mc::sbb_rm),
                O::And => (mc::and_rm, mc::and_rm),
                O::Or => (mc::or_rm, mc::or_rm),
                O::Xor => (mc::xor_rm, mc::xor_rm),
                _ => return unsupported(I64_OP),
            };
            for lane in 0..s.n {
                let at = s.at(lane);
                ld(cx, EAX, 4, false, a.offset_by(at));
                ld(cx, EDX, 4, false, a.offset_by(at + 4));
                frame_inst(cx, b.offset_by(at), |m| lo(Size::D, EAX, m));
                frame_inst(cx, b.offset_by(at + 4), |m| hi(Size::D, EDX, m));
                lane_out(cx, s, d, lane);
            }
        }
        E::Int(w) => {
            let signed = matches!(op, O::SDiv | O::SRem | O::AShr);
            for lane in 0..s.n {
                let at = s.at(lane);
                ld(cx, EAX, w, signed, a.offset_by(at));
                ld(cx, ECX, w, signed, b.offset_by(at));
                match op {
                    O::Add => put(cx, mc::add_rr(Size::D, EAX, ECX)),
                    O::Sub => put(cx, mc::sub_rr(Size::D, EAX, ECX)),
                    O::Mul => put(cx, mc::imul_rr(Size::D, EAX, ECX)),
                    O::And => put(cx, mc::and_rr(Size::D, EAX, ECX)),
                    O::Or => put(cx, mc::or_rr(Size::D, EAX, ECX)),
                    O::Xor => put(cx, mc::xor_rr(Size::D, EAX, ECX)),
                    O::Shl => put(cx, mc::shl_rcl(Size::D, EAX)),
                    O::LShr => put(cx, mc::shr_rcl(Size::D, EAX)),
                    O::AShr => put(cx, mc::sar_rcl(Size::D, EAX)),
                    O::UDiv | O::URem | O::SDiv | O::SRem => {
                        if signed {
                            put(cx, mc::cdq());
                            put(cx, mc::idiv_r(Size::D, ECX));
                        } else {
                            put(cx, mc::xor_rr(Size::D, EDX, EDX));
                            put(cx, mc::div_r(Size::D, ECX));
                        }
                        if matches!(op, O::URem | O::SRem) {
                            put(cx, mc::mov_rr(Size::D, EAX, EDX));
                        }
                    }
                    _ => return cx.bug("a floating-point opcode on integer lanes"),
                }
                st(cx, d.offset_by(at), EAX, w);
            }
        }
    }
    Ok(())
}

/// `fneg` (the sign bit of each lane flipped) and `freeze` (a copy).
fn unary<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let s = shape(cx, inst.ty())?;
    let a = cx.use_mem(arg)?;
    let d = cx.def_mem(inst)?;
    copy(cx, d, a, s.alloc);
    if inst.opcode() == Opcode::FNeg {
        for lane in 0..s.n {
            let top = d.offset_by(s.at(lane) + s.lane() - 1);
            frame_inst(cx, top, |m| mc::xor_mi(Size::B, m, -0x80));
        }
    }
    Ok(())
}

// ---- compares and select ----

const fn signed_pred(pred: IntPredicate) -> bool {
    matches!(pred, IntPredicate::Sgt | IntPredicate::Sge | IntPredicate::Slt | IntPredicate::Sle)
}

fn icmp<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    use IntPredicate as P;
    let s = shape(cx, value_ty(cx, lhs))?;
    let r = shape(cx, inst.ty())?;
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let t = begin(cx, r)?;
    let signed = signed_pred(pred);
    for lane in 0..s.n {
        match s.e {
            E::Int(8) => {
                let at = s.at(lane);
                if matches!(pred, P::Eq | P::Ne) {
                    ld(cx, EAX, 4, false, a.offset_by(at));
                    ld(cx, ECX, 4, false, a.offset_by(at + 4));
                    frame_inst(cx, b.offset_by(at), |m| mc::xor_rm(Size::D, EAX, m));
                    frame_inst(cx, b.offset_by(at + 4), |m| mc::xor_rm(Size::D, ECX, m));
                    put(cx, mc::or_rr(Size::D, EAX, ECX));
                    put(cx, mc::setcc_r(if pred == P::Eq { Cond::E } else { Cond::Ne }, EAX));
                } else {
                    // `x < y` in the flags of `cmp`/`sbb` over the halves.
                    let (x, y, c) = match pred {
                        P::Ult => (a, b, Cond::B),
                        P::Uge => (a, b, Cond::Ae),
                        P::Ugt => (b, a, Cond::B),
                        P::Ule => (b, a, Cond::Ae),
                        P::Slt => (a, b, Cond::L),
                        P::Sge => (a, b, Cond::Ge),
                        P::Sgt => (b, a, Cond::L),
                        _ => (b, a, Cond::Ge),
                    };
                    ld(cx, EAX, 4, false, x.offset_by(at));
                    ld(cx, ECX, 4, false, x.offset_by(at + 4));
                    frame_inst(cx, y.offset_by(at), |m| mc::cmp_rm(Size::D, EAX, m));
                    frame_inst(cx, y.offset_by(at + 4), |m| mc::sbb_rm(Size::D, ECX, m));
                    put(cx, mc::setcc_r(c, EAX));
                }
            }
            E::Mask => {
                lane_in(cx, ECX, s, b, lane, signed);
                lane_in(cx, EDX, s, a, lane, signed);
                put(cx, mc::cmp_rr(Size::D, EDX, ECX));
                put(cx, mc::setcc_r(crate::alu::cond_of(pred), EAX));
            }
            _ => {
                lane_in(cx, EAX, s, a, lane, signed);
                lane_in(cx, ECX, s, b, lane, signed);
                put(cx, mc::cmp_rr(Size::D, EAX, ECX));
                put(cx, mc::setcc_r(crate::alu::cond_of(pred), EAX));
            }
        }
        put(cx, mc::movzx_rr(Size::D, EAX, Size::B, EAX));
        lane_out(cx, r, t, lane);
    }
    end(cx, inst, t, r)
}

fn fcmp<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let s = shape(cx, value_ty(cx, lhs))?;
    let r = shape(cx, inst.ty())?;
    let fp = match s.e {
        E::F32 => FpSize::S,
        E::F64 => FpSize::D,
        _ => return cx.bug("an fcmp of vectors that are no floating-point vectors"),
    };
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let t = begin(cx, r)?;
    let (swap, cond) = crate::sse::conditions(pred);
    let (x, y) = if swap { (b, a) } else { (a, b) };
    for lane in 0..s.n {
        let at = s.at(lane);
        match pred {
            FloatPredicate::True => put(cx, mc::mov_ri(Size::D, EAX, 1)),
            FloatPredicate::False => put(cx, mc::xor_rr(Size::D, EAX, EAX)),
            _ => {
                frame_inst(cx, x.offset_by(at), |m| mc::movs_rm(fp, X, m));
                frame_inst(cx, y.offset_by(at), |m| mc::ucomis_rm(fp, X, m));
                match cond {
                    FpCond::One(c) => put(cx, mc::setcc_r(c, EAX)),
                    FpCond::All(c1, c2) | FpCond::Any(c1, c2) => {
                        put(cx, mc::setcc_r(c1, EAX));
                        put(cx, mc::setcc_r(c2, ECX));
                        let all = matches!(cond, FpCond::All(..));
                        put(
                            cx,
                            if all {
                                mc::and_rr(Size::B, EAX, ECX)
                            } else {
                                mc::or_rr(Size::B, EAX, ECX)
                            },
                        );
                    }
                }
                put(cx, mc::movzx_rr(Size::D, EAX, Size::B, EAX));
            }
        }
        lane_out(cx, r, t, lane);
    }
    end(cx, inst, t, r)
}

/// `d = f ^ ((t ^ f) & eax)` over `bytes` bytes at `off` (`eax` all
/// ones or zero; 4, 2, 1 bytes at a time).
fn pick(cx: &mut FnCx<'_, X86>, d: FrameRef, t: FrameRef, f: FrameRef, off: u32, bytes: u32) {
    let mut at = off;
    while at < off + bytes {
        let w = match off + bytes - at {
            4.. => 4,
            2 | 3 => 2,
            _ => 1,
        };
        ld(cx, ECX, w, false, t.offset_by(at));
        ld(cx, EDX, w, false, f.offset_by(at));
        put(cx, mc::xor_rr(Size::D, ECX, EDX));
        put(cx, mc::and_rr(Size::D, ECX, EAX));
        put(cx, mc::xor_rr(Size::D, ECX, EDX));
        st(cx, d.offset_by(at), ECX, w);
        at += w;
    }
}

/// `select` without a branch: a scalar condition picks every byte; a
/// mask picks each lane (a mask result: each bit).
fn select<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let s = shape(cx, inst.ty())?;
    let t = cx.use_mem(if_true)?;
    let f = cx.use_mem(if_false)?;
    if is_vec(cx, value_ty(cx, cond)) {
        let c = cx.use_mem(cond)?;
        let d = cx.def_mem(inst)?;
        if s.e == E::Mask {
            for k in 0..s.alloc {
                ld(cx, EAX, 1, false, c.offset_by(k));
                pick(cx, d, t, f, k, 1);
            }
            return Ok(());
        }
        for lane in 0..s.n {
            bit(cx, c, lane);
            put(cx, mc::neg_r(Size::D, EAX));
            pick(cx, d, t, f, s.at(lane), s.lane());
        }
        return Ok(());
    }
    let r = cx.use_reg(cond)?;
    let d = cx.def_mem(inst)?;
    put(cx, mc::mov_rr(Size::D, EAX, gp(r)));
    put(cx, mc::and_ri(Size::D, EAX, 1));
    put(cx, mc::neg_r(Size::D, EAX));
    pick(cx, d, t, f, 0, s.alloc);
    Ok(())
}

// ---- casts ----

fn cast<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    use Opcode as O;
    let (from, to) = (value_ty(cx, arg), inst.ty());
    let op = inst.opcode();
    if op == O::BitCast || (op == O::AddrSpaceCast) {
        return bitcast(cx, inst, arg);
    }
    let s = shape(cx, from)?;
    let r = shape(cx, to)?;
    let a = cx.use_mem(arg)?;
    let t = begin(cx, r)?;
    let int_like = |e: E| matches!(e, E::Mask | E::Int(_));
    for lane in 0..s.n {
        let at = s.at(lane);
        match op {
            O::Trunc | O::ZExt | O::SExt | O::PtrToInt | O::IntToPtr
                if int_like(s.e) && int_like(r.e) =>
            {
                let signed = op == O::SExt;
                lane_in(cx, EAX, s, a, lane, signed);
                if r.e == E::Int(8) {
                    if s.e == E::Int(8) {
                        ld(cx, EDX, 4, false, a.offset_by(at + 4));
                    } else if signed {
                        put(cx, mc::cdq());
                    } else {
                        put(cx, mc::xor_rr(Size::D, EDX, EDX));
                    }
                }
            }
            O::FPExt if s.e == E::F32 && r.e == E::F64 => {
                frame_inst(cx, a.offset_by(at), |m| mc::cvtss2sd_rm(X, m));
                frame_inst(cx, t.offset_by(r.at(lane)), |m| mc::movs_mr(FpSize::D, m, X));
                continue;
            }
            O::FPTrunc if s.e == E::F64 && r.e == E::F32 => {
                frame_inst(cx, a.offset_by(at), |m| mc::cvtsd2ss_rm(X, m));
                frame_inst(cx, t.offset_by(r.at(lane)), |m| mc::movs_mr(FpSize::S, m, X));
                continue;
            }
            O::SIToFP | O::UIToFP if matches!(r.e, E::F32 | E::F64) => {
                let signed = op == O::SIToFP;
                let ok = match s.e {
                    E::Mask => true,
                    E::Int(w) => w < 4 || (w == 4 && signed),
                    _ => false,
                };
                if !ok {
                    return unsupported(CAST);
                }
                lane_in(cx, EAX, s, a, lane, signed);
                let fp = if r.e == E::F32 { FpSize::S } else { FpSize::D };
                put(cx, mc::cvtsi2s_rr(fp, X, Size::D, EAX));
                frame_inst(cx, t.offset_by(r.at(lane)), |m| mc::movs_mr(fp, m, X));
                continue;
            }
            O::FPToSI | O::FPToUI if matches!(s.e, E::F32 | E::F64) => {
                let ok = match r.e {
                    E::Mask => true,
                    E::Int(w) => w < 4 || (w == 4 && op == O::FPToSI),
                    _ => false,
                };
                if !ok {
                    return unsupported(CAST);
                }
                let fp = if s.e == E::F32 { FpSize::S } else { FpSize::D };
                frame_inst(cx, a.offset_by(at), |m| mc::cvtts2si_rm(Size::D, EAX, fp, m));
            }
            _ => return unsupported(CAST),
        }
        lane_out(cx, r, t, lane);
    }
    end(cx, inst, t, r)
}

/// `bitcast`: the same bytes (a scalar of class `Gpr` from or into a
/// register).
fn bitcast<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let (from, to) = (value_ty(cx, arg), inst.ty());
    let types = cx.types();
    let bytes = types.alloc_size_of(from).min(types.alloc_size_of(to)) as u32;
    let size = types.size_of(to) as u32;
    if crate::abi::is_gpr(types, to) {
        let a = cx.use_mem(arg)?;
        let rd = cx.def_reg(inst)?;
        ld(cx, EAX, size.min(4), false, a);
        put(cx, mc::mov_rr(Size::D, gp(rd), EAX));
        return cx.finish(inst, rd);
    }
    if crate::abi::is_gpr(types, from) {
        let r = cx.use_reg(arg)?;
        let d = cx.def_mem(inst)?;
        put(cx, mc::mov_rr(Size::D, EAX, gp(r)));
        st(cx, d, EAX, size.min(4));
        return Ok(());
    }
    if is_vec(cx, to) {
        shape(cx, to)?;
    }
    if is_vec(cx, from) {
        shape(cx, from)?;
    }
    let a = cx.use_mem(arg)?;
    let d = cx.def_mem(inst)?;
    copy(cx, d, a, bytes);
    Ok(())
}

// ---- reductions ----

/// `llvm.vector.reduce.{add,mul,and,or,xor,smax,smin,umax,umin}` of
/// lanes of 1 to 4 bytes (`cmov` for the extremes: every SSE2 CPU has
/// it) and of at most 32 `i1`.
fn reduce<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Option<Intrinsic<'_>>,
    args: &[Value],
) -> CgResult<()> {
    use Intrinsic as I;
    let Some(which) = which else { return unsupported(INTRINSIC) };
    let is_reduce = matches!(
        which,
        I::VectorReduceAdd
            | I::VectorReduceMul
            | I::VectorReduceAnd
            | I::VectorReduceOr
            | I::VectorReduceXor
            | I::VectorReduceSmax
            | I::VectorReduceSmin
            | I::VectorReduceUmax
            | I::VectorReduceUmin
    );
    let [arg] = args else { return unsupported(INTRINSIC) };
    if !is_reduce || !crate::abi::is_gpr(cx.types(), inst.ty()) {
        return unsupported(INTRINSIC);
    }
    let s = shape(cx, value_ty(cx, *arg))?;
    let img = cx.use_mem(*arg)?;
    match s.e {
        E::Mask if s.n <= 32 => {
            ld(cx, EAX, s.alloc.min(4), false, img);
            let low = if s.n == 32 { -1 } else { i64::from(((1u64 << s.n) - 1) as u32 as i32) };
            put(cx, mc::and_ri(Size::D, EAX, low));
            // In `i1`, `add` is `xor`; `mul`, `umin`, `smax` are `and`;
            // `umax`, `smin` are `or`.
            match which {
                I::VectorReduceOr | I::VectorReduceUmax | I::VectorReduceSmin => {
                    put(cx, mc::test_rr(Size::D, EAX, EAX));
                    put(cx, mc::setcc_r(Cond::Ne, EAX));
                }
                I::VectorReduceXor | I::VectorReduceAdd => {
                    put(cx, mc::mov_rr(Size::D, ECX, EAX));
                    put(cx, mc::shr_ri(Size::D, ECX, 16));
                    put(cx, mc::xor_rr(Size::D, EAX, ECX));
                    put(cx, mc::mov_rr(Size::D, ECX, EAX));
                    put(cx, mc::shr_ri(Size::D, ECX, 8));
                    put(cx, mc::xor_rr(Size::D, EAX, ECX));
                    put(cx, mc::test_rr(Size::B, EAX, EAX));
                    put(cx, mc::setcc_r(Cond::Np, EAX));
                }
                _ => {
                    put(cx, mc::cmp_ri(Size::D, EAX, low));
                    put(cx, mc::setcc_r(Cond::E, EAX));
                }
            }
            put(cx, mc::movzx_rr(Size::D, EAX, Size::B, EAX));
        }
        E::Int(w) if w <= 4 => {
            let signed = matches!(which, I::VectorReduceSmax | I::VectorReduceSmin);
            lane_in(cx, EAX, s, img, 0, signed);
            for lane in 1..s.n {
                lane_in(cx, ECX, s, img, lane, signed);
                let keep = match which {
                    I::VectorReduceAdd => Some(mc::add_rr(Size::D, EAX, ECX)),
                    I::VectorReduceMul => Some(mc::imul_rr(Size::D, EAX, ECX)),
                    I::VectorReduceAnd => Some(mc::and_rr(Size::D, EAX, ECX)),
                    I::VectorReduceOr => Some(mc::or_rr(Size::D, EAX, ECX)),
                    I::VectorReduceXor => Some(mc::xor_rr(Size::D, EAX, ECX)),
                    _ => None,
                };
                if let Some(i) = keep {
                    put(cx, i);
                    continue;
                }
                // `eax` takes `ecx` when `ecx` is the new extreme.
                let c = match which {
                    I::VectorReduceSmax => Cond::L,
                    I::VectorReduceSmin => Cond::G,
                    I::VectorReduceUmax => Cond::B,
                    _ => Cond::A,
                };
                put(cx, mc::cmp_rr(Size::D, EAX, ECX));
                put(cx, mc::cmovcc_rr(c, Size::D, EAX, ECX));
            }
        }
        E::Int(_) => return unsupported(I64_OP),
        _ => return unsupported(INTRINSIC),
    }
    let rd = cx.def_reg(inst)?;
    put(cx, mc::mov_rr(Size::D, gp(rd), EAX));
    cx.finish(inst, rd)
}
