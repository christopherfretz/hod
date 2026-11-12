//! Vector instructions and intrinsics (`spec/interp.md` §3.1, §4, §5.10).
//!
//! Each element-wise operation applies the scalar operation of §5 to each
//! lane through the same kernels (`ops`, [`Machine::cast`], the NaN policy)
//! and keeps a poison flag per lane: an operation on a poison lane makes
//! only that lane poison, and what the scalar rules call poison (a shift by
//! the width or more, an `fptosi` that does not fit, `ctlz(0, true)`)
//! makes the lane poison instead of trapping. Undefined behavior in any
//! lane (a division by zero, or by a poison lane) traps as for a scalar. A
//! poison lane traps `poison-lane` only where it would be observed.

// Lanes are `x`, `y`, `z`, vectors `a`, `b`, as in the scalar code.
#![allow(clippy::many_single_char_names)]

use hod_ir::{
    CallSite, FloatPredicate, Inst, InstKind, IntPredicate, Intrinsic, Opcode, SHUFFLE_UNDEF,
    TypeId, TypeKind, Value,
};

use super::{Class, Flow, Frame, Machine, NanPolicy, decode, encode};
use crate::error::{Stop, Trap, invalid, unsupported};
use crate::ops::{self, IntOp};
use crate::value::{Val, mask, sext};
use crate::vector::{Lane, Vector, image_size};

/// The result of an operation on one lane: its bits, or poison.
type LaneResult = Result<Option<u128>, Stop>;

