//! Vectors as values (`spec/codegen.md` §5.5.1 "Vectors, the base
//! (WP-079)"; design note `docs/design/0003-vectors.md`): the shared
//! helpers every vector lowering builds on, and the lowering of
//! `extractelement`, `insertelement` and `shufflevector`.
//!
//! **Owner: WP-079.** Tests: `tests/vector.rs`. The lane loop
//! (`lanes.rs`, WP-080), masks (`mask.rs`, WP-081) and the vector ABI
//! (WP-082) use the helpers below and do not re-implement them.
//!
//! # The helper API
//!
//! - [`Shape::of`]: what a fixed vector type is — element ([`Elem`]),
//!   lane count, class, store size, alloc size and alignment, all as
//!   `hod_ir`'s layout gives them (`spec/ir.md` §4.3).
//! - [`Shape::lanes`] / [`lanes`]: the same as a [`Lanes`], the view of a
//!   vector whose lanes are addressable bytes (1, 2, 4 or 8 bytes each,
//!   at most [`MAX_LANES`]); every other shape is refused here, by the
//!   names of design §4.4, so a lowering that starts from [`lanes`]
//!   never sees one.
//! - [`Lanes::offset`]: where lane `i` is in the value's in-memory image
//!   (`i` times the lane size: vector elements are packed).
//! - [`image`]: the frame place that holds a vector operand's image —
//!   its own slot for a `Mem` vector, a scratch image for an `Fpr` one
//!   ([`spill`], [`scratch`]); [`reload`] puts an image back in a
//!   register.
//! - [`index`] then [`lane_cursor`]: a lane index operand (constant or
//!   variable) and a [`Cursor`] at that lane of an image, never outside
//!   it.
//! - [`load_lane`], [`store_lane`]: one lane between an image and a
//!   register of the element's class.
//!
//! # Where a register vector's image is
//!
//! An `Fpr` vector (64 or 128 bits in a `D`/`Q` register) has no frame
//! slot of its own while it is in a register. When a lowering needs its
//! image (a variable lane index, a lane loop), it stores the register
//! into one of [`SCRATCH_IMAGES`] **scratch images** of the function: a
//! 48-byte, 16-aligned frame area allocated on first use and kept in
//! [`VectorState`]. An image in a scratch slot is valid from the store
//! until the lowering ends, provided no deferred instruction is emitted
//! in between (one may itself use the scratch images): a lowering calls
//! [`deferred_first`](crate::common::deferred_first) before anything
//! else, as every entry point does, and from then on asking for an
//! operand only loads or materializes it, so [`image`] may be called for
//! one operand after another. The scratch area is plain frame memory: it
//! survives calls the lowering makes.
//!
//! # Lane indices out of range
//!
//! `extractelement`/`insertelement` with an index `≥ N` give poison
//! (`LangRef`), so any lane is a correct result, but the access must stay
//! inside the image. A variable index is first reduced to its width (the
//! bits above it are unspecified, §2.2), then **masked** with `N - 1`
//! when `N` is a power of two, or **clamped** to lane 0 (`cmp` + `csel`
//! with `xzr`) otherwise. A constant index out of range is
//! [`Index::Poison`], which reads lane 0 and writes nothing.
//!
//! # Registers
//!
//! The helpers write `x16` and `x17` only (and the register they are
//! given); none writes `v30`/`v31` except where it says so.

use hod_cg::abi::{AbiUnsupported, Sig};
use hod_cg::{CgResult, Class, FnCx, FrameRef, PReg, Target};
use hod_ir::{Const, Inst, SHUFFLE_UNDEF, TypeId, TypeKind, TypeTable, Value, ValueKind};
use hod_mc_a64 as mc;
use hod_mc_a64::{Arr, Cond, Load, MemSize, Reg, Shift, Size, VMemSize, VReg, VecBits};

use crate::common::{
    Cursor, V31, X16, X17, copy_bytes, deferred_first, frame_address, load_fpr_at, load_gpr_at,
    materialize_mem_const, put, store_fpr_at, store_gpr_at, unsupported, value_ty, vr, xr,
};
use crate::reg;
use crate::target::A64;

// ---- refusals (design 0003 §4.4) ----

