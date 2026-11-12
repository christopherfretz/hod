//! Argument assignment for `aarch64-unknown-linux-gnu`: `spec/abi-a64.md`.
//!
//! [`assign`] answers where each byte of each argument and of the return
//! value of a signature travels, exactly as LLVM lowers the same IR-level
//! signature, so that hod-compiled and LLVM-compiled code can call each
//! other. The section numbers in the comments are the spec's.

use hod_cg::PReg;
use hod_cg::abi::{AbiUnsupported, Assignment, Part, PartKind, Place, RetMemory, Sig, VaInfo};
use hod_ir::{CallConv, ParamAttrs, ParamMem, TypeId, TypeKind, TypeTable};

use crate::reg;

/// The largest number of pieces the arguments of one signature may be
/// broken into (§10).
pub const MAX_PIECES: u32 = 1 << 16;

/// The number of argument registers of each class, `x0`..`x7` and
/// `v0`..`v7` (§3.1).
pub const ARG_REGS: u8 = 8;

/// The register that carries the address of a returned-in-memory value
/// and an `sret` argument (§5.2, §6.1).
pub const INDIRECT_RESULT: PReg = reg::X8;

/// The register that carries a `nest` argument (§6.3).
pub const NEST: PReg = reg::X15;

/// Every name [`assign`] reports, in the order of §10.
pub const UNSUPPORTED: [&str; 23] = [
    CC,
    INVALID_SIGNATURE,
    TYPE_X86_FP80,
    SCALABLE_VECTOR,
    TARGET_EXT_TYPE,
    ADDRSPACE,
    VECTOR_ELEMENT,
    VECTOR_PROMOTED,
    WIDE_INT_ARRAY,
    BFLOAT_ARRAY,
    VECTOR_ARRAY,
    ONE_LEAF_ARRAY,
    ATTRIBUTE_AFTER_EMPTY,
    INREG,
    SWIFTSELF,
    SWIFTERROR,
    SWIFTASYNC,
    INALLOCA,
    PREALLOCATED,
    ELEMENTTYPE,
    BYVAL_ALIGN,
    TOO_MANY_PIECES,
    SIZE_LIMIT,
];

const CC: &str = "abi:cc";
const INVALID_SIGNATURE: &str = "abi:invalid-signature";
const TYPE_X86_FP80: &str = "abi:type-x86_fp80";
const SCALABLE_VECTOR: &str = "abi:scalable-vector";
const TARGET_EXT_TYPE: &str = "abi:target-ext-type";
const ADDRSPACE: &str = "abi:addrspace";
const VECTOR_ELEMENT: &str = "abi:vector-element";
const VECTOR_PROMOTED: &str = "abi:vector-promoted";
const WIDE_INT_ARRAY: &str = "abi:wide-int-array";
const BFLOAT_ARRAY: &str = "abi:bfloat-array";
const VECTOR_ARRAY: &str = "abi:vector-array";
const ONE_LEAF_ARRAY: &str = "abi:one-leaf-array";
const ATTRIBUTE_AFTER_EMPTY: &str = "abi:attribute-after-empty-argument";
const INREG: &str = "abi:inreg";
const SWIFTSELF: &str = "abi:swiftself";
const SWIFTERROR: &str = "abi:swifterror";
const SWIFTASYNC: &str = "abi:swiftasync";
const INALLOCA: &str = "abi:inalloca";
const PREALLOCATED: &str = "abi:preallocated";
const ELEMENTTYPE: &str = "abi:elementtype";
const BYVAL_ALIGN: &str = "abi:byval-align";
const TOO_MANY_PIECES: &str = "abi:too-many-pieces";
const SIZE_LIMIT: &str = "abi:size-limit";

type Abi<T> = Result<T, AbiUnsupported>;

fn no<T>(name: &'static str) -> Abi<T> {
    Err(AbiUnsupported(name))
}

/// Whether `cc` passes arguments, returns values and preserves registers
/// exactly as `ccc` does on this target (§1.2).
pub fn cc_is_c(cc: CallConv) -> bool {
    cc == CallConv::C
        || cc == CallConv::FAST
        || cc == CallConv::SWIFT
        || cc == CallConv::CXX_FAST_TLS
}

