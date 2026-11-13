//! Vectors as values on x86-64 (`spec/codegen.md` §5.6.13, "Vectors
//! (WP-132)"): the helpers every vector lowering of this target builds
//! on, and `extractelement`, `insertelement`, `shufflevector`.
//!
//! **Owner: WP-132.** Tests: `tests/vector.rs`. The lane loop
//! (`lanes.rs`), masks (`mask.rs`) and vectors across calls (`vcall.rs`)
//! use the helpers below.
//!
//! # Shapes
//!
//! [`Shape::of`] is what a fixed vector type is (element, lanes, class,
//! sizes, as `hod_ir`'s layout gives them); [`Shape::lanes`] the same as
//! [`Lanes`], a vector whose lanes are addressable bytes (1, 2, 4 or 8
//! each, at most [`MAX_LANES`]). Every other shape is refused there by
//! the generic names AArch64 uses ([`MASK_WIDE`], [`LENGTH`],
//! [`WIDE_ELEMENT`], [`ODD_ELEMENT`]); vectors of `i1` of at most 64
//! lanes are masks (`mask.rs`), which the dispatch sends there first.
//!
//! # Images
//!
//! A lane is reached in the vector's **image**: its own slot for a `Mem`
//! vector, or, for an `Fpr` vector (8 or 16 bytes in an `xmm`), a
//! 16-byte frame slot the register is stored into ([`spill`]). The
//! target keeps no per-function state for vectors, so each image is a
//! slot of its own ([`scratch`], allocated when asked for, never reused);
//! a lowering asks for its operands' images once, after
//! `deferred_first`.
//!
//! # Lane indices out of range
//!
//! As on AArch64 (§5.5.1): a constant index `≥ N`, `undef` or `poison`
//! is [`Index::Poison`] (an extract reads lane 0, an insert writes
//! nothing); a variable index is reduced to its width, then masked with
//! `N - 1` for a power-of-two `N`, else clamped to lane 0 when `≥ N`
//! ([`lane_number`]). An index wider than 64 bits is [`INDEX`].
//!
//! # Registers
//!
//! The helpers write `rax`, `rcx`, `rdx`, `r11` and `xmm15` (each says
//! which), never the flags across a reader (no compare is live in a
//! vector lowering).

use hod_cg::{CgResult, Class, FnCx, FrameRef, PReg, Target};
use hod_ir::{Const, Inst, SHUFFLE_UNDEF, TypeId, TypeKind, TypeTable, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Mem, Reg, Scale, Size, Xmm};

use crate::common::{
    R11, XMM15, copy_frame, deferred_first, frame_address, frame_inst, gp, gpr_load, load_fpr, put,
    store_fpr, unsupported, value_ty, xm,
};
use crate::reg;
use crate::target::X64;

pub use hod_cg::lanes::{Elem, MAX_LANES};

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;

// ---- refusals ----

/// An operation on a vector of class `Fpr` that no lowering of this
/// target takes (an `fcmp` of vectors whose result is no mask, a vector
/// condition of more than 64 lanes).
pub const VECTOR: &str = "x64:vector-op";
/// A lane operation on a vector of more than 64 `i1`.
pub const MASK_WIDE: &str = "x64:mask-wide";
/// A lane operation on a vector of integers of a width other than 1, 8,
/// 16, 32 or 64 bits (`<4 x i4>`, `<2 x i24>`).
pub const ODD_ELEMENT: &str = "x64:vector-odd-element";
/// A lane operation on a vector of `i128` (or wider), `fp128` or
/// `x86_fp80` other than `extractelement`, `insertelement` and
/// `shufflevector` (WP-152, [`Wide`]).
pub const WIDE_ELEMENT: &str = "x64:vector-wide-element";
/// A lane operation on a vector of more than [`MAX_LANES`] lanes.
pub const LENGTH: &str = "x64:vector-length";
/// A lane index wider than 64 bits.
pub const INDEX: &str = "x64:vector-index";
/// A conversion of a vector of 64-bit integers to `bfloat` (the scalar
/// rounding by hand is not repeated per lane).
pub const BFLOAT: &str = "x64:vector-bfloat";
/// `getelementptr` over a vector of pointers.
pub use crate::mem::PTRADD_VECTOR;

/// Every stable name this module and the lane loop refuse with.
pub const UNSUPPORTED: &[&str] =
    &[VECTOR, MASK_WIDE, ODD_ELEMENT, WIDE_ELEMENT, LENGTH, INDEX, BFLOAT, PTRADD_VECTOR];

// ---- shapes ----

/// What a fixed vector type is (`spec/ir.md` §4.3): `size` is the store
/// size, `alloc` the alloc size (the slot of a `Mem` vector).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// The vector type.
    pub ty: TypeId,
    /// The element type.
    pub elem_ty: TypeId,
    /// The element.
    pub elem: Elem,
    /// The number of lanes.
    pub lanes: u32,
    /// The value class (`target::vector_class`).
    pub class: Class,
    /// The store size in bytes.
    pub size: u64,
    /// The alloc size in bytes.
    pub alloc: u64,
}