/// A lane operation on a vector of `i1` of at most 64 lanes (a mask):
/// WP-081 makes masks bit-packed `Gpr` values.
pub const MASK: &str = "a64:vector-mask";
/// A lane operation on a vector of more than 64 `i1`.
pub const MASK_WIDE: &str = "a64:mask-wide";
/// A lane operation on a vector of integers of a width other than 8, 16,
/// 32 or 64 bits, at most 64, and not `i1` (`<4 x i4>`, `<2 x i24>`).
pub const ODD_ELEMENT: &str = "a64:vector-odd-element";
/// A lane operation on a vector of `i128` (or wider) or `fp128`.
pub const WIDE_ELEMENT: &str = "a64:vector-wide-element";
/// Arithmetic on a vector of `bfloat` (moves of them are lowered).
pub const BFLOAT: &str = "a64:vector-bfloat";
/// A lane operation on a vector of more than [`MAX_LANES`] lanes.
pub const LENGTH: &str = "a64:vector-length";
/// An `llvm.aarch64.neon.*` or `llvm.aarch64.crypto.*` intrinsic.
pub const INTRINSIC_TARGET: &str = "a64:intrinsic-target";
/// A vector of class `Mem` crossing a call or a return, as an argument,
/// a result, or a leaf of an aggregate one: WP-082 (the vector ABI).
/// 64- and 128-bit vectors (class `Fpr`) travel in `v0`..`v7`.
pub const CALL: &str = "a64:vector-call";
/// A lane index wider than 64 bits.
pub const INDEX: &str = "a64:vector-index";
/// An element-wise operation on a `Mem` vector: the lane loop, WP-080.
pub use crate::aggr::MEM_VECTOR;
/// An element-wise operation on an `Fpr` vector without one instruction
/// (vector `frem` among them): the lane loop, WP-080.
pub use crate::float::VECTOR_OP;
/// `getelementptr` over a vector of pointers.
pub use crate::mem::PTRADD_VECTOR;

/// The most lanes a lane operation handles: its code is unrolled at emit
/// time, one access per lane.
pub const MAX_LANES: u32 = 256;

// ---- shapes ----

/// The element of a vector type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Elem {
    /// An integer of this many bits (`i1` included).
    Int(u32),
    /// A pointer in address space 0 (64 bits).
    Ptr,
    /// `half`.
    Half,
    /// `bfloat`.
    BFloat,
    /// `float`.
    Float,
    /// `double`.
    Double,
    /// `fp128`.
    Fp128,
}

impl Elem {
    /// The element of type `ty`, if a vector may have it on this target.
    pub fn of(types: &TypeTable, ty: TypeId) -> Option<Elem> {
        Some(match types.kind(ty) {
            TypeKind::Int(bits) => Elem::Int(bits),
            TypeKind::Ptr { addr_space: 0 } => Elem::Ptr,
            TypeKind::Half => Elem::Half,
            TypeKind::BFloat => Elem::BFloat,
            TypeKind::Float => Elem::Float,
            TypeKind::Double => Elem::Double,
            TypeKind::Fp128 => Elem::Fp128,
            _ => return None,
        })
    }

    /// The width in bits.
    pub fn bits(self) -> u32 {
        match self {
            Elem::Int(bits) => bits,
            Elem::Ptr | Elem::Double => 64,
            Elem::Half | Elem::BFloat => 16,
            Elem::Float => 32,
            Elem::Fp128 => 128,
        }
    }

    /// Whether it is a floating-point type.
    pub fn is_float(self) -> bool {
        !matches!(self, Elem::Int(_) | Elem::Ptr)
    }
}

/// What a fixed vector type is (`spec/ir.md` §4.3). The sizes are
/// `hod_ir`'s: `size` is the store size (`⌈lanes · bits / 8⌉`), `alloc`
/// the alloc size, `align` the ABI alignment.
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
    /// The value class (`Fpr`: 64 or 128 bits of a power-of-two count of
    /// non-`i1` elements; `Mem` otherwise).
    pub class: Class,
    /// The store size in bytes.
    pub size: u64,
    /// The alloc size in bytes: the size of the slot of a `Mem` vector.
    pub alloc: u64,
    /// The ABI alignment in bytes.
    pub align: u64,
}

