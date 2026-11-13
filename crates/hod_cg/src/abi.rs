//! Argument assignment: `spec/codegen.md` §7.2.
//!
//! A target's `assign` function maps a [`Sig`] to an [`Assignment`]: where
//! each byte of each argument and of the return value travels. The same
//! answer serves a call (outgoing placement) and a function's entry
//! (incoming locations). The rules are the target's, in
//! `spec/abi-<target>.md`.

use hod_ir::{
    CallConv, CallSite, Module, ParamAttrs, StackArgs, Symbol, SymbolKind, TypeId, TypeKind,
    TypeTable, call_arg_attrs, call_ret_attrs,
};

use crate::reg::PReg;
use crate::target::Target;

/// What is being called or defined, as far as passing goes.
#[derive(Clone, Copy, Debug)]
pub struct Sig<'a> {
    /// The calling convention.
    pub cc: CallConv,
    /// The return type; `TypeId::VOID` for none.
    pub ret: TypeId,
    /// The attributes of the return value.
    pub ret_attrs: ParamAttrs,
    /// The types of the arguments passed: for a call through a variadic
    /// type, the variable ones included.
    pub params: &'a [TypeId],
    /// The attributes of the first `param_attrs.len()` arguments; the rest
    /// have none.
    pub param_attrs: &'a [ParamAttrs],
    /// The number of named parameters: `params.len()` unless `variadic`.
    pub fixed: u32,
    /// Whether the function type is variadic.
    pub variadic: bool,
}

impl Sig<'_> {
    /// The attributes of argument `index`.
    pub fn attrs(&self, index: usize) -> ParamAttrs {
        self.param_attrs.get(index).copied().unwrap_or(ParamAttrs::NONE)
    }
}

/// Where a part travels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// In a register, in its low bytes.
    Reg(PReg),
    /// In the stack-argument area, at this byte offset from its lowest
    /// address (the stack pointer at the call instruction).
    Stack(u32),
}

/// What the bytes of a part are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartKind {
    /// Bytes `offset..offset + size` of the argument's value (of its
    /// in-memory image, `spec/ir.md` §4.3, for a `Mem`-class value).
    Value,
    /// Bytes `offset..offset + size` of the memory the argument points
    /// to: a `byval` argument, which is copied into the stack-argument
    /// area. In the callee the argument's value is the address of that
    /// copy.
    Pointee,
    /// Elements of a vector, each widened to a lane of the place (§7.2):
    /// the vector of `len` elements of `bits` bits each (1 for a mask)
    /// whose image starts at byte `offset` of the value; lane `i` of the
    /// place (`lane` bytes each, `size / lane` lanes) carries element
    /// `first + i`, extended by zero. Its consumer reads only the
    /// element's bits of each lane. The target moves these
    /// ([`Target::lanes_out`](crate::Target::lanes_out),
    /// [`Target::lanes_in`](crate::Target::lanes_in)); the call sequence
    /// never copies them as bytes.
    Lanes {
        /// The bits of an element: 1, 8 or 16.
        bits: u8,
        /// The bytes of a lane at the place.
        lane: u8,
        /// The element the first lane carries.
        first: u8,
        /// The number of elements of the vector.
        len: u8,
    },
}

/// One side of a [`PartKind::Lanes`] move: a register or a frame place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneLoc {
    /// A register: the place's register, or a register of the value's
    /// class holding the value (a general register holding a mask's
    /// bits, bit `i` element `i`; on AArch64 a vector register holding a
    /// lone `<1 x i128>`, WP-167).
    Reg(PReg),
    /// A frame place: the place's stack slot, or the vector's image.
    Frame(crate::FrameRef),
}

/// One contiguous piece of an argument or of the return value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Part {
    /// The argument's index in `Sig::params`; 0 for a part of the return
    /// value.
    pub arg: u32,
    /// What the bytes are.
    pub kind: PartKind,
    /// The first byte of the piece.
    pub offset: u32,
    /// Its size in bytes: at most the register's size for a register.
    pub size: u32,
    /// Where it travels.
    pub place: Place,
}