impl Shape {
    /// The shape of `ty`, if it is a fixed vector of elements `Elem`
    /// knows.
    pub fn of(types: &TypeTable, ty: TypeId) -> Option<Shape> {
        let TypeKind::Vector { elem: elem_ty, len } = types.kind(ty) else { return None };
        Some(Shape {
            ty,
            elem_ty,
            elem: Elem::of(types, elem_ty)?,
            lanes: len,
            class: X64.class_of(types, ty),
            size: types.size_of(ty),
            alloc: types.alloc_size_of(ty),
        })
    }

    /// The vector as addressable lanes.
    ///
    /// # Errors
    /// [`LENGTH`], [`MASK_WIDE`] (a mask of at most 64 lanes:
    /// `crate::mask::MASK`), [`ODD_ELEMENT`], [`WIDE_ELEMENT`].
    pub fn lanes(self) -> CgResult<Lanes> {
        use hod_cg::lanes::ShapeRefusal as R;
        let log2 = match hod_cg::lanes::lane_log2(self.elem, self.lanes) {
            Ok(log2) => log2,
            Err(R::Mask) => return unsupported(crate::mask::MASK),
            Err(R::MaskWide) => return unsupported(MASK_WIDE),
            Err(R::Length) => return unsupported(LENGTH),
            Err(R::WideElement) => return unsupported(WIDE_ELEMENT),
            Err(R::OddElement) => return unsupported(ODD_ELEMENT),
        };
        Ok(Lanes { shape: self, log2 })
    }
}

/// A vector whose lanes are addressable: elements of 1, 2, 4 or 8 bytes,
/// at most [`MAX_LANES`] of them. Made by [`Shape::lanes`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lanes {
    /// The vector.
    pub shape: Shape,
    /// log2 of the lane size in bytes (0 to 3).
    pub log2: u32,
}

impl Lanes {
    /// The number of lanes.
    pub fn count(&self) -> u32 {
        self.shape.lanes
    }

    /// The lane size in bytes.
    pub fn bytes(&self) -> u32 {
        1 << self.log2
    }

    /// The byte offset of lane `lane` in the image.
    pub fn offset(&self, lane: u32) -> u32 {
        lane << self.log2
    }

    /// The class of one lane as a scalar.
    pub fn elem_class(&self) -> Class {
        self.shape.elem.class()
    }

    /// The lane loop's view of these lanes.
    pub fn geometry(&self) -> hod_cg::lanes::Geometry {
        hod_cg::lanes::Geometry {
            count: self.count(),
            log2: self.log2,
            class: self.shape.class,
            elem_class: self.elem_class(),
        }
    }

    /// The general operand size of one lane.
    pub fn size(&self) -> Size {
        [Size::B, Size::W, Size::D, Size::Q][self.log2 as usize]
    }

    /// The bytes an `Fpr` vector of this shape fills: 8 or 16.
    pub fn reg_bytes(&self) -> u32 {
        if self.shape.size > 8 { 16 } else { 8 }
    }
}

/// The lanes of the vector type `ty`.
///
/// # Errors
/// What [`Shape::lanes`] refuses; [`ODD_ELEMENT`] for an element `Elem`
/// does not know (a vector of pointers of another address space).
pub fn lanes(cx: &FnCx<'_, X64>, ty: TypeId) -> CgResult<Lanes> {
    match Shape::of(cx.types(), ty) {
        Some(shape) => shape.lanes(),
        None if Wide::of(cx, ty).is_some() => unsupported(WIDE_ELEMENT),
        None if cx.types().is_vector(ty) => unsupported(ODD_ELEMENT),
        None => cx.bug("a vector operation on a type that is no fixed vector"),
    }
}

// ---- images ----

/// The number of scratch images a lowering uses at once.
pub const SCRATCH_IMAGES: u32 = 3;

/// A scratch image: a new 16-byte, 16-aligned frame slot (see the module
/// docs). `k` names the image within the lowering; each call gives a
/// slot of its own.
///
/// # Errors
/// `limit:frame-size`.
pub fn scratch(cx: &mut FnCx<'_, X64>, k: u32) -> CgResult<FrameRef> {
    let _ = k;
    cx.alloc_slot(16, 16)
}

/// Stores the `Fpr` vector of `lanes` in `x` into a scratch image and
/// returns it (`movsd` or `movups`).
///
/// # Errors
/// As [`scratch`].
pub fn spill(cx: &mut FnCx<'_, X64>, x: Xmm, lanes: &Lanes, k: u32) -> CgResult<FrameRef> {
    let image = scratch(cx, k)?;
    store_fpr(cx, x, image, lanes.reg_bytes());
    Ok(image)
}

/// Loads the `Fpr` vector of `lanes` from `image` into `x`.
pub fn reload(cx: &mut FnCx<'_, X64>, x: Xmm, lanes: &Lanes, image: FrameRef) {
    load_fpr(cx, x, image, lanes.reg_bytes());
}