impl<'a> Machine<'_, 'a> {
    // ---- types and images ----

    /// The class of vector type `ty`, or why it is unsupported (§8).
    pub(super) fn vector_class(&self, ty: TypeId) -> Result<Class, Stop> {
        let types = &self.module.types;
        let TypeKind::Vector { elem, len } = types.kind(ty) else {
            return invalid(format!("{ty:?} is not a vector type"));
        };
        let lane = match Lane::of(types, elem) {
            Ok(lane) => lane,
            Err(reason) => return unsupported(reason),
        };
        if len == 0 {
            return invalid("a vector of no elements");
        }
        let size = image_size(lane, len) as u64;
        if size > self.config.max_memory || size > u64::from(u32::MAX) {
            return Err(Trap::Memory.into());
        }
        Ok(Class::Vector(ty, size as u32))
    }

    /// The lane kind and count of vector type `ty`.
    fn shape(&self, ty: TypeId) -> Result<(Lane, u32), Stop> {
        self.vector_class(ty)?;
        match self.module.types.kind(ty) {
            TypeKind::Vector { elem, len } => match Lane::of(&self.module.types, elem) {
                Ok(lane) => Ok((lane, len)),
                Err(reason) => unsupported(reason),
            },
            _ => invalid(format!("{ty:?} is not a vector type")),
        }
    }

    /// The element type of vector type `ty`.
    fn elem_ty(&self, ty: TypeId) -> TypeId {
        self.module.types.element_type(ty).unwrap_or(ty)
    }

    /// A vector of type `ty` from its image, or zero. The bits above the
    /// lanes must be zero.
    pub(super) fn vector_from(&self, ty: TypeId, image: Option<&[u8]>) -> Result<Vector, Stop> {
        let (lane, len) = self.shape(ty)?;
        let Some(image) = image else {
            return Ok(Vector::zeroed(ty, lane, len));
        };
        let Some(vector) = Vector::from_bytes(ty, lane, len, image) else {
            return invalid("a vector image of the wrong size");
        };
        if !vector.padding_is_zero() {
            return Err(Trap::NonZeroPadding.into());
        }
        Ok(vector)
    }

    fn vector(&self, f: &Frame<'a>, value: Value) -> Result<Vector, Stop> {
        match self.get(f, value)? {
            Val::Agg(vector) if vector.is_vector() => Ok(vector),
            other => invalid(format!("{other} where a vector is required")),
        }
    }

    fn is_vector_value(&self, f: &Frame<'a>, value: Value) -> bool {
        self.module.types.is_vector(f.body.value_type(self.module, value))
    }

    // ---- memory (§4) ----

    /// A load of a vector: a lane with a byte that was never written is
    /// poison, not a trap (the load of such a byte is `undef` by the
    /// language reference's `load`, here per lane).
    pub(super) fn vector_load(&self, ty: TypeId, addr: u64, align: u64) -> Result<Val, Stop> {
        let (lane, len) = self.shape(ty)?;
        let size = image_size(lane, len);
        let at = self.mem.check(addr, size as u64, align, false)?;
        let mut vector = Vector::from_bytes(ty, lane, len, self.mem.raw(at, size))
            .ok_or_else(|| Stop::Invalid("a vector image of the wrong size".into()))?;
        let last = size - 1;
        if self.mem.is_written(at + last, 1) && !vector.padding_is_zero() {
            return Err(Trap::NonZeroPadding.into());
        }
        if !self.mem.is_written(at, size) {
            for index in 0..len {
                let range = vector.byte_range(index);
                if !self.mem.is_written(at + range.start, range.len()) {
                    vector.set_poison(index);
                }
            }
        }
        Ok(Val::Agg(vector))
    }

    /// A store of a vector: its image; the bytes of a poison lane end up
    /// unwritten (for a mask, every byte that holds a poison bit).
    pub(super) fn vector_store(
        &mut self,
        vector: &Vector,
        addr: u64,
        align: u64,
    ) -> Result<(), Stop> {
        let size = vector.bytes.len();
        let at = self.mem.check(addr, size as u64, align, true)?;
        self.mem.put(at, &vector.bytes);
        if vector.has_poison() {
            for index in (0..vector.count()).filter(|&index| vector.is_poison(index)) {
                let range = vector.byte_range(index);
                self.mem.set_written(at + range.start, range.len(), false);
            }
        }
        Ok(())
    }

    // ---- bitcasts (§5.3) ----

    /// `bitcast` of a vector to a scalar: the image read as the scalar. A
    /// poison lane makes the whole scalar poison, which is observed.
    pub(super) fn vector_to_scalar(&self, vector: &Vector, ty: TypeId) -> Result<Val, Stop> {
        let class = self.class(ty)?;
        if matches!(class, Class::Void | Class::Agg | Class::Vector(..))
            || class.store_len() != vector.bytes.len()
            || self.module.types.size_in_bits_of(ty)
                != u64::from(vector.count()) * u64::from(vector.kind().bits())
        {
            return invalid(format!("bitcast of {} to {ty:?}", Val::Agg(vector.clone())));
        }
        if vector.has_poison() {
            return Err(Trap::PoisonLane.into());
        }
        Ok(decode(class, &vector.bytes))
    }

    /// `bitcast` of a scalar or a vector to vector type `ty`: the same
    /// image. A lane of the result is poison when a bit of it was in a
    /// poison lane of the operand.
    fn bitcast_to_vector(&self, arg: Val, ty: TypeId) -> Result<Vector, Stop> {
        let (lane, len) = self.shape(ty)?;
        let bits = u64::from(len) * u64::from(lane.bits());
        let (bytes, source) = match arg {
            Val::Agg(vector) if vector.is_vector() => {
                let source_bits = u64::from(vector.count()) * u64::from(vector.kind().bits());
                if source_bits != bits {
                    return invalid("bitcast between vectors of different sizes");
                }
                (vector.bytes.clone(), Some(vector))
            }
            Val::Agg(_) => return invalid("bitcast of an aggregate"),
            scalar => {
                let class = match scalar {
                    Val::Int { bits, .. } => Class::Int(bits),
                    Val::Ptr(_) => Class::Ptr,
                    Val::Float(_) => Class::Float,
                    _ => Class::Double,
                };
                let scalar_bits = match scalar {
                    Val::Int { bits, .. } => u64::from(bits),
                    Val::Float(_) => 32,
                    _ => 64,
                };
                if scalar_bits != bits {
                    return invalid("bitcast between types of different sizes");
                }
                let mut bytes = vec![0u8; class.store_len()];
                encode(&scalar, &mut bytes);
                (bytes.into_boxed_slice(), None)
            }
        };
        let mut out = self.vector_from(ty, Some(&bytes))?;
        if let Some(source) = source.filter(Vector::has_poison) {
            let (from, to) = (u64::from(source.kind().bits()), u64::from(lane.bits()));
            for index in (0..source.count()).filter(|&index| source.is_poison(index)) {
                let first = u64::from(index) * from;
                for target in first / to..=(first + from - 1) / to {
                    out.set_poison(target as u32);
                }
            }
        }
        Ok(out)
    }

    // ---- instructions ----

    /// Executes `inst` if it is a vector instruction this module handles:
    /// one whose result is a vector, or `extractelement`. `None` leaves it
    /// to the scalar code (loads, calls, `select` on a scalar condition,
    /// `extractvalue`), which handles vector values as any other.
    #[allow(clippy::too_many_lines)]
    pub(super) fn vector_inst(&self, f: &Frame<'a>, inst: Inst<'a>) -> Result<Option<Flow>, Stop> {
        let op = inst.opcode();
        let ty = inst.ty();
        let result = match inst.kind() {
            InstKind::Binary { lhs, rhs } => {
                let (a, b) = (self.vector(f, lhs)?, self.vector(f, rhs)?);
                self.same_shape(ty, &[&a, &b])?;
                if op.is_int_binary() {
                    let Lane::Int(bits) = a.kind() else {
                        return invalid("integer arithmetic on lanes that are not integers");
                    };
                    self.lanewise(ty, &[&a, &b], |lanes| int_lane(op, bits, lanes[0], lanes[1]))?
                } else {
                    let lane = a.kind();
                    self.lanewise(ty, &[&a, &b], |lanes| {
                        let (Some(x), Some(y)) = (lanes[0], lanes[1]) else { return Ok(None) };
                        self.float_lane(
                            lane,
                            x,
                            y,
                            |x, y| ops::f64_binary(op, x, y),
                            |x, y| ops::f32_binary(op, x, y),
                        )
                    })?
                }
            }
            InstKind::Unary { arg } => {
                let a = self.vector(f, arg)?;
                self.same_shape(ty, &[&a])?;
                match (op, a.kind()) {
                    (Opcode::FNeg, Lane::Float | Lane::Double) => {
                        let sign = 1u128 << (a.kind().bits() - 1);
                        self.lanewise(ty, &[&a], |lanes| Ok(lanes[0].map(|x| x ^ sign)))?
                    }
                    (Opcode::FNeg, _) => return invalid("fneg of lanes that are not floats"),
                    // `freeze` fixes a poison lane to a value each
                    // compilation chooses: still not one result (§5.7).
                    _ => a,
                }
            }
            InstKind::Cast { arg } => {
                let val = self.get(f, arg)?;
                if op == Opcode::BitCast {
                    self.bitcast_to_vector(val, ty)?
                } else {
                    let a = match val {
                        Val::Agg(a) if a.is_vector() => a,
                        _ => return invalid(format!("{} of a scalar to a vector", op.name())),
                    };
                    let (lane, len) = self.shape(ty)?;
                    if a.count() != len {
                        return invalid("a cast between vectors of different lengths");
                    }
                    let elem = self.elem_ty(ty);
                    let mut out = Vector::zeroed(ty, lane, len);
                    for index in 0..len {
                        let Some(x) = a.lane(index) else {
                            out.set_poison(index);
                            continue;
                        };
                        match self.cast(op, elem, x) {
                            Ok(value) => match lane.raw(&value) {
                                Some(raw) => out.set(index, raw),
                                None => return invalid("a cast to a lane of another type"),
                            },
                            // LangRef: poison, per lane.
                            Err(Stop::Trap(Trap::FloatToIntOverflow)) => out.set_poison(index),
                            Err(stop) => return Err(stop),
                        }
                    }
                    out
                }
            }
            InstKind::ICmp { pred, lhs, rhs } => {
                let (a, b) = (self.vector(f, lhs)?, self.vector(f, rhs)?);
                Self::same_lanes(&a, &b)?;
                let lane = a.kind();
                self.lanewise(ty, &[&a, &b], |lanes| {
                    let (Some(x), Some(y)) = (lanes[0], lanes[1]) else { return Ok(None) };
                    self.icmp_lane(pred, lane, x, y).map(|r| Some(u128::from(r)))
                })?
            }
            InstKind::FCmp { pred, lhs, rhs } => {
                let (a, b) = (self.vector(f, lhs)?, self.vector(f, rhs)?);
                Self::same_lanes(&a, &b)?;
                let lane = a.kind();
                self.lanewise(ty, &[&a, &b], |lanes| {
                    let (Some(x), Some(y)) = (lanes[0], lanes[1]) else { return Ok(None) };
                    fcmp_lane(pred, lane, x, y).map(|r| Some(u128::from(r)))
                })?
            }
            InstKind::Select { cond, if_true, if_false } if self.is_vector_value(f, cond) => {
                let c = self.vector(f, cond)?;
                let (a, b) = (self.vector(f, if_true)?, self.vector(f, if_false)?);
                self.same_shape(ty, &[&a, &b])?;
                if c.kind() != Lane::Int(1) || c.count() != a.count() {
                    return invalid("a select whose condition is not one i1 per lane");
                }
                self.lanewise(ty, &[&c, &a, &b], |lanes| {
                    Ok(match lanes[0] {
                        Some(1) => lanes[1],
                        Some(_) => lanes[2],
                        None => None,
                    })
                })?
            }
            InstKind::ExtractElement { vector, index } => {
                let a = self.vector(f, vector)?;
                let (_, index) = self.int(f, index)?;
                let lane = u32::try_from(index).ok().and_then(|index| a.lane(index));
                // Out of range, or a poison lane: a poison scalar.
                let value = lane.ok_or(Trap::PoisonLane)?;
                return Ok(Some(Flow::Value(value)));
            }
            InstKind::InsertElement { vector, value, index } => {
                let mut a = self.vector(f, vector)?;
                self.same_shape(ty, &[&a])?;
                let value = self.get(f, value)?;
                let (_, index) = self.int(f, index)?;
                let Some(raw) = a.kind().raw(&value) else {
                    return invalid("insertelement of a value of another type");
                };
                match u32::try_from(index) {
                    Ok(index) if index < a.count() => a.set(index, raw),
                    // LangRef: the result is poison.
                    _ => (0..a.count()).for_each(|index| a.set_poison(index)),
                }
                a
            }
            InstKind::ShuffleVector { first, second, mask } => {
                let (a, b) = (self.vector(f, first)?, self.vector(f, second)?);
                Self::same_lanes(&a, &b)?;
                let (lane, len) = self.shape(ty)?;
                if lane != a.kind() || mask.len() != len as usize {
                    return invalid("a shufflevector whose type does not fit its operands");
                }
                let mut out = Vector::zeroed(ty, lane, len);
                for (at, &pick) in mask.iter().enumerate() {
                    let source = match pick {
                        SHUFFLE_UNDEF => None,
                        pick if pick < a.count() => a.lane(pick),
                        pick if pick - a.count() < b.count() => b.lane(pick - a.count()),
                        _ => return invalid("a shufflevector mask entry out of range"),
                    };
                    match source.as_ref().and_then(|value| lane.raw(value)) {
                        Some(raw) => out.set(at as u32, raw),
                        None => out.set_poison(at as u32),
                    }
                }
                out
            }
            InstKind::PtrAdd(_) => return unsupported("inst:ptradd-vector"),
            InstKind::AtomicRmw { .. } => return unsupported("inst:atomicrmw-vector"),
            _ => return Ok(None),
        };
        Ok(Some(Flow::Value(Val::Agg(result))))
    }

    /// Checks that `ty` is a vector type of the operands' shape.
    fn same_shape(&self, ty: TypeId, operands: &[&Vector]) -> Result<(), Stop> {
        let (lane, len) = self.shape(ty)?;
        if operands.iter().any(|v| v.kind() != lane || v.count() != len) {
            return invalid("vector operands of another shape than the result");
        }
        Ok(())
    }

    fn same_lanes(a: &Vector, b: &Vector) -> Result<(), Stop> {
        if a.kind() != b.kind() || a.count() != b.count() {
            return invalid("vector operands of two shapes");
        }
        Ok(())
    }

    /// The vector of type `ty` whose lane `i` is `op` of lane `i` of each
    /// operand (`None` for a poison lane); lanes in order, so the first
    /// lane that traps is the trap.
    fn lanewise(
        &self,
        ty: TypeId,
        operands: &[&Vector],
        mut op: impl FnMut(&[Option<u128>]) -> LaneResult,
    ) -> Result<Vector, Stop> {
        let (lane, len) = self.shape(ty)?;
        let mut out = Vector::zeroed(ty, lane, len);
        let mut lanes = Vec::with_capacity(operands.len());
        for index in 0..len {
            lanes.clear();
            lanes.extend(operands.iter().map(|v| (!v.is_poison(index)).then(|| v.get(index))));
            match op(&lanes)? {
                Some(raw) => out.set(index, raw),
                None => out.set_poison(index),
            }
        }
        Ok(out)
    }

    /// A float operation on one lane of kind `lane`, under the NaN policy
    /// (§5.1).
    fn float_lane(
        &self,
        lane: Lane,
        x: u128,
        y: u128,
        double: impl Fn(f64, f64) -> f64,
        float: impl Fn(f32, f32) -> f32,
    ) -> LaneResult {
        let raw = match lane {
            Lane::Float => {
                let r = float(f32::from_bits(x as u32), f32::from_bits(y as u32));
                self.nan_check(r.is_nan())?;
                u128::from(r.to_bits())
            }
            Lane::Double => {
                let r = double(f64::from_bits(x as u64), f64::from_bits(y as u64));
                self.nan_check(r.is_nan())?;
                u128::from(r.to_bits())
            }
            _ => return invalid("float arithmetic on lanes that are not floats"),
        };
        Ok(Some(raw))
    }

    fn nan_check(&self, nan: bool) -> Result<(), Stop> {
        if nan && self.config.nan == NanPolicy::Trap {
            return Err(Trap::NanResult.into());
        }
        Ok(())
    }

    /// `icmp` of two lanes (§5.2): integers, or pointers as addresses with
    /// the rule for block addresses of §5.5.
    fn icmp_lane(&self, pred: IntPredicate, lane: Lane, x: u128, y: u128) -> Result<bool, Stop> {
        match lane {
            Lane::Int(bits) => Ok(ops::icmp(pred, bits, x, y)),
            Lane::Ptr => {
                let label = |addr: u128| self.block_at.contains_key(&(addr as u64));
                if (label(x) && y != 0) || (label(y) && x != 0) {
                    return Err(Trap::BlockAddressInspected.into());
                }
                Ok(ops::icmp(pred, 64, x, y))
            }
            _ => invalid("icmp of lanes that are not integers or pointers"),
        }
    }

    // ---- intrinsics (§5.6) ----

    /// An intrinsic call with vector operands or a vector result.
    /// `Some(result)` when this handles it (every one it handles has a
    /// result); `None` leaves it to the scalar code (a scalar intrinsic,
    /// or one that is unsupported by name).
    #[allow(clippy::too_many_lines)]
    pub(super) fn vector_intrinsic(
        &self,
        f: &Frame<'a>,
        intrinsic: Intrinsic<'a>,
        call: CallSite<'a>,
        ty: TypeId,
    ) -> Result<Option<Val>, Stop> {
        use Intrinsic as I;
        let args = call.args();
        let reduction = matches!(
            intrinsic,
            I::VectorReduceAdd
                | I::VectorReduceMul
                | I::VectorReduceAnd
                | I::VectorReduceOr
                | I::VectorReduceXor
                | I::VectorReduceSmax
                | I::VectorReduceSmin
                | I::VectorReduceUmax
                | I::VectorReduceUmin
                | I::VectorReduceFadd
                | I::VectorReduceFmul
        );
        if reduction {
            return self.reduce(f, intrinsic, call, ty).map(Some);
        }
        if matches!(intrinsic, I::VectorExtract | I::VectorInsert) {
            return self.subvector(f, intrinsic, call, ty).map(Some);
        }
        let vector_args = args.iter().any(|&value| self.is_vector_value(f, value));
        if !self.module.types.is_vector(ty) && !vector_args {
            return Ok(None);
        }
        let int_op = match intrinsic {
            I::Abs => Some(IntOp::Abs),
            I::Smax => Some(IntOp::Smax),
            I::Smin => Some(IntOp::Smin),
            I::Umax => Some(IntOp::Umax),
            I::Umin => Some(IntOp::Umin),
            I::Ctpop => Some(IntOp::Ctpop),
            I::Ctlz => Some(IntOp::Ctlz),
            I::Cttz => Some(IntOp::Cttz),
            I::BitReverse => Some(IntOp::BitReverse),
            I::Bswap => Some(IntOp::Bswap),
            I::Fshl => Some(IntOp::Fshl),
            I::Fshr => Some(IntOp::Fshr),
            I::SaddSat => Some(IntOp::SaddSat),
            I::UaddSat => Some(IntOp::UaddSat),
            I::SsubSat => Some(IntOp::SsubSat),
            I::UsubSat => Some(IntOp::UsubSat),
            _ => None,
        };
        // The operands: vectors lane by lane, scalars (the `i1` flag of
        // `abs`, `ctlz`, `cttz`) the same in every lane.
        let mut vectors = Vec::new();
        let mut scalars = Vec::new();
        for &value in args {
            match self.get(f, value)? {
                Val::Agg(vector) if vector.is_vector() => vectors.push(vector),
                Val::Int { value, .. } => scalars.push(value),
                other => return invalid(format!("{other} as an operand of a vector intrinsic")),
            }
        }
        let refs: Vec<&Vector> = vectors.iter().collect();
        if let Some(first) = refs.first()
            && refs.iter().any(|v| v.count() != first.count())
        {
            return invalid("vector operands of two lengths");
        }
        let result = if let Some(int_op) = int_op {
            let Some(&first) = refs.first() else { return invalid("no vector operand") };
            let Lane::Int(bits) = first.kind() else {
                return invalid("an integer intrinsic on lanes that are not integers");
            };
            if int_op == IntOp::Bswap && bits % 16 != 0 {
                return invalid("bswap of an odd number of bytes");
            }
            let flag = scalars.first().copied().unwrap_or(0);
            self.lanewise(ty, &refs, |lanes| {
                if lanes.iter().any(Option::is_none) {
                    return Ok(None);
                }
                let at = |index: usize| lanes.get(index).copied().flatten().unwrap_or(flag);
                match ops::int_intrinsic(int_op, bits, at(0), at(1), at(2)) {
                    Ok(raw) => Ok(Some(raw)),
                    Err(Trap::PoisonIntrinsic) => Ok(None),
                    Err(trap) => Err(trap.into()),
                }
            })?
        } else {
            match intrinsic {
                I::Scmp | I::Ucmp => {
                    let (lane, _) = self.shape(ty)?;
                    let (Some(&a), Lane::Int(result_bits)) = (refs.first(), lane) else {
                        return invalid("a three-way compare of vectors of another shape");
                    };
                    let Lane::Int(bits) = a.kind() else {
                        return invalid("a three-way compare of lanes that are not integers");
                    };
                    let signed = intrinsic == I::Scmp;
                    self.lanewise(ty, &refs, |lanes| {
                        let (Some(x), Some(y)) = (lanes[0], lanes.get(1).copied().flatten()) else {
                            return Ok(None);
                        };
                        Ok(Some(ops::three_way(signed, bits, x, y, result_bits)))
                    })?
                }
                I::Fabs | I::CopySign => {
                    let Some(&a) = refs.first() else { return invalid("no vector operand") };
                    if !matches!(a.kind(), Lane::Float | Lane::Double) {
                        return invalid("a float intrinsic on lanes that are not floats");
                    }
                    let sign = 1u128 << (a.kind().bits() - 1);
                    let copy = intrinsic == I::CopySign;
                    self.lanewise(ty, &refs, |lanes| {
                        Ok(match (lanes[0], lanes.get(1).copied().flatten()) {
                            (Some(x), _) if !copy => Some(x & !sign),
                            (Some(x), Some(y)) => Some((x & !sign) | (y & sign)),
                            _ => None,
                        })
                    })?
                }
                I::Sqrt
                | I::Floor
                | I::Ceil
                | I::Trunc
                | I::Round
                | I::RoundEven
                | I::Rint
                | I::NearbyInt => {
                    let Some(&a) = refs.first() else { return invalid("no vector operand") };
                    let lane = a.kind();
                    macro_rules! rounded {
                        ($x:expr) => {
                            match intrinsic {
                                I::Sqrt => $x.sqrt(),
                                I::Floor => $x.floor(),
                                I::Ceil => $x.ceil(),
                                I::Trunc => $x.trunc(),
                                I::Round => $x.round(),
                                _ => $x.round_ties_even(),
                            }
                        };
                    }
                    self.lanewise(ty, &refs, |lanes| {
                        let Some(x) = lanes[0] else { return Ok(None) };
                        self.float_lane(lane, x, 0, |x, _| rounded!(x), |x, _| rounded!(x))
                    })?
                }
                I::Fma => {
                    let Some(&a) = refs.first() else { return invalid("no vector operand") };
                    if refs.len() != 3 {
                        return invalid("fma of vectors needs three vectors");
                    }
                    let lane = a.kind();
                    self.lanewise(ty, &refs, |lanes| {
                        let [Some(x), Some(y), Some(z)] = [lanes[0], lanes[1], lanes[2]] else {
                            return Ok(None);
                        };
                        self.float_lane(
                            lane,
                            x,
                            y,
                            |x, y| x.mul_add(y, f64::from_bits(z as u64)),
                            |x, y| x.mul_add(y, f32::from_bits(z as u32)),
                        )
                    })?
                }
                // Vector results of these would need a vector of flags
                // as well: not in any known input.
                I::SaddWithOverflow
                | I::UaddWithOverflow
                | I::SsubWithOverflow
                | I::UsubWithOverflow
                | I::SmulWithOverflow
                | I::UmulWithOverflow => {
                    return unsupported(format!("intrinsic-vector:{}", intrinsic.name()));
                }
                // Anything else is the scalar code's: refused by name, or
                // one of the no-code intrinsics.
                _ => return Ok(None),
            }
        };
        Ok(Some(Val::Agg(result)))
    }

    /// `llvm.vector.reduce.*` (§5.10): a fold of the lanes in order. A
    /// poison lane is observed.
    fn reduce(
        &self,
        f: &Frame<'a>,
        intrinsic: Intrinsic<'a>,
        call: CallSite<'a>,
        ty: TypeId,
    ) -> Result<Val, Stop> {
        use Intrinsic as I;
        let args = call.args();
        let ordered = matches!(intrinsic, I::VectorReduceFadd | I::VectorReduceFmul);
        let (start, vector) = match (ordered, args) {
            (true, &[start, vector]) => (Some(self.get(f, start)?), vector),
            (false, &[vector]) => (None, vector),
            _ => return invalid(format!("{} with the wrong operands", intrinsic.name())),
        };
        let v = self.vector(f, vector)?;
        if v.has_poison() {
            return Err(Trap::PoisonLane.into());
        }
        let lanes = (0..v.count()).map(|index| v.get(index));
        if let Some(start) = start {
            // Ordered: ((start op v0) op v1) ..., each step under the NaN
            // policy; with `reassoc` LLVM may reorder, which hod_ir does
            // not carry, so the order is the only one.
            let Some(mut acc) = v.kind().raw(&start) else {
                return invalid("a reduction whose start is not of the lane type");
            };
            let op = if intrinsic == I::VectorReduceFadd { Opcode::FAdd } else { Opcode::FMul };
            for lane in lanes {
                let step = self.float_lane(
                    v.kind(),
                    acc,
                    lane,
                    |x, y| ops::f64_binary(op, x, y),
                    |x, y| ops::f32_binary(op, x, y),
                )?;
                acc = step.unwrap_or(0);
            }
            return Ok(v.kind().val(acc));
        }
        let Lane::Int(bits) = v.kind() else {
            return invalid("an integer reduction of lanes that are not integers");
        };
        if self.class(ty)? != Class::Int(bits) {
            return invalid("a reduction whose result is not the lane type");
        }
        let signed = |x: u128| sext(bits, x);
        let mut lanes = lanes;
        let first = lanes.next().unwrap_or(0);
        let value = lanes.fold(first, |acc, x| match intrinsic {
            I::VectorReduceAdd => acc.wrapping_add(x) & mask(bits),
            I::VectorReduceMul => acc.wrapping_mul(x) & mask(bits),
            I::VectorReduceAnd => acc & x,
            I::VectorReduceOr => acc | x,
            I::VectorReduceXor => acc ^ x,
            I::VectorReduceSmax => {
                if signed(x) > signed(acc) {
                    x
                } else {
                    acc
                }
            }
            I::VectorReduceSmin => {
                if signed(x) < signed(acc) {
                    x
                } else {
                    acc
                }
            }
            I::VectorReduceUmax => acc.max(x),
            _ => acc.min(x),
        });
        Ok(Val::Int { bits, value })
    }

    /// `llvm.vector.extract(v, i)` and `llvm.vector.insert(v, s, i)`
    /// (LangRef): the subvector of `len(s)` lanes at lane `i`, which is a
    /// multiple of that length; past the end the result is poison.
    fn subvector(
        &self,
        f: &Frame<'a>,
        intrinsic: Intrinsic<'a>,
        call: CallSite<'a>,
        ty: TypeId,
    ) -> Result<Val, Stop> {
        let args = call.args();
        let insert = intrinsic == Intrinsic::VectorInsert;
        let (whole, part, index) = match (insert, args) {
            (false, &[whole, index]) => (self.vector(f, whole)?, None, index),
            (true, &[whole, part, index]) => {
                (self.vector(f, whole)?, Some(self.vector(f, part)?), index)
            }
            _ => return invalid(format!("{} with the wrong operands", intrinsic.name())),
        };
        let (_, start) = self.int(f, index)?;
        let mut out = if part.is_some() {
            self.same_shape(ty, &[&whole])?;
            whole.clone()
        } else {
            let (lane, len) = self.shape(ty)?;
            if lane != whole.kind() {
                return invalid("llvm.vector.extract to lanes of another type");
            }
            Vector::zeroed(ty, lane, len)
        };
        let sub_len = part.as_ref().map_or(out.count(), Vector::count);
        if part.as_ref().is_some_and(|part| part.kind() != whole.kind()) {
            return invalid("llvm.vector.insert of lanes of another type");
        }
        if start % u128::from(sub_len) != 0 {
            return invalid(format!("{} at an index that is not a multiple", intrinsic.name()));
        }
        if start + u128::from(sub_len) > u128::from(whole.count()) {
            (0..out.count()).for_each(|index| out.set_poison(index));
            return Ok(Val::Agg(out));
        }
        let start = start as u32;
        for at in 0..sub_len {
            let (to, from) = if insert { (start + at, at) } else { (at, start + at) };
            let source = match &part {
                Some(part) => part.lane(from),
                None => whole.lane(from),
            };
            match source.as_ref().and_then(|value| out.kind().raw(value)) {
                Some(raw) => out.set(to, raw),
                None => out.set_poison(to),
            }
        }
        Ok(Val::Agg(out))
    }
}