/// Where the arguments and the return value of `sig` travel
/// (`spec/abi-a64.md`).
///
/// `out` is cleared first and filled on success; its vectors are reused,
/// so a warm `out` makes the call allocation-free. On an error `out` is
/// left empty.
///
/// # Errors
/// [`AbiUnsupported`] with one of the names in [`UNSUPPORTED`] for a
/// signature the specification leaves unsupported, and for one LLVM's
/// verifier would refuse.
///
/// # Panics
/// Panics only for a `TypeId` that is not `types`'s.
pub fn assign(types: &TypeTable, sig: &Sig<'_>, out: &mut Assignment) -> Abi<()> {
    out.clear();
    let result = assign_signature(types, sig, out);
    if result.is_err() {
        out.clear();
    }
    result
}

fn assign_signature(types: &TypeTable, sig: &Sig<'_>, out: &mut Assignment) -> Abi<()> {
    if !cc_is_c(sig.cc) {
        return no(CC);
    }
    let fixed = if sig.variadic { sig.fixed as usize } else { sig.params.len() };
    if fixed > sig.params.len() || u32::try_from(sig.params.len()).is_err() {
        return no(INVALID_SIGNATURE);
    }
    if sig.variadic && sig.cc == CallConv::FAST {
        // §1.2: LLVM's verifier refuses a variadic `fastcc` function.
        return no(INVALID_SIGNATURE);
    }

    let mut state = State { returns_value: sig.ret != TypeId::VOID, ..State::default() };
    if assign_return(types, sig, &mut out.ret)? {
        // §5.2: the hidden pointer is assigned before every argument.
        state.x8 = true;
        out.ret_memory =
            Some(RetMemory { address: Place::Reg(INDIRECT_RESULT), returned_in: None });
    }

    let mut budget = MAX_PIECES;
    let mut empty_seen = false;
    for (index, &ty) in sig.params.iter().enumerate() {
        if index == fixed {
            out.va = state.va_info();
        }
        let attrs = sig.attrs(index);
        let before = out.args.len();
        assign_argument(types, ty, attrs, index as u32, &mut state, &mut out.args, &mut budget)?;
        // §6.5: LLVM's GlobalISel reads the attributes of the parameters
        // from a zero-sized one on at the wrong positions. An argument
        // with no part is such a one.
        empty_seen |= out.args.len() == before;
        if empty_seen && changes_passing(attrs) {
            return no(ATTRIBUTE_AFTER_EMPTY);
        }
    }
    if sig.variadic && fixed == sig.params.len() {
        out.va = state.va_info();
    }
    out.stack_size = u32::try_from(state.nsaa).or(no(SIZE_LIMIT))?;
    Ok(())
}

/// Bits above an integer extended by sign (`signed`) or by zero, up to
/// bit `to` (§7).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Extension {
    pub signed: bool,
    pub to: u32,
}

/// `zeroext` (false) or `signext` (true), if `attrs` has one.
fn extension_attr(attrs: ParamAttrs) -> Option<bool> {
    if attrs.has(ParamAttrs::SIGNEXT) {
        Some(true)
    } else if attrs.has(ParamAttrs::ZEROEXT) {
        Some(false)
    } else {
        None
    }
}

/// §7: what the producer of an argument or return value of type
/// `i<bits>` with the attributes `attrs` must do above the value, in a
/// register and on the stack alike; `None` when it owes nothing. For an
/// integer wider than 64 bits this speaks of its last piece.
pub fn produced_extension(bits: u32, attrs: ParamAttrs) -> Option<Extension> {
    let extension = match (bits, extension_attr(attrs)) {
        // A lone `i1` is a byte holding 0 or 1.
        (1, None) => Extension { signed: false, to: 8 },
        (_, None) => return None,
        (..=32, Some(signed)) => Extension { signed, to: 32 },
        (_, Some(signed)) => Extension { signed, to: bits.checked_next_multiple_of(64)? },
    };
    (extension.to != bits).then_some(extension)
}

/// §7: what the consumer of such a value may assume when its last part
/// is at `place`; `None` when nothing.
pub fn assumed_extension(bits: u32, attrs: ParamAttrs, place: Place) -> Option<Extension> {
    let produced = produced_extension(bits, attrs)?;
    // LLVM's callers store only the low byte, or two, of these on the
    // stack.
    let stored = match (bits, extension_attr(attrs), place) {
        (1 | 8, Some(_), Place::Stack(_)) => 8,
        (16, Some(_), Place::Stack(_)) => 16,
        _ => produced.to,
    };
    (stored != bits).then_some(Extension { signed: produced.signed, to: stored })
}