/// Where the image of vector operand `value` is: the slot of a `Mem`
/// vector (a constant written to a slot of its own), or its register
/// stored into a scratch image.
///
/// # Errors
/// What asking for the operand reports; as [`scratch`].
pub fn image(cx: &mut FnCx<'_, X64>, value: Value, lanes: &Lanes, k: u32) -> CgResult<FrameRef> {
    if lanes.shape.class == Class::Mem {
        return cx.use_mem(value);
    }
    let r = cx.use_reg(value)?;
    let image = spill(cx, xm(r), lanes, k)?;
    // Given back at once, as `hod_cg::lanes::images` does, so the
    // lowering has the whole pool: ask for each value once.
    cx.release(value);
    cx.free_temp(r);
    Ok(image)
}

/// Writes `value`, a vector of class `Mem` and `bytes` store bytes, into
/// slot `dst`: a constant by its image, a value by a copy. Writes `r11`
/// and `xmm15`.
///
/// # Errors
/// What writing the constant or asking for the operand reports.
pub fn write_mem(cx: &mut FnCx<'_, X64>, dst: FrameRef, value: Value, bytes: u32) -> CgResult<()> {
    if matches!(value.kind(), ValueKind::Const(_)) {
        return crate::aggr::materialize_mem(cx, dst, value);
    }
    let src = cx.use_mem(value)?;
    if src != dst {
        copy_frame(cx, dst, src, bytes);
    }
    Ok(())
}

/// The store size of `lanes` as a frame byte count.
///
/// # Errors
/// `limit:frame-size` beyond 4 GiB (never: at most 256 lanes of 8).
pub fn store_bytes(lanes: &Lanes) -> CgResult<u32> {
    u32::try_from(lanes.shape.size).map_or_else(|_| unsupported(hod_cg::LIMIT_FRAME_SIZE), Ok)
}

// ---- lane indices ----

/// A lane index operand, asked for by [`index`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Index {
    /// A constant index inside the vector.
    Lane(u32),
    /// A constant index `≥ N`, `undef` or `poison`: the result is poison.
    Poison,
    /// A variable index of `bits` bits (1 to 64) in `reg`; the bits above
    /// them are unspecified.
    Reg {
        /// The register.
        reg: Reg,
        /// The index's width.
        bits: u32,
    },
}

/// The lane index operand `value` of a vector of `lanes`: a constant is
/// decided here, a variable is asked for (and pinned).
///
/// # Errors
/// [`INDEX`] for an index wider than 64 bits; what asking reports.
pub fn index(cx: &mut FnCx<'_, X64>, value: Value, lanes: &Lanes) -> CgResult<Index> {
    index_in(cx, value, lanes.count())
}

/// [`index`] for a vector of `count` lanes of any shape.
///
/// # Errors
/// As [`index`].
pub fn index_in(cx: &mut FnCx<'_, X64>, value: Value, count: u32) -> CgResult<Index> {
    let ty = value_ty(cx, value);
    let Some(bits @ 1..=64) = cx.types().int_bits(ty) else {
        return unsupported(INDEX);
    };
    if let ValueKind::Const(id) = value.kind() {
        let lane = match cx.module.consts.get(id) {
            Const::Int { value, .. } => value,
            Const::Zero(_) => 0,
            _ => return Ok(Index::Poison),
        };
        return Ok(match u32::try_from(lane) {
            Ok(lane) if lane < count => Index::Lane(lane),
            _ => Index::Poison,
        });
    }
    let reg = gp(cx.use_reg(value)?);
    Ok(Index::Reg { reg, bits })
}

/// Puts the lane number of the variable index `reg` (`bits` wide) in
/// `rax`, inside a vector of `count` lanes (the module docs' rule):
/// masked with `count - 1` for a power of two, else `≥ count` clamped to
/// 0. Writes `rax`, `rdx` and the flags.
pub fn lane_number(cx: &mut FnCx<'_, X64>, reg: Reg, bits: u32, count: u32) {
    if count.is_power_of_two() {
        let width_mask = if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 };
        let mask = u64::from(count - 1) & width_mask;
        put(cx, mc::mov_rr(Size::D, RAX, reg));
        put(cx, mc::and_ri(Size::D, RAX, mask as i64));
        return;
    }
    put(cx, mc::mov_rr(Size::Q, RAX, reg));
    crate::mem::zero_extend(cx, RAX, bits);
    put(cx, mc::mov_ri(Size::D, RDX, 0));
    put(cx, mc::cmp_ri(Size::Q, RAX, i64::from(count)));
    put(cx, mc::cmovcc_rr(Cond::Ae, Size::Q, RAX, RDX));
}