/// How a return value too large for the return registers travels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetMemory {
    /// Where the caller passes the address of the buffer the callee
    /// writes the value to.
    pub address: Place,
    /// The register in which the callee hands that address back, on a
    /// target whose convention requires it.
    pub returned_in: Option<PReg>,
}

/// What `va_start` needs to know about the named parameters.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct VaInfo {
    /// How many general argument registers the named parameters used.
    pub gpr_used: u8,
    /// How many vector argument registers the named parameters used.
    pub fpr_used: u8,
    /// The offset in the stack-argument area of the first variable
    /// argument passed on the stack.
    pub stack_offset: u32,
}

/// The answer of `assign`. Reused between calls: `clear` keeps the
/// allocation.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct Assignment {
    /// The parts of the arguments, in argument order and, within an
    /// argument, in offset order. An argument that takes no space (an
    /// empty aggregate) has no part.
    pub args: Vec<Part>,
    /// The parts of the return value, in offset order; empty for `void`
    /// and when `ret_memory` is set.
    pub ret: Vec<Part>,
    /// Set when the return value travels in memory although the signature
    /// has no `sret` parameter.
    pub ret_memory: Option<RetMemory>,
    /// The register in which a function with an `sret` parameter hands
    /// that parameter back, on a target whose convention requires it.
    pub sret_returned_in: Option<PReg>,
    /// The size of the stack-argument area in bytes, not rounded to the
    /// stack alignment.
    pub stack_size: u32,
    /// For a variadic signature.
    pub va: VaInfo,
    /// How many vector registers the arguments use, for a convention that
    /// passes that count to a variadic callee (x86-64's `al`).
    pub vector_regs_used: u8,
}

impl Assignment {
    /// Empties the answer, keeping its buffers.
    pub fn clear(&mut self) {
        self.args.clear();
        self.ret.clear();
        self.ret_memory = None;
        self.sret_returned_in = None;
        self.stack_size = 0;
        self.va = VaInfo::default();
        self.vector_regs_used = 0;
    }
}

/// A signature a target cannot pass, under the stable name the coverage
/// log reports (`abi:<what>`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AbiUnsupported(pub &'static str);

/// `hod_ir::StackArgs` answered by a target's [`Target::assign`]: the
/// hook `classify` needs to decide a `musttail` call (`spec/codegen.md`
/// §7.1, "Tail kinds"; `spec/ir.md` §8.6).
///
/// Both sides are assigned as the call sequence assigns them: a
/// function's own signature with its declared attributes, a call with its
/// effective ones (`hod_ir::call_arg_attrs`). The variable arguments of a
/// variadic call (those beyond its function type's parameters, before a
/// forwarded `...`) are typed by their values in the call's body, as the
/// call sequence types them (WP-115).
///
/// A target whose convention depends on the function being compiled
/// (x86-64's features, `spec/abi-x64.md` §1.3) is given that function's
/// target by [`TargetStackArgs::per_function`]: both sides are then
/// assigned with the target of the function that makes the call, as
/// [`Target::for_function`] gives it to the call sequence (WP-148).
#[derive(Clone, Copy)]
pub struct TargetStackArgs<'t, T> {
    target: &'t T,
    /// The target as it compiles a function, `None` for none (the
    /// function is not one the target compiles: no answer).
    per_function: Option<PerFunction<T>>,
}

/// The target as it compiles `function` of `module`: what
/// [`Target::for_function`] answers for its body, computed from the
/// symbol alone (`classify` has no body for `of_function`).
pub type PerFunction<T> = fn(&Module, Symbol<'_>) -> Option<T>;

impl<T> std::fmt::Debug for TargetStackArgs<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TargetStackArgs")
    }
}