/// The allocation state of §3.1: the next general register, the next
/// vector register and the next stack offset, plus the two registers
/// outside the argument registers.
#[derive(Default)]
struct State {
    ngrn: u8,
    nsrn: u8,
    nsaa: u64,
    x8: bool,
    x15: bool,
    /// The signature's return type is not `void`.
    returns_value: bool,
}

impl State {
    fn stack(&mut self, size: u64, align: u64) -> u64 {
        let at = self.nsaa.next_multiple_of(align);
        self.nsaa = at.saturating_add(size);
        at
    }

    fn va_info(&self) -> VaInfo {
        VaInfo {
            gpr_used: self.ngrn,
            fpr_used: self.nsrn,
            stack_offset: u32::try_from(self.nsaa.next_multiple_of(8)).unwrap_or(u32::MAX),
        }
    }

    /// §3.2: one piece outside a block.
    fn piece(&mut self, class: Class, split_first: bool) -> Loc {
        if class.is_gpr() {
            if split_first {
                // The even-register rule, and its 16-byte stack alignment.
                let even = (self.ngrn + 1) & !1;
                if even < ARG_REGS {
                    self.ngrn = even + 1;
                    return Loc::Reg(reg::x(even));
                }
                self.ngrn = ARG_REGS;
                return Loc::Stack(self.stack(8, 16));
            }
            if self.ngrn < ARG_REGS {
                self.ngrn += 1;
                return Loc::Reg(reg::x(self.ngrn - 1));
            }
        } else if self.nsrn < ARG_REGS {
            self.nsrn += 1;
            return Loc::Reg(reg::v(self.nsrn - 1));
        }
        let slot = class.slot();
        Loc::Stack(self.stack(slot, slot))
    }

    /// §3.3: a block of `pieces` pieces of one class, all in registers or
    /// all on the stack.
    fn block(&mut self, class: Class, pieces: u64, mem_align: u64) -> Block {
        let next = if class.is_gpr() { &mut self.ngrn } else { &mut self.nsrn };
        if u64::from(*next).saturating_add(pieces) <= u64::from(ARG_REGS) {
            let first = *next;
            *next += pieces as u8;
            return Block::Regs(first);
        }
        *next = ARG_REGS;
        Block::Stack { align: mem_align.clamp(8, 16) }
    }
}

/// Where a piece goes, before the offsets are checked against `u32`.
#[derive(Clone, Copy)]
enum Loc {
    Reg(PReg),
    Stack(u64),
}

enum Block {
    /// The members take registers from this number on.
    Regs(u8),
    /// The members are packed on the stack; the first is aligned.
    Stack { align: u64 },
}

/// The class of a piece (§3.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    /// A general register, for a value of up to 32 bits.
    W,
    /// A general register, for a value of 33 to 64 bits.
    X,
    /// A vector register holding a `half`.
    H,
    /// A vector register holding a `bfloat`.
    Bf,
    /// A vector register holding a `float`.
    S,
    /// A vector register holding 64 bits.
    D,
    /// A vector register holding 128 bits.
    Q,
}

impl Class {
    fn is_gpr(self) -> bool {
        matches!(self, Class::W | Class::X)
    }

    /// The size, and the alignment, of a piece's stack slot outside a
    /// block.
    fn slot(self) -> u64 {
        if self == Class::Q { 16 } else { 8 }
    }

    /// The bytes a member takes on the stack inside a block.
    fn packed(self) -> u64 {
        match self {
            Class::H | Class::Bf => 2,
            Class::W | Class::S => 4,
            Class::X | Class::D => 8,
            Class::Q => 16,
        }
    }
}

/// How one leaf (a scalar or a vector) is broken into pieces (§4): `count`
/// pieces of class `class`; piece `i` holds the bytes of the leaf from
/// `i * stride`, `size` of them (`last_size` for the last piece).
#[derive(Clone, Copy, Debug)]
struct Leaf {
    class: Class,
    count: u32,
    stride: u32,
    size: u32,
    last_size: u32,
    /// The first piece is the first of an integer wider than a register.
    split: bool,
}

impl Leaf {
    fn one(class: Class, size: u32) -> Leaf {
        Leaf { class, count: 1, stride: 0, size, last_size: size, split: false }
    }

    fn each(class: Class, count: u32, size: u32) -> Leaf {
        Leaf { class, count, stride: size, size, last_size: size, split: false }
    }
}