/// Where lane `index` of the image at `image` is: a frame place for a
/// constant (lane 0 for [`Index::Poison`]), else `[rcx + rax * size]`
/// with `rcx` the image's address and `rax` the lane number. Writes
/// `rax`, `rcx`, `rdx` and the flags for a variable.
pub fn lane_at(cx: &mut FnCx<'_, X64>, image: FrameRef, lanes: &Lanes, index: Index) -> At {
    match index {
        Index::Lane(lane) => At::Frame(image.offset_by(lanes.offset(lane))),
        Index::Poison => At::Frame(image),
        Index::Reg { reg, bits } => {
            lane_number(cx, reg, bits, lanes.count());
            frame_address(cx, RCX, image);
            let scale = [Scale::S1, Scale::S2, Scale::S4, Scale::S8][lanes.log2 as usize];
            match RAX.index() {
                Some(i) => At::Mem(Mem::base_index(RCX, i, scale, 0)),
                None => At::Mem(Mem::base(RCX, 0)),
            }
        }
    }
}

/// A lane's place: in the frame, or at an address in registers.
#[derive(Clone, Copy, Debug)]
pub enum At {
    /// A frame place.
    Frame(FrameRef),
    /// A memory operand over registers.
    Mem(Mem),
}

/// Emits the instruction `make` builds over the memory operand of `at`.
pub fn at_inst(cx: &mut FnCx<'_, X64>, at: At, make: impl FnOnce(Mem) -> mc::Inst) {
    match at {
        At::Frame(place) => frame_inst(cx, place, make),
        At::Mem(m) => put(cx, make(m)),
    }
}

/// Loads one lane at `at` into `rt`: a general register (the lane
/// zero-extended to 64 bits) or a vector register (its scalar view: a
/// 2-byte lane through `r11`, the bits above it zero). Writes `r11`.
pub fn load_lane(cx: &mut FnCx<'_, X64>, lanes: &Lanes, at: At, rt: PReg) {
    let bytes = lanes.bytes();
    if reg::is_fpr(rt) {
        let x = xm(rt);
        match bytes {
            4 => at_inst(cx, at, |m| mc::movs_rm(FpSize::S, x, m)),
            8 => at_inst(cx, at, |m| mc::movs_rm(FpSize::D, x, m)),
            _ => {
                at_inst(cx, at, |m| gpr_load(R11, m, bytes));
                put(cx, mc::movd_xr(Size::D, x, R11));
            }
        }
    } else {
        let r = gp(rt);
        at_inst(cx, at, |m| gpr_load(r, m, bytes));
    }
}

/// Stores one lane from `rt` (a general register's low bytes or a vector
/// register's scalar view) at `at`. Writes `r11` for a 1- or 2-byte lane
/// of a vector register.
pub fn store_lane(cx: &mut FnCx<'_, X64>, lanes: &Lanes, at: At, rt: PReg) {
    let size = lanes.size();
    if reg::is_fpr(rt) {
        let x = xm(rt);
        match lanes.bytes() {
            4 => at_inst(cx, at, |m| mc::movs_mr(FpSize::S, m, x)),
            8 => at_inst(cx, at, |m| mc::movs_mr(FpSize::D, m, x)),
            _ => {
                put(cx, mc::movd_rx(Size::D, R11, x));
                at_inst(cx, at, |m| mc::mov_mr(size, m, R11));
            }
        }
    } else {
        let r = gp(rt);
        at_inst(cx, at, |m| mc::mov_mr(size, m, r));
    }
}

// ---- element access ----

/// Lane `lane` of the `Fpr` vector in `v` into `rd`, without memory: a
/// general result by `movd`/`movq` (after `pshufd` to bring the lane
/// down) or `pextrw` (2- and 1-byte lanes, a byte shifted down); a
/// floating-point result by `pshufd` (4- and 8-byte lanes) or `pextrw`
/// and `movd` through `r11`. Writes `r11`, `xmm15`.
fn extract_reg(cx: &mut FnCx<'_, X64>, lanes: &Lanes, v: Xmm, lane: u32, rd: PReg) {
    let fpr = reg::is_fpr(rd);
    match lanes.bytes() {
        4 | 8 => {
            // The lane's dwords to the bottom.
            let order = if lanes.bytes() == 4 { lane } else { (2 * lane) | ((2 * lane + 1) << 2) };
            let src = if lane == 0 {
                v
            } else {
                let t = if fpr { xm(rd) } else { XMM15 };
                put(cx, mc::pshufd_rri(t, v, order as u8));
                t
            };
            let size = if lanes.bytes() == 4 { Size::D } else { Size::Q };
            if fpr {
                if src != xm(rd) {
                    put(cx, mc::movap_rr(FpSize::S, xm(rd), src));
                }
            } else {
                put(cx, mc::movd_rx(size, gp(rd), src));
            }
        }
        2 => {
            let r = if fpr { R11 } else { gp(rd) };
            put(cx, mc::pextrw_rri(r, v, lane as u8));
            if fpr {
                put(cx, mc::movd_xr(Size::D, xm(rd), R11));
            }
        }
        _ => {
            let r = gp(rd);
            put(cx, mc::pextrw_rri(r, v, (lane / 2) as u8));
            if lane % 2 == 1 {
                put(cx, mc::shr_ri(Size::D, r, 8));
            }
        }
    }
}