impl Shape {
    /// The shape of `ty`, if it is a fixed vector (not a scalable one).
    pub fn of(types: &TypeTable, ty: TypeId) -> Option<Shape> {
        let TypeKind::Vector { elem: elem_ty, len } = types.kind(ty) else { return None };
        Some(Shape {
            ty,
            elem_ty,
            elem: Elem::of(types, elem_ty)?,
            lanes: len,
            class: A64.class_of(types, ty),
            size: types.size_of(ty),
            alloc: types.alloc_size_of(ty),
            align: types.align_of(ty),
        })
    }

    /// The width in bits: lanes times the element width.
    pub fn bits(&self) -> u64 {
        u64::from(self.lanes) * u64::from(self.elem.bits())
    }

    /// The vector as addressable lanes.
    ///
    /// # Errors
    /// [`LENGTH`] beyond [`MAX_LANES`] lanes; [`MASK`] or [`MASK_WIDE`]
    /// for `i1` elements; [`ODD_ELEMENT`] for integers of a width that is
    /// not a whole number of bytes; [`WIDE_ELEMENT`] for elements wider
    /// than 64 bits.
    pub fn lanes(self) -> CgResult<Lanes> {
        if self.elem == Elem::Int(1) {
            return unsupported(if self.lanes > 64 { MASK_WIDE } else { MASK });
        }
        if self.lanes > MAX_LANES {
            return unsupported(LENGTH);
        }
        let log2 = match self.elem.bits() {
            8 => 0,
            16 => 1,
            32 => 2,
            64 => 3,
            bits if bits > 64 => return unsupported(WIDE_ELEMENT),
            _ => return unsupported(ODD_ELEMENT),
        };
        Ok(Lanes { shape: self, log2 })
    }
}

/// A vector whose lanes are addressable: elements of 1, 2, 4 or 8 bytes
/// (`i8`..`i64`, `ptr`, `half`, `bfloat`, `float`, `double`), at most
/// [`MAX_LANES`] of them. Made by [`Shape::lanes`].
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

    /// The class of one lane as a scalar value: `Gpr` for integers and
    /// pointers, `Fpr` for floating-point elements.
    pub fn elem_class(&self) -> Class {
        if self.shape.elem.is_float() { Class::Fpr } else { Class::Gpr }
    }

    /// The lane width as an encoder element.
    pub fn elem(&self) -> mc::Elem {
        mc::Elem::ALL[self.log2 as usize]
    }

    /// The register width of an `Fpr` vector of this shape: 64 or 128
    /// bits.
    pub fn vec_bits(&self) -> VecBits {
        if self.shape.size > 8 { VecBits::V128 } else { VecBits::V64 }
    }

    /// The arrangement of an `Fpr` vector of this shape. A 64-bit vector
    /// of one 64-bit element is `2D`; its upper half is unspecified.
    pub fn arr(&self) -> Arr {
        match (self.log2, self.vec_bits()) {
            (0, VecBits::V64) => Arr::B8,
            (0, VecBits::V128) => Arr::B16,
            (1, VecBits::V64) => Arr::H4,
            (1, VecBits::V128) => Arr::H8,
            (2, VecBits::V64) => Arr::S2,
            (2, VecBits::V128) => Arr::S4,
            _ => Arr::D2,
        }
    }

    /// The general load of one lane (zero-extending).
    pub fn gpr_load(&self) -> Load {
        [Load::B, Load::H, Load::W, Load::X][self.log2 as usize]
    }

    /// The general store of one lane.
    pub fn gpr_store(&self) -> MemSize {
        [MemSize::B, MemSize::H, MemSize::W, MemSize::X][self.log2 as usize]
    }

    /// The vector-register view of one lane.
    pub fn fpr_view(&self) -> VMemSize {
        [VMemSize::B, VMemSize::H, VMemSize::S, VMemSize::D][self.log2 as usize]
    }
}