/// §4: the pieces of a leaf type.
fn leaf_of(types: &TypeTable, ty: TypeId) -> Abi<Leaf> {
    Ok(match types.kind(ty) {
        TypeKind::Int(bits) => {
            let bytes = bits.div_ceil(8);
            match bits {
                ..=32 => Leaf::one(Class::W, bytes),
                33..=64 => Leaf::one(Class::X, bytes),
                _ => {
                    let count = bits.div_ceil(64);
                    Leaf {
                        class: Class::X,
                        count,
                        stride: 8,
                        size: 8,
                        last_size: bytes - 8 * (count - 1),
                        split: true,
                    }
                }
            }
        }
        TypeKind::Ptr { addr_space: 0 } => Leaf::one(Class::X, 8),
        TypeKind::Ptr { .. } => return no(ADDRSPACE),
        TypeKind::Half => Leaf::one(Class::H, 2),
        TypeKind::BFloat => Leaf::one(Class::Bf, 2),
        TypeKind::Float => Leaf::one(Class::S, 4),
        TypeKind::Double => Leaf::one(Class::D, 8),
        TypeKind::Fp128 => Leaf::one(Class::Q, 16),
        TypeKind::Vector { elem, len } => vector_leaf(types, elem, len)?,
        TypeKind::X86Fp80 => return no(TYPE_X86_FP80),
        TypeKind::ScalableVector { .. } => return no(SCALABLE_VECTOR),
        TypeKind::TargetExt { .. } => return no(TARGET_EXT_TYPE),
        TypeKind::Void
        | TypeKind::Label
        | TypeKind::Metadata
        | TypeKind::Token
        | TypeKind::Opaque(_)
        | TypeKind::Function { .. }
        | TypeKind::Array { .. }
        | TypeKind::Struct { .. } => return no(INVALID_SIGNATURE),
    })
}

/// §4.3: the pieces of a fixed vector of `len` elements of type `elem`.
fn vector_leaf(types: &TypeTable, elem: TypeId, len: u32) -> Abi<Leaf> {
    // The element as a scalar, and whether it is an integer.
    let (scalar, bytes, integer) = match types.kind(elem) {
        TypeKind::Int(8) => (Class::W, 1, true),
        TypeKind::Int(16) => (Class::W, 2, true),
        TypeKind::Int(32) => (Class::W, 4, true),
        // A lone 64-bit element is a one-element vector, which is legal.
        TypeKind::Int(64) | TypeKind::Ptr { addr_space: 0 } => (Class::D, 8, true),
        TypeKind::Half => (Class::H, 2, false),
        TypeKind::BFloat => (Class::Bf, 2, false),
        TypeKind::Float => (Class::S, 4, false),
        TypeKind::Double => (Class::D, 8, false),
        TypeKind::Fp128 => return Ok(Leaf::each(Class::Q, len, 16)),
        TypeKind::Ptr { .. } => return no(ADDRSPACE),
        _ => return no(VECTOR_ELEMENT),
    };
    let Some(total) = len.checked_mul(bytes) else { return no(SIZE_LIMIT) };
    let whole = |total: u32| match total {
        8 => Some(Class::D),
        16 => Some(Class::Q),
        _ => None,
    };
    if let Some(class) = whole(total) {
        // A legal vector.
        return Ok(Leaf::one(class, total));
    }
    if len == 1 {
        // `half` and `bfloat` are scalarized; the others are widened to a
        // 64-bit vector.
        let class = if bytes == 2 && !integer { scalar } else { Class::D };
        return Ok(Leaf::one(class, bytes));
    }
    if len.is_power_of_two() {
        if total > 16 {
            // Split into 128-bit vectors.
            return Ok(Leaf::each(Class::Q, total / 16, 16));
        }
        // Shorter than 64 bits: integer elements are promoted, which
        // changes the layout; floating-point elements are widened.
        return if integer { no(VECTOR_PROMOTED) } else { Ok(Leaf::one(Class::D, total)) };
    }
    // Not a power of two: widened to the next power of two if that is a
    // legal vector, and passed element by element otherwise.
    let widened = len.checked_next_power_of_two().and_then(|n| n.checked_mul(bytes));
    Ok(match widened.and_then(whole) {
        Some(class) => Leaf::one(class, total),
        None => Leaf::each(scalar, len, bytes),
    })
}