/// `%r = extractelement %vector, %index`: for an `Fpr` vector and a
/// constant index, the lane taken out of the register
/// ([`extract_reg`]); otherwise one load of the lane from the vector's
/// image. An index out of range reads lane 0 (a constant) or the masked
/// or clamped lane (a variable).
///
/// # Errors
/// What [`lanes`] and [`index`] refuse; what asking for the operands
/// reports.
pub fn extract_element<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    vector: Value,
    index_value: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if let Some(w) = Wide::of(cx, value_ty(cx, vector)) {
        return wide_extract(cx, inst, &w?, vector, index_value);
    }
    let lanes = lanes(cx, value_ty(cx, vector))?;
    let idx = index(cx, index_value, &lanes)?;
    if lanes.shape.class == Class::Fpr {
        let v = cx.use_reg(vector)?;
        if let Index::Lane(_) | Index::Poison = idx {
            let lane = if let Index::Lane(lane) = idx { lane } else { 0 };
            let rd = cx.def_reg(inst)?;
            extract_reg(cx, &lanes, xm(v), lane, rd);
            return cx.finish(inst, rd);
        }
        let image = spill(cx, xm(v), &lanes, 0)?;
        let rd = cx.def_reg(inst)?;
        let at = lane_at(cx, image, &lanes, idx);
        load_lane(cx, &lanes, at, rd);
        return cx.finish(inst, rd);
    }
    let image = cx.use_mem(vector)?;
    let rd = cx.def_reg(inst)?;
    let at = lane_at(cx, image, &lanes, idx);
    load_lane(cx, &lanes, at, rd);
    cx.finish(inst, rd)
}

/// `%r = insertelement %vector, %value, %index`: for an `Fpr` vector, a
/// 2-byte lane at a constant index by `pinsrw`; otherwise the register
/// spilled to a scratch image, the lane stored, the image reloaded. A
/// `Mem` vector is copied to the result's slot and the lane stored there.
/// An index out of range writes nothing (a constant) or the masked or
/// clamped lane (a variable).
///
/// # Errors
/// What [`lanes`] and [`index`] refuse; what asking for the operands
/// reports.
pub fn insert_element<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    vector: Value,
    value: Value,
    index_value: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if let Some(w) = Wide::of(cx, inst.ty()) {
        return wide_insert(cx, inst, &w?, vector, value, index_value);
    }
    let lanes = lanes(cx, inst.ty())?;
    let idx = index(cx, index_value, &lanes)?;
    let e = cx.use_reg(value)?;
    if lanes.shape.class == Class::Fpr {
        let v = cx.use_reg(vector)?;
        match idx {
            Index::Poison => {}
            Index::Lane(lane) if lanes.bytes() == 2 => {
                let r = if reg::is_fpr(e) {
                    put(cx, mc::movd_rx(Size::D, R11, xm(e)));
                    R11
                } else {
                    gp(e)
                };
                put(cx, mc::pinsrw_rri(xm(v), r, lane as u8));
            }
            _ => {
                let image = spill(cx, xm(v), &lanes, 0)?;
                let at = lane_at(cx, image, &lanes, idx);
                store_lane(cx, &lanes, at, e);
                reload(cx, xm(v), &lanes, image);
            }
        }
        return cx.finish(inst, v);
    }
    let bytes = store_bytes(&lanes)?;
    let r = cx.def_mem(inst)?;
    write_mem(cx, r, vector, bytes)?;
    if idx != Index::Poison {
        let at = lane_at(cx, r, &lanes, idx);
        store_lane(cx, &lanes, at, e);
    }
    Ok(())
}

/// Where lane `entry` of a shuffle's concatenated sources is: source 0 or
/// 1 and the lane in it; `None` for an undefined entry.
pub fn source(entry: u32, n: u32) -> Option<(usize, u32)> {
    if entry == SHUFFLE_UNDEF || entry >= 2 * n {
        None
    } else if entry < n {
        Some((0, entry))
    } else {
        Some((1, entry - n))
    }
}