impl<'t, T: Target> TargetStackArgs<'t, T> {
    /// The hook of `target`.
    pub const fn new(target: &'t T) -> TargetStackArgs<'t, T> {
        TargetStackArgs { target, per_function: None }
    }

    /// The hook of a target whose convention depends on the function
    /// being compiled: `of(module, function)` is the target as it
    /// compiles `function` (WP-148). `target` is not used then; it is
    /// kept for the type.
    pub const fn per_function(target: &'t T, of: PerFunction<T>) -> TargetStackArgs<'t, T> {
        TargetStackArgs { target, per_function: Some(of) }
    }

    /// The `stack_size` of `sig` as `function` (the function making the
    /// call, or being called by its own signature) assigns it.
    fn stack_size(&self, module: &Module, function: Symbol<'_>, sig: &Sig<'_>) -> Option<u64> {
        let mut asg = Assignment::default();
        match self.per_function {
            Some(of) => of(module, function)?.assign(&module.types, sig, &mut asg).ok()?,
            None => self.target.assign(&module.types, sig, &mut asg).ok()?,
        }
        Some(u64::from(asg.stack_size))
    }
}

impl<T: Target> StackArgs for TargetStackArgs<'_, T> {
    fn of_function(&self, module: &Module, function: Symbol<'_>) -> Option<u64> {
        let SymbolKind::Function { cc, ret_attrs, param_attrs, .. } = function.kind() else {
            return None;
        };
        let TypeKind::Function { ret, params, variadic } = module.types.kind(function.ty()) else {
            return None;
        };
        let fixed = u32::try_from(params.len()).ok()?;
        let sig = Sig { cc, ret, ret_attrs, params, param_attrs, fixed, variadic };
        self.stack_size(module, function, &sig)
    }

    fn of_call(&self, module: &Module, call: &CallSite<'_>) -> Option<u64> {
        let TypeKind::Function { ret, params, variadic } = module.types.kind(call.fn_type()) else {
            return None;
        };
        let args = call.args();
        if args.len() < params.len() || (args.len() > params.len() && !variadic) {
            return None;
        }
        // The variable arguments are typed as the call sequence types
        // them: by their values, in the call's body (WP-115).
        let types: Vec<TypeId> = params
            .iter()
            .copied()
            .chain(args[params.len()..].iter().map(|&value| call.body().value_type(module, value)))
            .collect();
        let ret_attrs = call_ret_attrs(module, call).ok()?;
        let param_attrs = (0..args.len())
            .map(|index| call_arg_attrs(module, call, index).ok())
            .collect::<Option<Vec<ParamAttrs>>>()?;
        let fixed = u32::try_from(params.len()).ok()?;
        let sig = Sig {
            cc: call.cc(),
            ret,
            ret_attrs,
            params: &types,
            param_attrs: &param_attrs,
            fixed,
            variadic,
        };
        // The call is assigned as the function it is in compiles it.
        self.stack_size(module, module.symbol(call.body().symbol()), &sig)
    }
}

// ---- The leaf walk (§7.2, "The leaf walk"; WP-117) ----

/// An argument broken into more pieces than its signature's budget
/// allows: the target charges each leaf's pieces against a budget it
/// chooses ([`charge`]).
pub const TOO_MANY_PIECES: &str = "abi:too-many-pieces";

/// An offset or a size of a part beyond `u32`.
pub const SIZE_LIMIT: &str = "abi:size-limit";

/// Where a target puts one piece, before [`push`] checks its stack offset
/// against `u32`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceLoc {
    /// In a register.
    Reg(PReg),
    /// In the stack-argument area, at this byte offset.
    Stack(u64),
}

/// The elements of a vector leaf as lanes ([`PartKind::Lanes`]): `len`
/// elements of `bits` bits, `lane` bytes per lane; with `each`, piece `i`
/// carries element `i` alone, else the one piece carries them all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LaneLeaf {
    /// The bits of an element.
    pub bits: u8,
    /// The bytes of a lane at the place.
    pub lane: u8,
    /// The number of elements.
    pub len: u8,
    /// One element per piece.
    pub each: bool,
}

/// How a target breaks one leaf (a value that is not an array or a
/// structure) into pieces: `count` pieces of the target's class `class`;
/// piece `i` holds the leaf's bytes from `i * stride`, `size` of them
/// (`last_size` for the last piece). A leaf whose elements travel widened
/// to lanes has `lanes`, and then `size` is the bytes of the place each
/// piece fills.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Leaf<C> {
    /// The target's class of every piece.
    pub class: C,
    /// The number of pieces.
    pub count: u32,
    /// The distance in the leaf between the starts of two pieces.
    pub stride: u32,
    /// The bytes of each piece but the last.
    pub size: u32,
    /// The bytes of the last piece.
    pub last_size: u32,
    /// The pieces that start an integer wider than a register: piece `i`
    /// with `i % split == 0`; none for 0. A target with a rule for such
    /// integers (AArch64's even register, x86-64's pair that is never
    /// split) applies it at those pieces.
    pub split: u32,
    /// The lanes, for a vector whose elements are widened.
    pub lanes: Option<LaneLeaf>,
}