/// A number that is equal for two leaf types exactly when LLVM gives them
/// one value type (§3.3): a pointer is an `i64`.
fn leaf_key(types: &TypeTable, ty: TypeId) -> u64 {
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

/// The leaves of a type, as far as §3.3 needs them.
#[derive(Clone, Copy)]
enum Shape {
    /// No leaf.
    Empty,
    /// `leaves` leaves of one value type, `ty` being one of them.
    Uniform { key: u64, ty: TypeId, leaves: u64 },
    /// Leaves of more than one value type.
    Mixed,
}

/// The shape of `ty`, in time proportional to the type's description,
/// not to its number of leaves.
fn shape(types: &TypeTable, ty: TypeId) -> Shape {
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

/// Refuses a type that has a leaf §4 does not pass, in time proportional
/// to the type's description.
fn check_type(types: &TypeTable, ty: TypeId) -> Abi<()> {
    match types.kind(ty) {
        TypeKind::Struct { fields, .. } => fields.iter().try_for_each(|&f| check_type(types, f)),
        TypeKind::Array { elem, .. } => check_type(types, elem),
        _ => leaf_of(types, ty).map(|_| ()),
    }
}

/// Calls `visit` with each leaf of `ty` and its byte offset in the value,
/// in offset order (§4.4).
fn walk<E>(
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
            // Every element has a leaf, and every leaf is charged against
            // the piece budget, so this loop is bounded by it.
            let stride = types.alloc_size_of(elem);
            for index in 0..len {
                walk(types, elem, base.saturating_add(index.saturating_mul(stride)), visit)?;
            }
            Ok(())
        }
        _ => visit(ty, base),
    }
}

fn charge(budget: &mut u32, pieces: u32) -> Abi<()> {
    match budget.checked_sub(pieces) {
        Some(left) => {
            *budget = left;
            Ok(())
        }
        None => no(TOO_MANY_PIECES),
    }
}

/// Appends a part; a stack part that continues the previous one, in the
/// value and on the stack, extends it instead (§3.4).
fn push(
    parts: &mut Vec<Part>,
    arg: u32,
    kind: PartKind,
    offset: u64,
    size: u32,
    loc: Loc,
) -> Abi<()> {
    let narrow = |n: u64| u32::try_from(n).or(no(SIZE_LIMIT));
    let offset = narrow(offset)?;
    let place = match loc {
        Loc::Reg(reg) => Place::Reg(reg),
        Loc::Stack(at) => {
            // The end of the part must be expressible too.
            narrow(at.saturating_add(u64::from(size)))?;
            Place::Stack(narrow(at)?)
        }
    };
    if offset.checked_add(size).is_none() {
        return no(SIZE_LIMIT);
    }
    if let (Some(last), Place::Stack(at)) = (parts.last_mut(), place)
        && let Place::Stack(last_at) = last.place
        && last.arg == arg
        && last.kind == kind
        && last_at + last.size == at
        && last.offset + last.size == offset
    {
        last.size += size;
        return Ok(());
    }
    parts.push(Part { arg, kind, offset, size, place });
    Ok(())
}

fn is_plain_pointer(types: &TypeTable, ty: TypeId) -> bool {
    matches!(types.kind(ty), TypeKind::Ptr { addr_space: 0 })
}

/// The attributes §6.6 leaves unsupported or that no valid signature
/// has.
fn check_flags(types: &TypeTable, ty: TypeId, attrs: ParamAttrs) -> Abi<()> {
    if attrs.is_none() {
        return Ok(());
    }
    if attrs.has(ParamAttrs::INREG) {
        return no(INREG);
    }
    if attrs.has(ParamAttrs::SWIFT_SELF) {
        return no(SWIFTSELF);
    }
    if attrs.has(ParamAttrs::SWIFT_ERROR) {
        return no(SWIFTERROR);
    }
    if attrs.has(ParamAttrs::SWIFT_ASYNC) {
        return no(SWIFTASYNC);
    }
    let extended = attrs.has(ParamAttrs::ZEROEXT) || attrs.has(ParamAttrs::SIGNEXT);
    if extended && !types.is_int(ty) {
        return no(INVALID_SIGNATURE);
    }
    Ok(())
}

/// Whether an attribute set changes where or how its argument travels.
fn changes_passing(attrs: ParamAttrs) -> bool {
    !attrs.is_none()
        && (attrs.has(ParamAttrs::ZEROEXT)
            || attrs.has(ParamAttrs::SIGNEXT)
            || attrs.has(ParamAttrs::NEST)
            || attrs.stack_align().is_some()
            || matches!(attrs.mem(), ParamMem::Sret | ParamMem::ByVal))
}