/// `%r = shufflevector %first, %second, <mask>`: an `Fpr` splat of a
/// 4-byte lane or a permutation of the 4-byte lanes of one `Fpr` source
/// is one `pshufd`; otherwise lane by lane through `r11`, from the
/// sources' images into the result's (a scratch image reloaded for an
/// `Fpr` result, the slot of a `Mem` one), an undefined entry zero. A
/// source no entry names is not asked for; `%a, %a` is asked once.
///
/// # Errors
/// What [`lanes`] refuses for either type; what asking for the operands
/// reports.
pub fn shuffle_vector<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    first: Value,
    second: Value,
    mask: &[u32],
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if let Some(w) = Wide::of(cx, value_ty(cx, first)) {
        return wide_shuffle(cx, inst, &w?, first, second, mask);
    }
    let src = lanes(cx, value_ty(cx, first))?;
    let dst = lanes(cx, inst.ty())?;
    let n = src.count();
    let mut used = [false; 2];
    for &entry in mask {
        if let Some((which, _)) = source(entry, n) {
            used[which] = true;
        }
    }
    if second == first && used[1] {
        used[0] = true;
    }
    if let Some(done) = shuffle_dwords(cx, inst, first, second, mask, &src, &dst, used) {
        return done;
    }
    let mut images: [Option<FrameRef>; 2] = [None; 2];
    let mut k = 0;
    for (which, value) in [first, second].into_iter().enumerate() {
        if !used[which] {
            continue;
        }
        if which == 1 && second == first {
            images[1] = images[0];
            continue;
        }
        images[which] = Some(image(cx, value, &src, k)?);
        k += 1;
    }
    let out = if dst.shape.class == Class::Fpr { scratch(cx, k)? } else { cx.def_mem(inst)? };
    let size = dst.size();
    for (lane, &entry) in (0u32..).zip(mask) {
        let to = out.offset_by(dst.offset(lane));
        match source(entry, n).and_then(|(which, l)| Some((images[which]?, l))) {
            None => frame_inst(cx, to, |m| mc::mov_mi(size, m, 0)),
            Some((image, l)) => {
                frame_inst(cx, image.offset_by(src.offset(l)), |m| gpr_load(R11, m, dst.bytes()));
                frame_inst(cx, to, |m| mc::mov_mr(size, m, R11));
            }
        }
    }
    if dst.shape.class == Class::Fpr {
        let rd = cx.def_reg(inst)?;
        reload(cx, xm(rd), &dst, out);
        return cx.finish(inst, rd);
    }
    Ok(())
}

/// The `pshufd` form of [`shuffle_vector`]: 4-byte lanes, an `Fpr`
/// source and result of four lanes, every entry defined and from one
/// source. `None` for any other shuffle (an undefined entry is zero, as
/// on AArch64).
#[allow(clippy::too_many_arguments, reason = "the shuffle's operands and shapes")]
fn shuffle_dwords<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    first: Value,
    second: Value,
    mask: &[u32],
    src: &Lanes,
    dst: &Lanes,
    used: [bool; 2],
) -> Option<CgResult<()>> {
    if src.bytes() != 4
        || dst.bytes() != 4
        || src.shape.class != Class::Fpr
        || dst.count() != 4
        || src.count() != 4
        || used == [true, true] && first != second
        || mask.iter().any(|&e| source(e, 4).is_none())
    {
        return None;
    }
    let value = if used[1] && !used[0] { second } else { first };
    let mut order = 0u8;
    for (k, &entry) in mask.iter().enumerate() {
        let lane = source(entry, 4).map_or(0, |(_, l)| l);
        order |= (lane as u8) << (2 * k);
    }
    Some((|| {
        let v = cx.use_reg(value)?;
        let rd = cx.def_reg(inst)?;
        put(cx, mc::pshufd_rri(xm(rd), xm(v), order));
        cx.finish(inst, rd)
    })())
}

// ---- wide lanes (WP-152, §5.6.18) ----

/// A vector of `i128` or `x86_fp80` lanes, which the lane loop does not
/// address: `extractelement`, `insertelement` and `shufflevector` copy
/// them whole between slots. An `i128` lane is 16 bytes at `16 · i`; an
/// `x86_fp80` lane is its 10 bytes at `10 · i` (vector elements are
/// packed, `spec/ir.md` §4.3). Vector and element are of class `Mem`
/// (`target::vector_class` makes `<1 x i128>` `Mem`). `fp128` lanes stay
/// [`WIDE_ELEMENT`] (§5.6.16).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Wide {
    /// The number of lanes.
    pub lanes: u32,
    /// The bytes of a lane, which are also the bytes between the starts
    /// of two lanes: 16, or 10.
    pub stride: u32,
    /// The store size of the vector.
    pub size: u32,
}

impl Wide {
    /// The wide lanes of `ty`; `None` when it is no vector of such
    /// lanes; [`LENGTH`] beyond [`MAX_LANES`] lanes.
    pub fn of(cx: &FnCx<'_, X64>, ty: TypeId) -> Option<CgResult<Wide>> {
        let types = cx.types();
        let TypeKind::Vector { elem, len } = types.kind(ty) else { return None };
        let stride = match types.kind(elem) {
            TypeKind::Int(128) => 16,
            TypeKind::X86Fp80 => 10,
            _ => return None,
        };
        if len > MAX_LANES || X64.class_of(types, ty) != Class::Mem {
            return Some(unsupported(LENGTH));
        }
        Some(Ok(Wide { lanes: len, stride, size: len * stride }))
    }

    /// The offset of lane `lane` in the image.
    pub fn offset(&self, lane: u32) -> u32 {
        lane * self.stride
    }
}

