//! The lane loop: element-wise operations on vectors one lane at a time
//! (`spec/codegen.md` §7.3 "The lane loop"; design 0006 §1.2, R3).
//!
//! **Owner: WP-129.** Tests: the unit tests below and `hod_a64`'s
//! `tests/lanes.rs`, `tests/mask.rs`.
//!
//! What is decided here is the same on every target: which vector shapes
//! have addressable lanes ([`lane_log2`]), where each operand's image is
//! (its slot, or a register vector stored into a scratch image), where the
//! result is written, which lanes are visited in which order, which
//! operand lanes are loaded into which temporaries, and what is given
//! back after each lane. What is emitted is the target's: it implements
//! [`LaneKernels`] (images and one-lane moves) and passes the scalar
//! operation of one lane to [`each_lane`] as a closure over the
//! temporaries.
//!
//! The driver emits nothing itself and keeps nothing in a register from
//! one kernel call to the next but frame images: a kernel may clobber its
//! target's free scratch registers and the flags, and it may compute its
//! result in any register it returns (a two-operand instruction leaves it
//! in the first operand).

use hod_ir::{Inst, TypeId, TypeKind, TypeTable, Value};

use crate::{CgResult, Class, FnCx, FrameRef, PReg, Target};

/// The most lanes a lane operation handles: its code is unrolled at emit
/// time, one access per lane.
pub const MAX_LANES: u32 = 256;

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
    /// The element of type `ty`, if a vector may have it.
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

    /// The class of one lane as a scalar value: `Gpr` for integers and
    /// pointers, `Fpr` for floating-point elements.
    pub fn class(self) -> Class {
        if self.is_float() { Class::Fpr } else { Class::Gpr }
    }
}

/// Why a vector's lanes are not addressable. The target refuses each
/// under a name of its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeRefusal {
    /// `i1` elements, at most 64 of them: a mask.
    Mask,
    /// More than 64 `i1` elements.
    MaskWide,
    /// More than [`MAX_LANES`] lanes.
    Length,
    /// Elements wider than 64 bits (`i128`, `fp128`).
    WideElement,
    /// Integers of a width that is not 8, 16, 32 or 64 bits.
    OddElement,
}

/// log2 of the lane size in bytes (0 to 3) of a vector of `count`
/// elements `elem`, or why its lanes are not addressable; checked in
/// this order: masks, the length, the element's width.
///
/// # Errors
/// [`ShapeRefusal`].
pub fn lane_log2(elem: Elem, count: u32) -> Result<u32, ShapeRefusal> {
    if elem == Elem::Int(1) {
        return Err(if count > 64 { ShapeRefusal::MaskWide } else { ShapeRefusal::Mask });
    }
    if count > MAX_LANES {
        return Err(ShapeRefusal::Length);
    }
    match elem.bits() {
        8 => Ok(0),
        16 => Ok(1),
        32 => Ok(2),
        64 => Ok(3),
        bits if bits > 64 => Err(ShapeRefusal::WideElement),
        _ => Err(ShapeRefusal::OddElement),
    }
}

/// What the driver needs of a vector with addressable lanes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Geometry {
    /// The number of lanes.
    pub count: u32,
    /// log2 of the lane size in bytes (0 to 3).
    pub log2: u32,
    /// The vector's class: `Fpr` (a register vector) or `Mem`.
    pub class: Class,
    /// The class of one lane as a scalar value.
    pub elem_class: Class,
}

impl Geometry {
    /// The lane size in bytes.
    pub fn bytes(&self) -> u32 {
        1 << self.log2
    }

    /// The byte offset of lane `lane` in the image (vector elements are
    /// packed).
    pub fn offset(&self, lane: u32) -> u32 {
        lane << self.log2
    }
}

/// What a target provides to the lane loop: where register vectors'
/// images go, and the moves of one lane. Every method emits through the
/// target's encoder; none may leave a value in a pool register.
pub trait LaneKernels: Target + Sized {
    /// The target's view of a vector with addressable lanes (it carries
    /// what the target's moves need: arrangements, load widths).
    type Lanes: Copy;