fn is_aggregate(types: &TypeTable, ty: TypeId) -> bool {
    matches!(types.kind(ty), TypeKind::Struct { .. } | TypeKind::Array { .. })
}

/// The return value does not fit the return registers.
struct Memory;

/// §5: the return value. `Ok(true)` when it travels in memory.
fn assign_return(types: &TypeTable, sig: &Sig<'_>, parts: &mut Vec<Part>) -> Abi<bool> {
    let attrs = sig.ret_attrs;
    check_flags(types, sig.ret, attrs)?;
    if attrs.has(ParamAttrs::NEST) || attrs.mem() != ParamMem::None {
        return no(INVALID_SIGNATURE);
    }
    if sig.ret == TypeId::VOID {
        return Ok(false);
    }

    // §5.1: the next register of each class; no even-register rule.
    let (mut general, mut vector) = (0, 0);
    let mut place = |leaf: Leaf, offset: u64, parts: &mut Vec<Part>| {
        let next = if leaf.class.is_gpr() { &mut general } else { &mut vector };
        if u64::from(*next) + u64::from(leaf.count) > u64::from(ARG_REGS) {
            return Err(Memory);
        }
        for piece in 0..leaf.count {
            let reg = if leaf.class.is_gpr() { reg::x(*next) } else { reg::v(*next) };
            *next += 1;
            let size = if piece + 1 == leaf.count { leaf.last_size } else { leaf.size };
            let at = offset.saturating_add(u64::from(piece) * u64::from(leaf.stride));
            // An offset beyond `u32`: such a value has far more than
            // eight leaves or is mostly padding; memory serves both.
            push(parts, 0, PartKind::Value, at, size, Loc::Reg(reg)).map_err(|_| Memory)?;
        }
        Ok(())
    };
    let fits = if is_aggregate(types, sig.ret) {
        check_type(types, sig.ret)?;
        walk(types, sig.ret, 0, &mut |leaf_ty, offset| {
            let leaf = leaf_of(types, leaf_ty).map_err(|_| Memory)?;
            place(leaf, offset, parts)
        })
    } else {
        place(leaf_of(types, sig.ret)?, 0, parts)
    };
    if fits.is_err() {
        // §5.2.
        parts.clear();
    }
    Ok(fits.is_err())
}

/// §6: one argument.
fn assign_argument(
    types: &TypeTable,
    ty: TypeId,
    attrs: ParamAttrs,
    arg: u32,
    state: &mut State,
    parts: &mut Vec<Part>,
    budget: &mut u32,
) -> Abi<()> {
    if attrs.is_none() {
        return assign_value(types, ty, None, arg, state, parts, budget);
    }
    check_flags(types, ty, attrs)?;
    let nest = attrs.has(ParamAttrs::NEST);
    match attrs.mem() {
        ParamMem::None => {}
        // §6.4: an ordinary pointer.
        ParamMem::ByRef if types.is_ptr(ty) => {}
        ParamMem::ByRef => return no(INVALID_SIGNATURE),
        ParamMem::InAlloca => return no(INALLOCA),
        ParamMem::Preallocated => return no(PREALLOCATED),
        ParamMem::ElementType => return no(ELEMENTTYPE),
        ParamMem::Sret => {
            // §6.1. LLVM's verifier wants one `sret` at most and a `void`
            // return with it, so the register is free.
            if nest || !is_plain_pointer(types, ty) || state.x8 || state.returns_value {
                return no(INVALID_SIGNATURE);
            }
            state.x8 = true;
            charge(budget, 1)?;
            return push(parts, arg, PartKind::Value, 0, 8, Loc::Reg(INDIRECT_RESULT));
        }
        ParamMem::ByVal => {
            // §6.2.
            let pointee = attrs.mem_type().unwrap_or(TypeId::VOID);
            if nest || !is_plain_pointer(types, ty) || !types.is_sized(pointee) {
                return no(INVALID_SIGNATURE);
            }
            if types.is_scalable(pointee) {
                return no(SCALABLE_VECTOR);
            }
            let align = attrs.stack_align().or(attrs.align()).unwrap_or(types.align_of(pointee));
            if align > 16 {
                return no(BYVAL_ALIGN);
            }
            let size = types.alloc_size_of(pointee);
            let Ok(part_size) = u32::try_from(size) else { return no(SIZE_LIMIT) };
            let at = state.stack(size.max(8).next_multiple_of(8), align.max(8));
            charge(budget, 1)?;
            return push(parts, arg, PartKind::Pointee, 0, part_size, Loc::Stack(at));
        }
    }
    if nest {
        // §6.3. A second `nest` is not valid IR.
        if !is_plain_pointer(types, ty) || state.x15 {
            return no(INVALID_SIGNATURE);
        }
        state.x15 = true;
        charge(budget, 1)?;
        return push(parts, arg, PartKind::Value, 0, 8, Loc::Reg(NEST));
    }
    assign_value(types, ty, attrs.stack_align(), arg, state, parts, budget)
}