/// The lanes of the vector type `ty`.
///
/// # Errors
/// What [`Shape::lanes`] refuses; `Internal` if `ty` is no fixed vector.
pub fn lanes(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<Lanes> {
    match Shape::of(cx.types(), ty) {
        Some(shape) => shape.lanes(),
        None => cx.bug("a vector operation on a type that is no fixed vector"),
    }
}

// ---- images ----

/// What the vector helpers remember about the function being compiled
/// (a part of `target::FnState`): the scratch images.
#[derive(Clone, Copy, Default, Debug)]
pub struct VectorState {
    /// The scratch area, once allocated.
    scratch: Option<FrameRef>,
}

/// The number of scratch images a lowering may use at once.
pub const SCRATCH_IMAGES: u32 = 3;

/// `slot` moved `off` bytes on.
pub fn offset(slot: FrameRef, off: u32) -> FrameRef {
    match slot {
        FrameRef::Local(o) => FrameRef::Local(o + off),
        FrameRef::Incoming(o) => FrameRef::Incoming(o + off),
        FrameRef::Outgoing(o) => FrameRef::Outgoing(o + off),
    }
}

/// Scratch image `k` (below [`SCRATCH_IMAGES`]): 16 bytes, 16-aligned,
/// for the image of an `Fpr` vector (see the module docs for how long it
/// is valid). The area is allocated on first use.
///
/// # Errors
/// `limit:frame-size`; `Internal` for `k` out of range.
pub fn scratch(cx: &mut FnCx<'_, A64>, k: u32) -> CgResult<FrameRef> {
    if k >= SCRATCH_IMAGES {
        return cx.bug("a scratch image beyond the area");
    }
    let area = if let Some(area) = cx.state.vector.scratch {
        area
    } else {
        let area = cx.alloc_slot(u64::from(16 * SCRATCH_IMAGES), 16)?;
        cx.state.vector.scratch = Some(area);
        area
    };
    Ok(offset(area, 16 * k))
}

/// The register view of an `Fpr` vector of `shape`: `D` or `Q`.
fn view(shape: &Shape) -> VMemSize {
    if shape.size > 8 { VMemSize::Q } else { VMemSize::D }
}

/// Stores the `Fpr` vector of `shape` in `reg` into scratch image `k`
/// and returns it. Writes `x16` (a far frame offset).
///
/// # Errors
/// As [`scratch`].
pub fn spill(cx: &mut FnCx<'_, A64>, reg: VReg, shape: &Shape, k: u32) -> CgResult<FrameRef> {
    let image = scratch(cx, k)?;
    let mut at = Cursor::frame(cx, image, X16);
    store_fpr_at(cx, view(shape), reg, &mut at);
    Ok(image)
}

/// Loads the `Fpr` vector of `shape` from `image` into `reg`. Writes
/// `x16` (a far frame offset).
pub fn reload(cx: &mut FnCx<'_, A64>, reg: VReg, shape: &Shape, image: FrameRef) {
    let mut at = Cursor::frame(cx, image, X16);
    load_fpr_at(cx, view(shape), reg, &mut at);
}

/// Where the image of vector operand `value` is: the slot of a `Mem`
/// vector (a constant written to a slot of its own), or, for an `Fpr`
/// vector, its register (asked for, and pinned) stored into scratch image
/// `k`.
///
/// # Errors
/// What asking for the operand reports; as [`scratch`].
pub fn image(cx: &mut FnCx<'_, A64>, value: Value, k: u32) -> CgResult<FrameRef> {
    let ty = cx.body.value_type(cx.module, value);
    let Some(shape) = Shape::of(cx.types(), ty) else {
        return cx.bug("the image of something that is no vector");
    };
    if shape.class == Class::Mem {
        return cx.use_mem(value);
    }
    let reg = vr(cx.use_reg(value)?);
    spill(cx, reg, &shape, k)
}

/// Writes `value`, a vector of class `Mem`, into slot `dst`: a constant
/// by its image, a value by copying its store size. Writes `x16`, `x17`
/// and `v31`.
///
/// # Errors
/// What writing the constant or asking for the operand reports.
pub fn write_mem(cx: &mut FnCx<'_, A64>, dst: FrameRef, value: Value) -> CgResult<()> {
    if matches!(value.kind(), ValueKind::Const(_)) {
        return materialize_mem_const(cx, dst, value);
    }
    let ty = cx.body.value_type(cx.module, value);
    let Ok(bytes) = u32::try_from(cx.types().size_of(ty)) else {
        return unsupported(hod_cg::LIMIT_FRAME_SIZE);
    };
    let src = cx.use_mem(value)?;
    if src != dst {
        let mut from = Cursor::frame(cx, src, X16);
        let mut to = Cursor::frame(cx, dst, X17);
        copy_bytes(cx, &mut to, &mut from, bytes);
    }
    Ok(())
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
/// decided here, a variable is asked for (and pinned). Emits nothing for
/// a constant; asking may emit (see the module docs: ask before
/// spilling).
///
/// # Errors
/// [`INDEX`] for an index wider than 64 bits; what asking reports.
pub fn index(cx: &mut FnCx<'_, A64>, value: Value, lanes: &Lanes) -> CgResult<Index> {
    let ty = cx.body.value_type(cx.module, value);
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
            Ok(lane) if lane < lanes.count() => Index::Lane(lane),
            _ => Index::Poison,
        });
    }
    let reg = xr(cx.use_reg(value)?);
    Ok(Index::Reg { reg, bits })
}