impl<C: Copy> Leaf<C> {
    /// One piece of `size` bytes.
    pub fn one(class: C, size: u32) -> Leaf<C> {
        Leaf { class, count: 1, stride: 0, size, last_size: size, split: 0, lanes: None }
    }

    /// `count` pieces of `size` bytes each, one after the other.
    pub fn each(class: C, count: u32, size: u32) -> Leaf<C> {
        Leaf { class, count, stride: size, size, last_size: size, split: 0, lanes: None }
    }

    /// Whether piece `piece` starts an integer wider than a register.
    pub fn split_at(&self, piece: u32) -> bool {
        self.split != 0 && piece.is_multiple_of(self.split)
    }

    /// The kind of piece `piece`'s part.
    pub fn kind(&self, piece: u32) -> PartKind {
        match self.lanes {
            None => PartKind::Value,
            Some(l) => PartKind::Lanes {
                bits: l.bits,
                lane: l.lane,
                first: if l.each { piece as u8 } else { 0 },
                len: l.len,
            },
        }
    }

    /// The bytes piece `piece` carries.
    pub fn piece_size(&self, piece: u32) -> u32 {
        if piece + 1 == self.count { self.last_size } else { self.size }
    }

    /// The offset of piece `piece` in a value whose leaf is at `offset`.
    pub fn piece_offset(&self, offset: u64, piece: u32) -> u64 {
        offset.saturating_add(u64::from(piece) * u64::from(self.stride))
    }
}

/// The per-target classification hook of the leaf walk: the pieces of
/// one leaf type, or the name under which the target refuses it. The
/// walk itself ([`walk`], [`place_leaf`], [`place_leaves`]) is the same
/// for every target; where each piece goes is the target's
/// `place` function, which sees the whole [`Leaf`] at each piece (so a
/// pair kept whole is decided at its first piece), and keeps the target's
/// own state (next registers, stack offset, the vector-register count).
/// A target decides on its own when a return value is demoted to memory
/// (its return `place` fails, and it clears the parts).
pub trait Classify {
    /// The target's register class of a piece.
    type Class: Copy;

    /// The pieces of the leaf type `ty` (never an array or a structure).
    ///
    /// # Errors
    /// The target's name for a leaf it does not pass.
    fn leaf(&self, types: &TypeTable, ty: TypeId) -> Result<Leaf<Self::Class>, AbiUnsupported>;
}

/// A number that is equal for two leaf types exactly when LLVM gives them
/// one value type: a pointer is an `i64`.
pub fn leaf_key(types: &TypeTable, ty: TypeId) -> u64 {
    let scalar = |ty: TypeId| -> u64 {
        match types.kind(ty) {
            TypeKind::Int(bits) => u64::from(bits),
            TypeKind::Ptr { .. } => 64,
            TypeKind::Half => 1 << 24,
            TypeKind::BFloat => 2 << 24,
            TypeKind::Float => 3 << 24,
            TypeKind::Double => 4 << 24,
            _ => 5 << 24,
        }
    };
    match types.kind(ty) {
        TypeKind::Vector { elem, len } => 1 << 63 | scalar(elem) << 32 | u64::from(len),
        _ => scalar(ty),
    }
}

/// The leaves of a type, as far as a rule about uniform aggregates (an
/// AArch64 block) needs them.
#[derive(Clone, Copy, Debug)]
pub enum Shape {
    /// No leaf.
    Empty,
    /// `leaves` leaves of one value type ([`leaf_key`] `key`), `ty`
    /// being one of them.
    Uniform {
        /// The leaves' [`leaf_key`].
        key: u64,
        /// One of the leaves.
        ty: TypeId,
        /// How many.
        leaves: u64,
    },
    /// Leaves of more than one value type.
    Mixed,
}