/// §3: an argument passed by value; `stack_align` is its `alignstack`.
fn assign_value(
    types: &TypeTable,
    ty: TypeId,
    stack_align: Option<u64>,
    arg: u32,
    state: &mut State,
    parts: &mut Vec<Part>,
    budget: &mut u32,
) -> Abi<()> {
    let is_array = match types.kind(ty) {
        TypeKind::Array { .. } => true,
        TypeKind::Struct { .. } => false,
        _ => {
            // §3.2: a scalar or a vector, piece by piece.
            let leaf = leaf_of(types, ty)?;
            charge(budget, leaf.count)?;
            for piece in 0..leaf.count {
                let loc = state.piece(leaf.class, leaf.split && piece == 0);
                let size = if piece + 1 == leaf.count { leaf.last_size } else { leaf.size };
                let at = u64::from(piece) * u64::from(leaf.stride);
                push(parts, arg, PartKind::Value, at, size, loc)?;
            }
            return Ok(());
        }
    };
    check_type(types, ty)?;

    // §3.3: an array whose leaves all have one value type is a block.
    let mut block = None;
    if is_array && let Shape::Uniform { ty: leaf_ty, leaves, .. } = shape(types, ty) {
        let leaf = leaf_of(types, leaf_ty)?;
        match leaf.class {
            // LLVM does not treat these as a block.
            Class::W => {}
            Class::Bf => return no(BFLOAT_ARRAY),
            Class::X if leaf.split => return no(WIDE_INT_ARRAY),
            // A vector passed in several registers: LLVM's two instruction
            // selectors disagree about a block of them.
            _ if leaf.count > 1 => return no(VECTOR_ARRAY),
            class => {
                let mem_align = stack_align.unwrap_or(types.align_of(ty));
                if leaves == 1 && class.slot() != mem_align.clamp(8, 16) {
                    // One selector passes a block, the other a plain
                    // value: they differ in the stack alignment.
                    return no(ONE_LEAF_ARRAY);
                }
                let placed = state.block(class, leaves, mem_align);
                if let Block::Stack { align } = placed
                    && u64::from(leaf.size) == class.packed()
                    && leaves.checked_mul(class.packed()) == Some(types.alloc_size_of(ty))
                {
                    // The members are the value's bytes with no gap: one
                    // part, however many members there are.
                    let size = types.alloc_size_of(ty);
                    let Ok(part_size) = u32::try_from(size) else { return no(SIZE_LIMIT) };
                    let at = state.stack(size, align);
                    charge(budget, 1)?;
                    return push(parts, arg, PartKind::Value, 0, part_size, Loc::Stack(at));
                }
                block = Some(placed);
            }
        }
    }

    walk(types, ty, 0, &mut |leaf_ty, offset| {
        let leaf = leaf_of(types, leaf_ty)?;
        charge(budget, leaf.count)?;
        for piece in 0..leaf.count {
            let loc = match &mut block {
                None => state.piece(leaf.class, leaf.split && piece == 0),
                Some(Block::Regs(next)) => {
                    *next += 1;
                    let number = *next - 1;
                    Loc::Reg(if leaf.class.is_gpr() { reg::x(number) } else { reg::v(number) })
                }
                Some(Block::Stack { align }) => {
                    let at = state.stack(leaf.class.packed(), *align);
                    *align = 1;
                    Loc::Stack(at)
                }
            };
            let size = if piece + 1 == leaf.count { leaf.last_size } else { leaf.size };
            let at = offset.saturating_add(u64::from(piece) * u64::from(leaf.stride));
            push(parts, arg, PartKind::Value, at, size, loc)?;
        }
        Ok(())
    })
}