/// Puts the lane number of the variable index `reg` (`bits` wide) in
/// `rd`, inside a vector of `count` lanes (the module docs' rule): masked
/// with `count - 1` for a power of two, else `≥ count` clamped to 0.
/// Writes `rd` only.
pub fn lane_number(cx: &mut FnCx<'_, A64>, rd: Reg, reg: Reg, bits: u32, count: u32) {
    if count.is_power_of_two() {
        let width_mask = if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 };
        let mask = u64::from(count - 1) & width_mask;
        if mask == 0 {
            put(cx, mc::mov(Size::X, rd.zr(), mc::RegZr::ZR));
        } else {
            put(cx, mc::and_imm(Size::X, rd.sp(), reg.zr(), mask));
        }
        return;
    }
    if bits < 64 {
        put(cx, mc::ubfx(Size::X, rd.zr(), reg.zr(), 0, bits));
    } else {
        put(cx, mc::mov(Size::X, rd.zr(), reg.zr()));
    }
    put(cx, mc::cmp_imm(Size::X, rd.sp(), u64::from(count)));
    put(cx, mc::csel(Size::X, rd.zr(), rd.zr(), mc::RegZr::ZR, Cond::Lo));
}

/// A cursor at the lane `index` names in the image at `image`: for a
/// constant, the lane's frame place (lane 0 for [`Index::Poison`]); for
/// a variable, `x16` = the image's address plus the lane number
/// ([`lane_number`], in `x17`) times the lane size. Writes `x16` and
/// `x17` only; the cursor rebases into `x16`.
pub fn lane_cursor(cx: &mut FnCx<'_, A64>, image: FrameRef, lanes: &Lanes, index: Index) -> Cursor {
    match index {
        Index::Lane(lane) => Cursor::frame(cx, offset(image, lanes.offset(lane)), X16),
        Index::Poison => Cursor::frame(cx, image, X16),
        Index::Reg { reg, bits } => {
            lane_number(cx, X17, reg, bits, lanes.count());
            frame_address(cx, X16, image);
            put(cx, mc::add_shifted(Size::X, X16.zr(), X16.zr(), X17.zr(), Shift::Lsl, lanes.log2));
            Cursor::at_reg(X16, 0, X16)
        }
    }
}

/// Loads one lane at the cursor into `rt`: a general register (the lane
/// zero-extended) or a vector register (its scalar view).
pub fn load_lane(cx: &mut FnCx<'_, A64>, lanes: &Lanes, rt: PReg, at: &mut Cursor) {
    if reg::is_fpr(rt) {
        load_fpr_at(cx, lanes.fpr_view(), vr(rt), at);
    } else {
        load_gpr_at(cx, lanes.gpr_load(), xr(rt), at);
    }
}

/// Stores one lane from `rt` (a general register's low bytes or a vector
/// register's scalar view) at the cursor.
pub fn store_lane(cx: &mut FnCx<'_, A64>, lanes: &Lanes, rt: PReg, at: &mut Cursor) {
    if reg::is_fpr(rt) {
        store_fpr_at(cx, lanes.fpr_view(), vr(rt), at);
    } else {
        store_gpr_at(cx, lanes.gpr_store(), xr(rt).zr(), at);
    }
}

// ---- element access ----