/// An integer binary operation on one lane (§5.1), with the rules of
/// §5.10: a division by zero, or by a poison lane, traps; a shift by the
/// width or more is a poison lane; poison in, poison out.
fn int_lane(op: Opcode, bits: u32, x: Option<u128>, y: Option<u128>) -> LaneResult {
    let divides = matches!(op, Opcode::UDiv | Opcode::SDiv | Opcode::URem | Opcode::SRem);
    if divides {
        match y {
            None | Some(0) => return Err(Trap::DivisionByZero.into()),
            Some(y) => {
                let signed = matches!(op, Opcode::SDiv | Opcode::SRem);
                // A poison dividend may be the smallest value.
                if signed && y == mask(bits) && x.is_none_or(|x| x == 1 << (bits - 1)) {
                    return Err(Trap::DivisionOverflow.into());
                }
            }
        }
    }
    let (Some(x), Some(y)) = (x, y) else { return Ok(None) };
    match ops::int_binary(op, bits, x, y) {
        Ok(raw) => Ok(Some(raw)),
        Err(Trap::ShiftTooLarge) => Ok(None),
        Err(trap) => Err(trap.into()),
    }
}

/// `fcmp` of two lanes (§5.2).
fn fcmp_lane(pred: FloatPredicate, lane: Lane, x: u128, y: u128) -> Result<bool, Stop> {
    match lane {
        Lane::Float => Ok(ops::fcmp(
            pred,
            f64::from(f32::from_bits(x as u32)),
            f64::from(f32::from_bits(y as u32)),
        )),
        Lane::Double => Ok(ops::fcmp(pred, f64::from_bits(x as u64), f64::from_bits(y as u64))),
        _ => invalid("fcmp of lanes that are not floats"),
    }
}