/// The shape of `ty`, in time proportional to the type's description,
/// not to its number of leaves.
pub fn shape(types: &TypeTable, ty: TypeId) -> Shape {
    match types.kind(ty) {
        TypeKind::Struct { fields, .. } => {
            let mut all = Shape::Empty;
            for &field in fields {
                all = match (all, shape(types, field)) {
                    (Shape::Empty, other) | (other, Shape::Empty) => other,
                    (
                        Shape::Uniform { key, ty, leaves },
                        Shape::Uniform { key: other, leaves: more, .. },
                    ) if key == other => {
                        Shape::Uniform { key, ty, leaves: leaves.saturating_add(more) }
                    }
                    _ => return Shape::Mixed,
                };
            }
            all
        }
        TypeKind::Array { elem, len } => match shape(types, elem) {
            Shape::Uniform { key, ty, leaves } if len > 0 => {
                Shape::Uniform { key, ty, leaves: leaves.saturating_mul(len) }
            }
            Shape::Mixed if len > 0 => Shape::Mixed,
            _ => Shape::Empty,
        },
        _ => Shape::Uniform { key: leaf_key(types, ty), ty, leaves: 1 },
    }
}

/// Refuses a type that has a leaf `classify` does not pass, in time
/// proportional to the type's description.
///
/// # Errors
/// What `classify` reports for the first such leaf.
pub fn check_type<K: Classify>(
    types: &TypeTable,
    ty: TypeId,
    classify: &K,
) -> Result<(), AbiUnsupported> {
    match types.kind(ty) {
        TypeKind::Struct { fields, .. } => {
            fields.iter().try_for_each(|&f| check_type(types, f, classify))
        }
        TypeKind::Array { elem, .. } => check_type(types, elem, classify),
        _ => classify.leaf(types, ty).map(|_| ()),
    }
}

/// Calls `visit` with each leaf of `ty` and its byte offset in the value
/// plus `base`, in offset order, to any depth: a structure's fields at
/// their offsets (`TypeTable::field_offsets`), an array's elements at
/// multiples of the element's alloc size. An array of elements without a
/// leaf is skipped without iterating it.
///
/// # Errors
/// The first error of `visit`, which ends the walk.
pub fn walk<E>(
    types: &TypeTable,
    ty: TypeId,
    base: u64,
    visit: &mut impl FnMut(TypeId, u64) -> Result<(), E>,
) -> Result<(), E> {
    match types.kind(ty) {
        TypeKind::Struct { fields, .. } => {
            let offsets = types.field_offsets(ty).unwrap_or(&[]);
            for (&field, &offset) in fields.iter().zip(offsets) {
                walk(types, field, base.saturating_add(offset), visit)?;
            }
            Ok(())
        }
        TypeKind::Array { elem, len } => {
            if matches!(shape(types, elem), Shape::Empty) {
                return Ok(());
            }
            // Every element has a leaf, and a caller that charges every
            // leaf against a piece budget bounds this loop by it.
            let stride = types.alloc_size_of(elem);
            for index in 0..len {
                walk(types, elem, base.saturating_add(index.saturating_mul(stride)), visit)?;
            }
            Ok(())
        }
        _ => visit(ty, base),
    }
}

/// Takes `pieces` from `budget`.
///
/// # Errors
/// [`TOO_MANY_PIECES`] when the budget does not have them.
pub fn charge(budget: &mut u32, pieces: u32) -> Result<(), AbiUnsupported> {
    match budget.checked_sub(pieces) {
        Some(left) => {
            *budget = left;
            Ok(())
        }
        None => Err(AbiUnsupported(TOO_MANY_PIECES)),
    }
}