/// `%r = extractelement %vector, %index`: for an `Fpr` vector and a
/// constant index, `umov` (a general result) or `mov` (DUP element,
/// scalar: a floating-point result) of the lane; otherwise one load of
/// the lane from the vector's image (an `Fpr` vector spilled to scratch
/// image 0). An index out of range reads lane 0 (a constant) or the
/// masked or clamped lane (a variable).
///
/// # Errors
/// What [`lanes`] and [`index`] refuse; what asking for the operands
/// reports.
pub fn extract_element<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    vector: Value,
    at: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let lanes = lanes(cx, value_ty(cx, vector))?;
    let idx = index(cx, at, &lanes)?;
    let image = if lanes.shape.class == Class::Fpr {
        let v = vr(cx.use_reg(vector)?);
        let rd = cx.def_reg(inst)?;
        if let Index::Lane(_) | Index::Poison = idx {
            let lane = if let Index::Lane(lane) = idx { lane } else { 0 };
            let word = if reg::is_fpr(rd) {
                mc::dup_scalar(lanes.elem(), vr(rd), v, lane)
            } else {
                mc::umov(lanes.elem(), xr(rd).zr(), v, lane)
            };
            put(cx, word);
            return cx.finish(inst, rd);
        }
        let image = spill(cx, v, &lanes.shape, 0)?;
        let mut cursor = lane_cursor(cx, image, &lanes, idx);
        load_lane(cx, &lanes, rd, &mut cursor);
        return cx.finish(inst, rd);
    } else {
        cx.use_mem(vector)?
    };
    let rd = cx.def_reg(inst)?;
    let mut cursor = lane_cursor(cx, image, &lanes, idx);
    load_lane(cx, &lanes, rd, &mut cursor);
    cx.finish(inst, rd)
}

/// `%r = insertelement %vector, %value, %index`: for an `Fpr` vector and
/// a constant index, `ins` (from a general or a vector register) into the
/// vector's register; for a variable index, the register spilled to
/// scratch image 0, the lane stored, the image reloaded. A `Mem` vector
/// is copied to the result's slot and the lane stored there. An index out
/// of range writes nothing (a constant) or the masked or clamped lane (a
/// variable): the result is poison either way.
///
/// # Errors
/// What [`lanes`] and [`index`] refuse; what asking for the operands
/// reports.
pub fn insert_element<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    vector: Value,
    value: Value,
    at: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let lanes = lanes(cx, inst.ty())?;
    let idx = index(cx, at, &lanes)?;
    let e = cx.use_reg(value)?;
    if lanes.shape.class == Class::Fpr {
        let v = cx.use_reg(vector)?;
        match idx {
            Index::Lane(lane) => {
                let word = if reg::is_fpr(e) {
                    mc::ins_elem(lanes.elem(), vr(v), lane, vr(e), 0)
                } else {
                    mc::ins_gpr(lanes.elem(), vr(v), lane, xr(e).zr())
                };
                put(cx, word);
            }
            Index::Poison => {}
            Index::Reg { .. } => {
                let image = spill(cx, vr(v), &lanes.shape, 0)?;
                let mut cursor = lane_cursor(cx, image, &lanes, idx);
                store_lane(cx, &lanes, e, &mut cursor);
                reload(cx, vr(v), &lanes.shape, image);
            }
        }
        return cx.finish(inst, v);
    }
    let r = cx.def_mem(inst)?;
    write_mem(cx, r, vector)?;
    if idx != Index::Poison {
        let mut cursor = lane_cursor(cx, r, &lanes, idx);
        store_lane(cx, &lanes, e, &mut cursor);
    }
    Ok(())
}

/// Where lane `entry` of a shuffle's concatenated sources is: source 0 or
/// 1 and the lane in it; `None` for an undefined entry (or one past both
/// sources, which the verifier rejects).
fn source(entry: u32, n: u32) -> Option<(usize, u32)> {
    if entry == SHUFFLE_UNDEF || entry >= 2 * n {
        None
    } else if entry < n {
        Some((0, entry))
    } else {
        Some((1, entry - n))
    }
}

/// An operand of a shuffle: its register (an `Fpr` vector) or its image
/// (a `Mem` vector).
#[derive(Clone, Copy)]
enum Operand {
    Reg(VReg),
    Image(FrameRef),
}