/// Where lane `index` of the wide image `image` is: a frame place for a
/// constant (lane 0 for [`Index::Poison`]); for a variable, its address
/// in `rcx` (the lane number by [`lane_number`], times the stride).
/// Writes `rax`, `rcx`, `rdx` and the flags for a variable.
fn wide_lane_at(cx: &mut FnCx<'_, X64>, image: FrameRef, w: &Wide, index: Index) -> At {
    match index {
        Index::Lane(lane) => At::Frame(image.offset_by(w.offset(lane))),
        Index::Poison => At::Frame(image),
        Index::Reg { reg, bits } => {
            lane_number(cx, reg, bits, w.lanes);
            frame_address(cx, RCX, image);
            let Some(i) = RAX.index() else { unreachable!("rax is an index register") };
            if w.stride == 16 {
                put(cx, mc::shl_ri(Size::Q, RAX, 4));
                put(cx, mc::lea(Size::Q, RCX, Mem::base_index(RCX, i, Scale::S1, 0)));
            } else {
                // 10 · n = 2 · (n + 4n).
                put(cx, mc::lea(Size::Q, RAX, Mem::base_index(RAX, i, Scale::S4, 0)));
                put(cx, mc::lea(Size::Q, RCX, Mem::base_index(RCX, i, Scale::S2, 0)));
            }
            At::Mem(Mem::base(RCX, 0))
        }
    }
}

/// Copies `bytes` bytes from `src` to the lane at `at` (a frame place, or
/// `[rcx]` from [`wide_lane_at`]): 16 through `xmm15`, the rest through
/// `r11`.
fn copy_into_lane(cx: &mut FnCx<'_, X64>, at: At, src: FrameRef, bytes: u32) {
    if let At::Frame(dst) = at {
        return copy_frame(cx, dst, src, bytes);
    }
    let mut done = 0;
    if bytes >= 16 {
        frame_inst(cx, src, |s| mc::movup_rm(FpSize::S, XMM15, s));
        put(cx, mc::movup_mr(FpSize::S, Mem::base(RCX, 0), XMM15));
        done = 16;
    }
    for (off, size) in crate::common::pieces(bytes - done) {
        let size_of = crate::common::exact_size(size).unwrap_or(Size::Q);
        frame_inst(cx, src.offset_by(done + off), |s| gpr_load(R11, s, size));
        put(cx, mc::mov_mr(size_of, Mem::base(RCX, (done + off) as i32), R11));
    }
}

/// `extractelement` of a wide lane: the lane's bytes copied to the
/// result's slot.
fn wide_extract<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    w: &Wide,
    vector: Value,
    index_value: Value,
) -> CgResult<()> {
    let idx = index_in(cx, index_value, w.lanes)?;
    let image = cx.use_mem(vector)?;
    let r = cx.def_mem(inst)?;
    match wide_lane_at(cx, image, w, idx) {
        At::Frame(src) => copy_frame(cx, r, src, w.stride),
        At::Mem(_) => crate::common::copy_from_ptr(cx, r, RCX, 0, w.stride),
    }
    Ok(())
}

/// `insertelement` of a wide lane: the vector copied to the result's
/// slot, the element's bytes stored over the lane.
fn wide_insert<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    w: &Wide,
    vector: Value,
    value: Value,
    index_value: Value,
) -> CgResult<()> {
    let idx = index_in(cx, index_value, w.lanes)?;
    let r = cx.def_mem(inst)?;
    write_mem(cx, r, vector, w.size)?;
    let e = cx.use_mem(value)?;
    if idx != Index::Poison {
        let at = wide_lane_at(cx, r, w, idx);
        copy_into_lane(cx, at, e, w.stride);
    }
    Ok(())
}

/// `shufflevector` of wide lanes: lane by lane from the sources' slots
/// into the result's, an undefined entry zero (as for the other lanes).
fn wide_shuffle<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    src: &Wide,
    first: Value,
    second: Value,
    mask: &[u32],
) -> CgResult<()> {
    let dst = match Wide::of(cx, inst.ty()) {
        Some(dst) => dst?,
        None => return cx.bug("a shuffle whose result has other lanes than its sources"),
    };
    let n = src.lanes;
    let mut used = [false; 2];
    for &entry in mask {
        if let Some((which, _)) = source(entry, n) {
            used[which] = true;
        }
    }
    if second == first && used[1] {
        used[0] = true;
    }
    let mut images: [Option<FrameRef>; 2] = [None; 2];
    for (which, value) in [first, second].into_iter().enumerate() {
        if !used[which] {
            continue;
        }
        images[which] =
            if which == 1 && second == first { images[0] } else { Some(cx.use_mem(value)?) };
    }
    let out = cx.def_mem(inst)?;
    for (lane, &entry) in (0u32..).zip(mask) {
        let to = out.offset_by(dst.offset(lane));
        match source(entry, n).and_then(|(which, l)| Some((images[which]?, l))) {
            None => {
                for (off, size) in crate::common::pieces(dst.stride) {
                    let size_of = crate::common::exact_size(size).unwrap_or(Size::Q);
                    frame_inst(cx, to.offset_by(off), |m| mc::mov_mi(size_of, m, 0));
                }
            }
            Some((image, l)) => copy_frame(cx, to, image.offset_by(src.offset(l)), dst.stride),
        }
    }
    Ok(())
}

/// Whether `ty` is a vector whose lanes `lanes` accepts.
pub fn is_lane_vector(types: &TypeTable, ty: TypeId) -> bool {
    Shape::of(types, ty).is_some_and(|s| s.lanes().is_ok())
}