/// Appends a part of argument `arg` (0 for the return value); a stack
/// part that continues the previous one of the same argument and kind,
/// in the value and on the stack, extends it instead (never a `Lanes`
/// part).
///
/// # Errors
/// [`SIZE_LIMIT`] when an offset or an end is beyond `u32`.
pub fn push(
    parts: &mut Vec<Part>,
    arg: u32,
    kind: PartKind,
    offset: u64,
    size: u32,
    loc: PieceLoc,
) -> Result<(), AbiUnsupported> {
    let narrow = |n: u64| u32::try_from(n).or(Err(AbiUnsupported(SIZE_LIMIT)));
    let offset = narrow(offset)?;
    let place = match loc {
        PieceLoc::Reg(reg) => Place::Reg(reg),
        PieceLoc::Stack(at) => {
            // The end of the part must be expressible too.
            narrow(at.saturating_add(u64::from(size)))?;
            Place::Stack(narrow(at)?)
        }
    };
    if offset.checked_add(size).is_none() {
        return Err(AbiUnsupported(SIZE_LIMIT));
    }
    if let (Some(last), Place::Stack(at)) = (parts.last_mut(), place)
        && let Place::Stack(last_at) = last.place
        && last.arg == arg
        && last.kind == kind
        && !matches!(kind, PartKind::Lanes { .. })
        && last_at + last.size == at
        && last.offset + last.size == offset
    {
        last.size += size;
        return Ok(());
    }
    parts.push(Part { arg, kind, offset, size, place });
    Ok(())
}

/// The parts of one leaf at `offset` in argument `arg`: piece by piece,
/// `place(leaf, piece)` says where it goes, then [`push`].
///
/// # Errors
/// The first error of `place`, or of [`push`] converted.
pub fn place_leaf<C: Copy, E: From<AbiUnsupported>>(
    leaf: &Leaf<C>,
    arg: u32,
    offset: u64,
    parts: &mut Vec<Part>,
    place: &mut impl FnMut(&Leaf<C>, u32) -> Result<PieceLoc, E>,
) -> Result<(), E> {
    for piece in 0..leaf.count {
        let loc = place(leaf, piece)?;
        let at = leaf.piece_offset(offset, piece);
        push(parts, arg, leaf.kind(piece), at, leaf.piece_size(piece), loc)?;
    }
    Ok(())
}

/// The parts of argument `arg` of type `ty`, any type: its leaves in
/// offset order ([`walk`]), each classified, charged against `budget`
/// and placed ([`place_leaf`]).
///
/// # Errors
/// What `classify`, [`charge`], `place` or [`push`] report.
pub fn place_leaves<K: Classify>(
    types: &TypeTable,
    ty: TypeId,
    classify: &K,
    arg: u32,
    budget: &mut u32,
    parts: &mut Vec<Part>,
    place: &mut impl FnMut(&Leaf<K::Class>, u32) -> Result<PieceLoc, AbiUnsupported>,
) -> Result<(), AbiUnsupported> {
    walk(types, ty, 0, &mut |leaf_ty, offset| {
        let leaf = classify.leaf(types, leaf_ty)?;
        charge(budget, leaf.count)?;
        place_leaf(&leaf, arg, offset, parts, place)
    })
}

#[cfg(test)]
mod tests {
    use hod_ir::{DataLayout, X86_64_LINUX_DATA_LAYOUT};

    use super::*;