/// `%r = shufflevector %first, %second, <mask>`: lane by lane, each
/// result lane the source lane its mask entry names, an undefined one
/// zero. `Fpr` result: `dup` for a splat of a register source, else the
/// lanes inserted into `v31` (`ins` element, or a load into `x16` then
/// `ins` for a `Mem` source) and moved to an operand's register (which
/// dies here) or a new one. `Mem` result: each lane through `x16`
/// (`umov` or a load), then stored. A source no entry names is not asked
/// for. Writes `x16`, `x17` and `v31`.
///
/// # Errors
/// What [`lanes`] refuses for either type; what asking for the operands
/// reports.
pub fn shuffle_vector<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    first: Value,
    second: Value,
    mask: &[u32],
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let src = lanes(cx, value_ty(cx, first))?;
    let dst = lanes(cx, inst.ty())?;
    let n = src.count();
    let mut used = [false; 2];
    for &entry in mask {
        if let Some((which, _)) = source(entry, n) {
            used[which] = true;
        }
    }
    let mut ops = [None; 2];
    let mut operand_reg: Option<PReg> = None;
    for (which, value) in [first, second].into_iter().enumerate() {
        if !used[which] {
            continue;
        }
        if which == 1 && second == first && ops[0].is_some() {
            ops[1] = ops[0];
            continue;
        }
        ops[which] = Some(if src.shape.class == Class::Fpr {
            let reg = cx.use_reg(value)?;
            operand_reg.get_or_insert(reg);
            Operand::Reg(vr(reg))
        } else {
            Operand::Image(cx.use_mem(value)?)
        });
    }
    let lane_of = |entry: u32| -> Option<(Operand, u32)> {
        let (which, lane) = source(entry, n)?;
        Some((ops[which]?, lane))
    };
    let elem = src.elem();
    if dst.shape.class == Class::Fpr {
        // The result is built in `v31` and left in an operand's register
        // (which dies here, `FnCx::use_reg`), so that two register
        // sources and the result fit the smallest pool.
        let rd = match operand_reg {
            Some(reg) => reg,
            None => cx.def_reg(inst)?,
        };
        let defined: Option<u32> = mask.iter().copied().find(|&e| source(e, n).is_some());
        let splat = defined
            .filter(|&e| mask.iter().all(|&m| source(m, n).is_none() || m == e))
            .and_then(lane_of);
        if let Some((Operand::Reg(v), lane)) = splat {
            put(cx, mc::dup_elem(dst.arr(), vr(rd), v, lane));
            return cx.finish(inst, rd);
        }
        let d = V31;
        if mask.iter().any(|&m| source(m, n).is_none()) {
            put(cx, mc::movi_mask(VecBits::V128, d, 0));
        }
        for (k, &entry) in (0u32..).zip(mask) {
            match lane_of(entry) {
                None => {}
                Some((Operand::Reg(v), lane)) => put(cx, mc::ins_elem(elem, d, k, v, lane)),
                Some((Operand::Image(image), lane)) => {
                    let mut cursor = Cursor::frame(cx, offset(image, src.offset(lane)), X16);
                    load_gpr_at(cx, src.gpr_load(), X16, &mut cursor);
                    put(cx, mc::ins_gpr(elem, d, k, X16.zr()));
                }
            }
        }
        put(cx, mc::orr_vec(VecBits::V128, vr(rd), d, d));
        return cx.finish(inst, rd);
    }
    let r = cx.def_mem(inst)?;
    for (k, &entry) in (0u32..).zip(mask) {
        let rt = match lane_of(entry) {
            None => mc::RegZr::ZR,
            Some((Operand::Reg(v), lane)) => {
                put(cx, mc::umov(elem, X16.zr(), v, lane));
                X16.zr()
            }
            Some((Operand::Image(image), lane)) => {
                let mut cursor = Cursor::frame(cx, offset(image, src.offset(lane)), X16);
                load_gpr_at(cx, src.gpr_load(), X16, &mut cursor);
                X16.zr()
            }
        };
        let mut cursor = Cursor::frame(cx, offset(r, dst.offset(k)), X17);
        store_gpr_at(cx, dst.gpr_store(), rt, &mut cursor);
    }
    Ok(())
}

// ---- calls ----