// ---- constants the generic glue puts in registers (WP-147) ----

/// Glue (`Target::materialize`, WP-147, §5.6.16): the vector constants
/// the scalar groups' materializers have no form for. `None` when
/// `value` is none of them (the caller goes on as before).
///
/// - A mask (`<N x i1>`, `N ≤ 64`, class `Gpr`) other than a zero-like
///   one: its bits (`mask::const_bits`, undefined lanes 0) by `mov`.
/// - A vector of class `Fpr` whose image has relocations (`<1 x i64>
///   <i64 ptrtoint (ptr @f to i64)>`, `<2 x ptr> <ptr @a, ptr @b>`):
///   the image is written below the stack pointer by
///   `constaddr::write_image` (`lea rsp, [rsp - 16]`, the image at
///   `Outgoing(0)`), loaded, and the 16 bytes given back. Writes `rax`
///   and `r11`; flags kept (`lea`, `mov`, `movups`).
///
/// # Errors
/// What `constaddr::write_image` reports (a relocation that is not an
/// aligned 8-byte symbol address in a vector: `x64:constant-relocation`).
pub fn materialize(cx: &mut FnCx<'_, X64>, reg: PReg, value: Value) -> Option<CgResult<()>> {
    let ValueKind::Const(id) = value.kind() else { return None };
    let ty = value_ty(cx, value);
    if !cx.types().is_vector(ty) {
        return None;
    }
    if reg::is_fpr(reg) {
        let mut image = Vec::new();
        let mut fixups = Vec::new();
        let flattened = cx.module.consts.flatten(cx.types(), id, &mut image, &mut fixups);
        if flattened.is_err() || fixups.is_empty() || image.len() > 16 {
            return None;
        }
        let bytes = if image.len() > 8 { 16 } else { 8 };
        let area = Mem::base(Reg::Rsp, -16);
        put(cx, mc::lea(Size::Q, Reg::Rsp, area));
        let written = crate::constaddr::write_image(cx, FrameRef::Outgoing(0), ty, &image, &fixups);
        if written.is_ok() {
            load_fpr(cx, xm(reg), FrameRef::Outgoing(0), bytes);
        }
        put(cx, mc::lea(Size::Q, Reg::Rsp, Mem::base(Reg::Rsp, 16)));
        return Some(written);
    }
    let n = crate::mask::mask_lanes(cx.types(), ty).filter(|&n| n <= crate::mask::MAX_MASK)?;
    let bits = crate::mask::const_bits(cx, id, n)?;
    crate::common::mov_imm(cx, gp(reg), bits, 64);
    Some(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TypeTable {
        TypeTable::new(hod_ir::TargetSpec::x86_64_linux_gnu().data_layout())
    }

    /// The shapes follow the layout and the class of `vector_class`.
    #[test]
    fn shapes_follow_the_layout() {
        let mut t = table();
        let cases = [
            (TypeId::I32, 4, Class::Fpr, 16, 16),
            (TypeId::I32, 2, Class::Fpr, 8, 8),
            (TypeId::I32, 3, Class::Mem, 12, 16),
            (TypeId::I8, 4, Class::Mem, 4, 4),
            (TypeId::I64, 4, Class::Mem, 32, 32),
            (TypeId::DOUBLE, 1, Class::Fpr, 8, 8),
            (TypeId::I1, 8, Class::Gpr, 1, 1),
        ];
        for (elem, len, class, size, alloc) in cases {
            let ty = t.vector(elem, len).unwrap();
            let s = Shape::of(&t, ty).unwrap();
            assert_eq!((s.class, s.size, s.alloc), (class, size, alloc), "{len}");
        }
    }

    /// The refusals by shape.
    #[test]
    fn lane_refusals_by_name() {
        let mut t = table();
        let i4 = t.int(4).unwrap();
        let i128 = t.int(128).unwrap();
        let cases = [
            (TypeId::I1, 65, MASK_WIDE),
            (i4, 16, ODD_ELEMENT),
            (i128, 1, WIDE_ELEMENT),
            (TypeId::FP128, 2, WIDE_ELEMENT),
            (TypeId::I8, 257, LENGTH),
        ];
        for (elem, len, name) in cases {
            let ty = t.vector(elem, len).unwrap();
            let err = Shape::of(&t, ty).unwrap().lanes().unwrap_err();
            assert_eq!(err, hod_cg::CgError::unsupported(name), "{len}");
        }
        let ty = t.vector(TypeId::DOUBLE, 3).unwrap();
        let l = Shape::of(&t, ty).unwrap().lanes().unwrap();
        assert_eq!(
            (l.bytes(), l.offset(2), l.elem_class(), l.size()),
            (8, 16, Class::Fpr, Size::Q)
        );
    }

    #[test]
    fn shuffle_sources() {
        assert_eq!(source(0, 4), Some((0, 0)));
        assert_eq!(source(5, 4), Some((1, 1)));
        assert_eq!(source(SHUFFLE_UNDEF, 4), None);
    }
}