    /// A toy x86-64-like classification (§7.2, "The leaf walk"): integers
    /// and pointers in general registers, `i128` a pair, `double` in a
    /// vector register.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Toy {
        Gpr,
        Xmm,
    }

    struct ToyLeaves;

    impl Classify for ToyLeaves {
        type Class = Toy;

        fn leaf(&self, types: &TypeTable, ty: TypeId) -> Result<Leaf<Toy>, AbiUnsupported> {
            Ok(match types.kind(ty) {
                TypeKind::Int(128) => Leaf { split: 2, ..Leaf::each(Toy::Gpr, 2, 8) },
                TypeKind::Int(bits) if bits <= 64 => Leaf::one(Toy::Gpr, bits.div_ceil(8)),
                TypeKind::Ptr { addr_space: 0 } => Leaf::one(Toy::Gpr, 8),
                TypeKind::Double => Leaf::one(Toy::Xmm, 8),
                _ => return Err(AbiUnsupported("abi:toy")),
            })
        }
    }

    /// Six general registers; a pair is never split between the last one
    /// and the stack; other leaves go to the stack one by one.
    #[derive(Default)]
    struct ToyState {
        gpr: u8,
        xmm: u8,
        stack: u64,
    }

    impl ToyState {
        fn place(&mut self, leaf: &Leaf<Toy>, piece: u32) -> PieceLoc {
            let (next, regs, base) = match leaf.class {
                Toy::Gpr => (&mut self.gpr, 6, 0),
                Toy::Xmm => (&mut self.xmm, 8, 16),
            };
            if leaf.split_at(piece) && *next + 2 > regs {
                *next = regs;
            }
            if *next < regs {
                *next += 1;
                return PieceLoc::Reg(PReg::new(base + *next - 1));
            }
            let at = self.stack;
            self.stack += 8;
            PieceLoc::Stack(at)
        }
    }

    fn assign_toy(types: &TypeTable, params: &[TypeId]) -> Vec<Part> {
        let mut state = ToyState::default();
        let mut parts = Vec::new();
        let mut budget = 100;
        for (arg, &ty) in params.iter().enumerate() {
            place_leaves(
                types,
                ty,
                &ToyLeaves,
                arg as u32,
                &mut budget,
                &mut parts,
                &mut |l, p| Ok(state.place(l, p)),
            )
            .expect("the toy passes these");
        }
        parts
    }

    fn types() -> TypeTable {
        TypeTable::new(&DataLayout::parse(X86_64_LINUX_DATA_LAYOUT).expect("the layout"))
    }

    fn part(arg: u32, offset: u32, size: u32, place: Place) -> Part {
        Part { arg, kind: PartKind::Value, offset, size, place }
    }

    /// §7.2: an aggregate's leaves may be split between the last register
    /// and the stack, a pair at a `split` piece is not; adjacent stack
    /// parts of one argument merge.
    #[test]
    fn walk_with_a_pair_rule_and_a_split_aggregate() {
        let mut types = types();
        let pair = types.struct_type(&[TypeId::I64, TypeId::I64], false).unwrap();
        let five = [TypeId::I64; 5];
        let parts = assign_toy(&types, &[&five[..], &[pair]].concat());
        assert_eq!(parts[5], part(5, 0, 8, Place::Reg(PReg::new(5))));
        assert_eq!(parts[6], part(5, 8, 8, Place::Stack(0)));

        let parts = assign_toy(&types, &[&five[..], &[TypeId::I128, TypeId::I64]].concat());
        // The pair goes to the stack whole (one merged part), and the
        // sixth register stays unused by this toy's rule.
        assert_eq!(parts[5], part(5, 0, 16, Place::Stack(0)));
        assert_eq!(parts[6], part(6, 0, 8, Place::Stack(16)));
    }

    /// §7.2: leaves in offset order at any depth, padding skipped, empty
    /// arrays skipped, the budget charged per piece, the hook's refusal
    /// reported.
    #[test]
    fn walk_offsets_budget_and_refusal() {
        let mut types = types();
        let empty = types.array(TypeId::I64, 0).unwrap();
        let inner = types.struct_type(&[TypeId::I8, TypeId::DOUBLE], false).unwrap();
        let arr = types.array(inner, 2).unwrap();
        let outer = types.struct_type(&[empty, arr, TypeId::I128], false).unwrap();
        let mut seen = Vec::new();
        walk(&types, outer, 0, &mut |ty, at| {
            seen.push((ty, at));
            Ok::<(), ()>(())
        })
        .unwrap();
        let expect = [
            (TypeId::I8, 0),
            (TypeId::DOUBLE, 8),
            (TypeId::I8, 16),
            (TypeId::DOUBLE, 24),
            (TypeId::I128, 32),
        ];
        assert_eq!(seen, expect);
        assert!(matches!(shape(&types, empty), Shape::Empty));
        assert!(matches!(shape(&types, outer), Shape::Mixed));

        let mut budget = 3;
        let mut parts = Vec::new();
        let err =
            place_leaves(&types, outer, &ToyLeaves, 0, &mut budget, &mut parts, &mut |_, _| {
                Ok(PieceLoc::Stack(0))
            });
        assert_eq!(err, Err(AbiUnsupported(TOO_MANY_PIECES)));
        let half = types.struct_type(&[TypeId::HALF], false).unwrap();
        assert_eq!(check_type(&types, half, &ToyLeaves), Err(AbiUnsupported("abi:toy")));
        let far = push(&mut parts, 0, PartKind::Value, 1 << 32, 1, PieceLoc::Stack(0));
        assert_eq!(far, Err(AbiUnsupported(SIZE_LIMIT)));
    }
}