/// Whether `ty` is, or has as a leaf, a vector of class `Mem`.
fn has_mem_vector(types: &TypeTable, ty: TypeId, depth: u32) -> bool {
    match types.kind(ty) {
        // A mask (class `Gpr`, WP-081) is passed lane by lane: WP-082.
        TypeKind::Vector { elem, .. } => {
            elem == TypeId::I1 || A64.class_of(types, ty) == Class::Mem
        }
        TypeKind::Array { elem, .. } => depth < 64 && has_mem_vector(types, elem, depth + 1),
        TypeKind::Struct { .. } => types.struct_fields(ty).is_some_and(|fields| {
            fields.iter().any(|&f| depth < 64 && has_mem_vector(types, f, depth + 1))
        }),
        _ => false,
    }
}

/// The vectors a signature may pass (the hook in `A64::assign`): those
/// of class `Fpr`, which `abi::assign` places in `v0`..`v7` (or on the
/// stack) as `llc` does; a vector of class `Mem` anywhere in the
/// signature is [`CALL`] until WP-082.
///
/// # Errors
/// [`CALL`].
pub fn check_signature(types: &TypeTable, sig: &Sig<'_>) -> Result<(), AbiUnsupported> {
    let refused = |ty: TypeId| ty != TypeId::VOID && has_mem_vector(types, ty, 0);
    if refused(sig.ret) || sig.params.iter().any(|&ty| refused(ty)) {
        return Err(AbiUnsupported(CALL));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TypeTable {
        TypeTable::new(hod_ir::TargetSpec::aarch64_linux_gnu().data_layout())
    }

    /// The shapes are `hod_ir`'s layout and the class of
    /// `target::vector_class`.
    #[test]
    fn shapes_follow_the_layout() {
        let mut t = table();
        let cases = [
            (TypeId::I32, 4, Class::Fpr, 16, 16, 16),
            (TypeId::I32, 2, Class::Fpr, 8, 8, 8),
            (TypeId::I32, 3, Class::Mem, 12, 16, 16),
            (TypeId::I8, 4, Class::Mem, 4, 4, 4),
            (TypeId::I64, 4, Class::Mem, 32, 32, 32),
            (TypeId::DOUBLE, 1, Class::Fpr, 8, 8, 8),
            (TypeId::I1, 8, Class::Gpr, 1, 1, 1),
        ];
        for (elem, len, class, size, alloc, align) in cases {
            let ty = t.vector(elem, len).unwrap();
            let s = Shape::of(&t, ty).unwrap();
            assert_eq!((s.class, s.size, s.alloc, s.align), (class, size, alloc, align), "{len}");
            assert_eq!(s.lanes, len);
        }
        assert!(Shape::of(&t, TypeId::I32).is_none());
    }

    /// The refusals of §4.4 by shape.
    #[test]
    fn lane_refusals_by_name() {
        let mut t = table();
        let i4 = t.int(4).unwrap();
        let i128 = t.int(128).unwrap();
        let cases = [
            (TypeId::I1, 8, MASK),
            (TypeId::I1, 64, MASK),
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
        let ty = t.vector(TypeId::I8, 256).unwrap();
        let l = Shape::of(&t, ty).unwrap().lanes().unwrap();
        assert_eq!((l.count(), l.bytes(), l.offset(255)), (256, 1, 255));
        let ty = t.vector(TypeId::DOUBLE, 3).unwrap();
        let l = Shape::of(&t, ty).unwrap().lanes().unwrap();
        assert_eq!((l.bytes(), l.offset(2), l.elem_class()), (8, 16, Class::Fpr));
    }

    #[test]
    fn arrangements() {
        let mut t = table();
        for (elem, len, arr) in [
            (TypeId::I8, 8, Arr::B8),
            (TypeId::I8, 16, Arr::B16),
            (TypeId::HALF, 4, Arr::H4),
            (TypeId::I16, 8, Arr::H8),
            (TypeId::FLOAT, 2, Arr::S2),
            (TypeId::I32, 4, Arr::S4),
            (TypeId::I64, 1, Arr::D2),
            (TypeId::DOUBLE, 2, Arr::D2),
        ] {
            let ty = t.vector(elem, len).unwrap();
            assert_eq!(Shape::of(&t, ty).unwrap().lanes().unwrap().arr(), arr);
        }
    }
}