    /// How many scratch images a lowering may use at once.
    const SCRATCH_IMAGES: u32;

    /// The driver's view of `lanes`.
    fn geometry(lanes: &Self::Lanes) -> Geometry;

    /// Emits the deferred operands of `inst` (§4.6) before anything else.
    ///
    /// # Errors
    /// What asking for them reports.
    fn deferred_first<'a>(cx: &mut FnCx<'a, Self>, inst: Inst<'a>) -> CgResult<()>;

    /// Scratch image `k` (below [`LaneKernels::SCRATCH_IMAGES`]): frame
    /// memory of the function as large and aligned as the widest register
    /// vector, valid until the lowering ends.
    ///
    /// # Errors
    /// `limit:frame-size`.
    fn scratch(cx: &mut FnCx<'_, Self>, k: u32) -> CgResult<FrameRef>;

    /// Stores the register vector of `lanes` in `reg` into scratch image
    /// `k` and returns the image.
    ///
    /// # Errors
    /// As [`LaneKernels::scratch`].
    fn spill(cx: &mut FnCx<'_, Self>, reg: PReg, lanes: &Self::Lanes, k: u32)
    -> CgResult<FrameRef>;

    /// Loads the register vector of `lanes` from `image` into `reg`.
    fn reload(cx: &mut FnCx<'_, Self>, reg: PReg, lanes: &Self::Lanes, image: FrameRef);

    /// Loads the lane at frame place `at` into `rt`: a general register
    /// gets the lane zero-extended, a floating-point one its scalar view.
    fn load_lane(cx: &mut FnCx<'_, Self>, lanes: &Self::Lanes, at: FrameRef, rt: PReg);

    /// Stores `rt` (a general register's low bytes, a floating-point
    /// register's scalar view) as the lane at frame place `at`.
    fn store_lane(cx: &mut FnCx<'_, Self>, lanes: &Self::Lanes, at: FrameRef, rt: PReg);
}

/// An operand of a lane loop.
#[derive(Clone, Copy, Debug)]
pub enum Arg<L> {
    /// A vector: where its image is, and its lanes.
    Vector(FrameRef, L),
    /// A scalar, the same in every lane.
    Scalar(Value),
}

/// The operands and the result of a lane loop, made by [`setup`].
#[derive(Clone, Copy, Debug)]
pub struct LaneLoop<L> {
    /// The operands, in order (unused ones are `Scalar(Value::NONE)`).
    pub args: [Arg<L>; 3],
    /// The result's lanes.
    pub result: L,
    /// Where the result's image is written.
    pub dst: FrameRef,
}

/// The images of the operands `values`: `shapes[i]` is `None` for a
/// scalar operand, which stays one. An operand repeated shares its first
/// image. A `Mem` vector's image is its slot; a register vector is asked
/// for, stored into the next scratch image, and given back at once (so
/// the loop has the whole pool). Writes `out[i]` for each operand and
/// returns the number of scratch images used.
///
/// # Errors
/// What asking for the operands reports; as [`LaneKernels::scratch`].
pub fn images<T: LaneKernels>(
    cx: &mut FnCx<'_, T>,
    values: &[Value],
    shapes: &[Option<T::Lanes>],
    out: &mut [Arg<T::Lanes>],
) -> CgResult<u32> {
    let mut k = 0;
    for (i, &v) in values.iter().enumerate() {
        let Some(lanes) = shapes[i] else {
            out[i] = Arg::Scalar(v);
            continue;
        };
        if let Some(j) = values[..i].iter().position(|&w| w == v) {
            out[i] = out[j];
            continue;
        }
        let image = if T::geometry(&lanes).class == Class::Fpr {
            let r = cx.use_reg(v)?;
            let image = T::spill(cx, r, &lanes, k)?;
            k += 1;
            cx.release(v);
            cx.free_temp(r);
            image
        } else {
            cx.use_mem(v)?
        };
        out[i] = Arg::Vector(image, lanes);
    }
    Ok(k)
}

/// Checks the shapes of `inst` (the result) and of its `operands` (at
/// most three; `lanes_of` gives a vector's lanes or the target's refusal,
/// a scalar operand is not asked), then, after the deferred operands,
/// finds the operands' images ([`images`]) and the result's: its slot
/// (`Mem`), else the next free scratch image, else the first operand's
/// image when its lanes are as wide as the result's (every lane of it is
/// read before that lane is written).
///
/// # Errors
/// What `lanes_of` refuses; `Internal` for more than three operands or
/// operands of another length than the result; as [`images`].
pub fn setup<'a, T: LaneKernels>(
    cx: &mut FnCx<'a, T>,
    inst: Inst<'a>,
    operands: &[Value],
    lanes_of: impl Fn(&FnCx<'a, T>, TypeId) -> CgResult<T::Lanes>,
) -> CgResult<LaneLoop<T::Lanes>> {
    if operands.len() > 3 {
        return cx.bug("a lane operation of more than three operands");
    }
    let result = lanes_of(cx, inst.ty())?;
    let count = T::geometry(&result).count;
    let mut shapes: [Option<T::Lanes>; 3] = [None; 3];
    for (shape, &v) in shapes.iter_mut().zip(operands) {
        let ty = cx.body.value_type(cx.module, v);
        if cx.types().is_vector(ty) {
            let lanes = lanes_of(cx, ty)?;
            if T::geometry(&lanes).count != count {
                return cx.bug("a lane operation on vectors of different lengths");
            }
            *shape = Some(lanes);
        }
    }
    T::deferred_first(cx, inst)?;
    let mut args = [Arg::Scalar(Value::NONE); 3];
    let used = images(cx, operands, &shapes[..operands.len()], &mut args)?;
    let geometry = T::geometry(&result);
    let dst = if geometry.class != Class::Fpr {
        cx.def_mem(inst)?
    } else if used < T::SCRATCH_IMAGES {
        T::scratch(cx, used)?
    } else if let Arg::Vector(image, lanes) = args[0]
        && T::geometry(&lanes).bytes() == geometry.bytes()
    {
        image
    } else {
        return cx.bug("no scratch image for the result of a lane operation");
    };
    Ok(LaneLoop { args, result, dst })
}

/// Loads lane `lane` of `arg` into `rt` (a scalar operand: its value).
///
/// # Errors
/// What putting a scalar operand reports.
pub fn load<T: LaneKernels>(
    cx: &mut FnCx<'_, T>,
    arg: Arg<T::Lanes>,
    lane: u32,
    rt: PReg,
) -> CgResult<()> {
    match arg {
        Arg::Vector(image, lanes) => {
            let at = image.offset_by(T::geometry(&lanes).offset(lane));
            T::load_lane(cx, &lanes, at, rt);
            Ok(())
        }
        Arg::Scalar(v) => cx.put(v, rt),
    }
}

/// The class of a lane of `arg` (of a scalar operand: its own).
pub fn class_of<T: LaneKernels>(cx: &FnCx<'_, T>, arg: Arg<T::Lanes>) -> Class {
    match arg {
        Arg::Vector(_, lanes) => T::geometry(&lanes).elem_class,
        Arg::Scalar(v) => cx.class_of(v),
    }
}

/// The most operand lanes one kernel call takes.
pub const MAX_TAKES: usize = 4;

/// The loop: for each lane in order, 0 first, each `(operand, class)` of
/// `takes` loaded into a new pool temporary of `class`, in the order
/// given ([`load`]); then `kernel(cx, lane, temporaries)`, which emits the
/// scalar operation and returns the register holding the result lane
/// (one of the temporaries, or a temporary of its own); that register
/// stored as lane `lane` of the result's image; then the result's
/// register and every taken temporary given back. A kernel that makes a
/// call (every temporary ends there) returns a register taken after it.
///
/// # Errors
/// What loading, the kernel and the pool report; `Internal` for more
/// than [`MAX_TAKES`] takes.
pub fn each_lane<'a, T: LaneKernels>(
    cx: &mut FnCx<'a, T>,
    lp: &LaneLoop<T::Lanes>,
    takes: &[(usize, Class)],
    mut kernel: impl FnMut(&mut FnCx<'a, T>, u32, &[PReg]) -> CgResult<PReg>,
) -> CgResult<()> {
    if takes.len() > MAX_TAKES {
        return cx.bug("a lane kernel of too many operands");
    }
    let result = T::geometry(&lp.result);
    for lane in 0..result.count {
        let mut regs = [PReg::new(0); MAX_TAKES];
        for (slot, &(arg, class)) in regs.iter_mut().zip(takes) {
            let t = cx.temp(class)?;
            load(cx, lp.args[arg], lane, t)?;
            *slot = t;
        }
        let regs = &regs[..takes.len()];
        let rt = kernel(cx, lane, regs)?;
        let at = lp.dst.offset_by(result.offset(lane));
        T::store_lane(cx, &lp.result, at, rt);
        cx.free_temp(rt);
        for &t in regs {
            cx.free_temp(t);
        }
    }
    Ok(())
}

/// The end of a loop that wrote the image `dst` of a result of `lanes`:
/// a register vector is loaded from it into the result's register.
///
/// # Errors
/// What defining the result reports.
pub fn finish<'a, T: LaneKernels>(
    cx: &mut FnCx<'a, T>,
    inst: Inst<'a>,
    lanes: &T::Lanes,
    dst: FrameRef,
) -> CgResult<()> {
    if T::geometry(lanes).class != Class::Fpr {
        return Ok(());
    }
    let rd = cx.def_reg(inst)?;
    T::reload(cx, rd, lanes, dst);
    cx.finish(inst, rd)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7.3 "The lane loop": the shapes whose lanes are addressable, and
    /// the order of the refusals.
    #[test]
    fn lane_sizes_and_refusals() {
        use ShapeRefusal as R;
        assert_eq!(lane_log2(Elem::Int(8), 256), Ok(0));
        assert_eq!(lane_log2(Elem::Half, 4), Ok(1));
        assert_eq!(lane_log2(Elem::BFloat, 4), Ok(1));
        assert_eq!(lane_log2(Elem::Float, 3), Ok(2));
        assert_eq!(lane_log2(Elem::Ptr, 2), Ok(3));
        assert_eq!(lane_log2(Elem::Double, 1), Ok(3));
        assert_eq!(lane_log2(Elem::Int(1), 64), Err(R::Mask));
        assert_eq!(lane_log2(Elem::Int(1), 65), Err(R::MaskWide));
        assert_eq!(lane_log2(Elem::Int(1), 1000), Err(R::MaskWide));
        assert_eq!(lane_log2(Elem::Int(8), 257), Err(R::Length));
        assert_eq!(lane_log2(Elem::Int(128), 257), Err(R::Length));
        assert_eq!(lane_log2(Elem::Int(128), 2), Err(R::WideElement));
        assert_eq!(lane_log2(Elem::Fp128, 2), Err(R::WideElement));
        assert_eq!(lane_log2(Elem::Int(24), 2), Err(R::OddElement));
        assert_eq!(lane_log2(Elem::Int(4), 16), Err(R::OddElement));
    }

    #[test]
    fn geometry_and_elements() {
        let g = Geometry { count: 3, log2: 3, class: Class::Mem, elem_class: Class::Fpr };
        assert_eq!((g.bytes(), g.offset(0), g.offset(2)), (8, 0, 16));
        assert_eq!((Elem::Ptr.class(), Elem::Half.class()), (Class::Gpr, Class::Fpr));
        assert_eq!((Elem::Ptr.bits(), Elem::Fp128.bits(), Elem::Int(7).bits()), (64, 128, 7));
    }
}
