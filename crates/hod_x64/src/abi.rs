//! Argument assignment for `x86_64-unknown-linux-gnu`: `spec/abi-x64.md`.
//!
//! [`assign`] answers where each byte of each argument and of the return
//! value of a signature travels, exactly as `llc` 22 lowers the same
//! IR-level signature, so that hod-compiled and LLVM-compiled code can
//! call each other. The section numbers in the comments are the spec's.
//!
//! The work is split as `hod_cg`'s shared walk will split it (WP-117):
//! [`classify`] is the per-target hook that turns one leaf type into
//! pieces of a register class, and the private `walk` module visits the
//! leaves of an aggregate. Placing the pieces (registers, stack slots,
//! the return registers) is this file's.

use hod_cg::PReg;
use hod_cg::abi::{AbiUnsupported, Assignment, Part, PartKind, Place, RetMemory, Sig, VaInfo};
use hod_ir::{CallConv, ParamAttrs, ParamMem, TypeId, TypeKind, TypeTable};

// The register numbering (§2). PROVISIONAL: WP-116 owns `hod_x64::reg`;
// these re-point at it when it lands, with the same numbers or not.

/// The general register with hardware encoding `n` (`rax` 0, `rcx` 1,
/// `rdx` 2, `rbx` 3, `rsp` 4, `rbp` 5, `rsi` 6, `rdi` 7, `r8`..`r15`).
///
/// # Panics
/// Panics if `n` is 16 or more.
pub const fn gpr(n: u8) -> PReg {
    assert!(n < 16, "the general registers are 0..16");
    PReg::new(n)
}

/// `xmm<n>`.
///
/// # Panics
/// Panics if `n` is 16 or more.
pub const fn xmm(n: u8) -> PReg {
    assert!(n < 16, "the vector registers are xmm0..xmm15");
    PReg::new(16 + n)
}

/// `st(<n>)`, the x87 stack slot `n` from the top.
///
/// # Panics
/// Panics if `n` is 8 or more.
pub const fn st(n: u8) -> PReg {
    assert!(n < 8, "the x87 stack is st0..st7");
    PReg::new(32 + n)
}

/// Whether `reg` is a general register.
pub const fn is_gpr(reg: PReg) -> bool {
    reg.index() < 16
}

/// Whether `reg` is one of `xmm0`..`xmm15`.
pub const fn is_xmm(reg: PReg) -> bool {
    reg.index() >= 16 && reg.index() < 32
}

/// Whether `reg` is one of `st0`..`st7`.
pub const fn is_x87(reg: PReg) -> bool {
    reg.index() >= 32 && reg.index() < 40
}

/// The number of `reg` within its file: the encoding of a general
/// register, `n` of `xmm<n>` or of `st(<n>)`.
pub const fn number(reg: PReg) -> u8 {
    reg.index() % 16
}

/// The DWARF register number of `reg` (psABI §3.6.2, figure 3.36):
/// `rax` 0, `rdx` 1, `rcx` 2, `rbx` 3, `rsi` 4, `rdi` 5, `rbp` 6, `rsp` 7,
/// `r8`..`r15` 8..=15, `xmm0`..`xmm15` 17..=32, `st0`..`st7` 33..=40.
/// `None` for an index that names no register. (The return address is
/// column 16, which no `PReg` names.)
pub const fn dwarf_number(reg: PReg) -> Option<u16> {
    const LOW: [u16; 8] = [0, 2, 1, 3, 7, 6, 4, 5];
    let index = reg.index();
    if index < 8 {
        Some(LOW[index as usize])
    } else if index < 16 {
        Some(index as u16)
    } else if index < 40 {
        // `xmm<n>` (16 + n) is 17 + n, `st<n>` (32 + n) is 33 + n.
        Some(index as u16 + 1)
    } else {
        None
    }
}

pub const RAX: PReg = gpr(0);
pub const RCX: PReg = gpr(1);
pub const RDX: PReg = gpr(2);
pub const RSI: PReg = gpr(6);
pub const RDI: PReg = gpr(7);
pub const R8: PReg = gpr(8);
pub const R9: PReg = gpr(9);
pub const R10: PReg = gpr(10);
pub const ST0: PReg = st(0);
pub const ST1: PReg = st(1);

/// The general argument registers, in order (§3.1).
pub const ARG_GPRS: [PReg; 6] = [RDI, RSI, RDX, RCX, R8, R9];

/// The number of vector argument registers, `xmm0`..`xmm7` (§3.1).
pub const ARG_XMMS: u8 = 8;

/// The general return registers, in order (§5.1).
pub const RET_GPRS: [PReg; 3] = [RAX, RDX, RCX];

/// The register that carries the address of a returned-in-memory value
/// (§5.2): the first argument register.
pub const INDIRECT_RESULT: PReg = RDI;

/// The register in which a callee hands back the address of its
/// returned-in-memory value or its `sret` argument (§5.2, §6.1).
pub const RETURNS_ADDRESS: PReg = RAX;

/// The register LLVM passes a `nest` argument in (§6.3); `assign`
/// refuses `nest` ([`NEST`]).
pub const NEST_REG: PReg = R10;

/// The largest number of pieces the arguments of one signature may be
/// broken into (§10).
pub const MAX_PIECES: u32 = 1 << 16;

/// The target features that change the convention (§1.3). Every other
/// feature leaves it as it is.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Features {
    /// `+avx` (or anything implying it): vectors wider than 128 bits
    /// travel in `ymm` registers.
    pub avx: bool,
    /// `+avx512f` (or anything implying it): 512-bit vectors in `zmm`,
    /// and vectors of `i1` in mask registers.
    pub avx512: bool,
}

/// Every name [`assign`] reports, in the order of §10.
pub const UNSUPPORTED: [&str; 19] = [
    CC,
    INVALID_SIGNATURE,
    SCALABLE_VECTOR,
    TARGET_EXT_TYPE,
    ADDRSPACE,
    VECTOR_ELEMENT,
    MASK_VECTOR,
    WIDE_VECTOR_ABI,
    RET_X87_PART,
    NEST,
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
const SCALABLE_VECTOR: &str = "abi:scalable-vector";
const TARGET_EXT_TYPE: &str = "abi:target-ext-type";
const ADDRSPACE: &str = "abi:addrspace";
const VECTOR_ELEMENT: &str = "abi:vector-element";
/// A vector of `i1` that LLVM passes element by element (§4.3).
pub const MASK_VECTOR: &str = "x64:mask-vector";
/// A vector wider than 128 bits under AVX, or of `i1` under AVX-512
/// (§1.3).
pub const WIDE_VECTOR_ABI: &str = "x64:wide-vector-abi";
/// A return value a part of which LLVM returns in `st0`/`st1` although it
/// is not an `x86_fp80` (§5.1).
pub const RET_X87_PART: &str = "x64:ret-x87-part";
/// A `nest` argument (§6.3).
pub const NEST: &str = "x64:nest";
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
    cc == CallConv::C || cc == CallConv::FAST
}

/// Where the arguments and the return value of `sig` travel
/// (`spec/abi-x64.md`), for a function compiled with `features`.
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
pub fn assign(
    types: &TypeTable,
    sig: &Sig<'_>,
    features: Features,
    out: &mut Assignment,
) -> Abi<()> {
    out.clear();
    let result = assign_signature(types, sig, features, out);
    if result.is_err() {
        out.clear();
    }
    result
}

fn assign_signature(
    types: &TypeTable,
    sig: &Sig<'_>,
    features: Features,
    out: &mut Assignment,
) -> Abi<()> {
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
    if assign_return(types, sig, features, &mut out.ret)? {
        // §5.2: the hidden pointer is the first argument.
        state.gpr = 1;
        out.ret_memory =
            Some(RetMemory { address: Place::Reg(INDIRECT_RESULT), returned_in: Some(RAX) });
    }

    let mut budget = MAX_PIECES;
    for (index, &ty) in sig.params.iter().enumerate() {
        if index == fixed {
            out.va = state.va_info();
        }
        let attrs = sig.attrs(index);
        let arg = index as u32;
        if attrs.mem() == ParamMem::Sret {
            out.sret_returned_in = Some(RETURNS_ADDRESS);
        }
        assign_argument(types, ty, attrs, arg, features, &mut state, &mut out.args, &mut budget)?;
    }
    if sig.variadic && fixed == sig.params.len() {
        out.va = state.va_info();
    }
    out.vector_regs_used = state.xmm;
    out.stack_size = u32::try_from(state.stack).or(no(SIZE_LIMIT))?;
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

/// §7: what the producer of an argument (`ret` false) or of the return
/// value (`ret` true) of type `i<bits>` with the attributes `attrs` must
/// do above the value, in a register and on the stack alike; `None` when
/// it owes nothing. For an integer wider than 64 bits this speaks of its
/// last piece.
pub fn produced_extension(bits: u32, attrs: ParamAttrs, ret: bool) -> Option<Extension> {
    let signed = extension_attr(attrs)?;
    let to = match bits {
        // An argument is promoted to 32 bits; a return value only to the
        // next of 8, 16 and 32.
        ..=32 if !ret => 32,
        ..=8 => 8,
        9..=16 => 16,
        17..=32 => 32,
        33..=64 => 64,
        _ => bits.checked_next_multiple_of(64)?,
    };
    (to != bits).then_some(Extension { signed, to })
}

/// §7: what the consumer of such a value may assume when its last part
/// is at `place`; `None` when nothing. It is what the producer owes,
/// wherever the value is.
pub fn assumed_extension(
    bits: u32,
    attrs: ParamAttrs,
    ret: bool,
    place: Place,
) -> Option<Extension> {
    let _ = place;
    produced_extension(bits, attrs, ret)
}

/// The register class of a piece (§4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// A general register; an 8-byte stack slot.
    Gpr,
    /// `half` or `bfloat` in the low lane of an `xmm`; an 8-byte slot.
    Half,
    /// `float` or `double` in the low lane of an `xmm`; an 8-byte slot.
    Float,
    /// `fp128` in an `xmm`; a 16-byte slot aligned to 16.
    Quad,
    /// A vector in an `xmm`; a 16-byte slot aligned to 16.
    Vector,
    /// `x86_fp80`: always a 16-byte slot aligned to 16 as an argument,
    /// `st0`/`st1` as a return value.
    X87,
}

impl Class {
    fn is_xmm(self) -> bool {
        matches!(self, Class::Half | Class::Float | Class::Quad | Class::Vector)
    }

    /// The size, and the alignment, of the piece's stack slot.
    fn slot(self) -> u64 {
        match self {
            Class::Gpr | Class::Half | Class::Float => 8,
            Class::Quad | Class::Vector | Class::X87 => 16,
        }
    }
}

/// How one leaf type travels (§4): `count` pieces of class `class`;
/// piece `i` holds the bytes of the leaf from `i * stride`, `size` of
/// them (`last_size` for the last piece).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Leaf {
    pub class: Class,
    pub count: u32,
    pub stride: u32,
    pub size: u32,
    pub last_size: u32,
    /// The leaf is an `i128`: passed alone, its two pieces take two
    /// general registers or one 16-byte slot, never one of each (§4.2).
    pub pair: bool,
    /// The leaf is a vector of `len` `i1` whose element `i` travels in
    /// the low bit of lane `i` (`lane` bytes) of the one piece (§4.3).
    pub mask: Option<(u8, u8)>,
}

impl Leaf {
    fn one(class: Class, size: u32) -> Leaf {
        Leaf { class, count: 1, stride: 0, size, last_size: size, pair: false, mask: None }
    }

    fn each(class: Class, count: u32, size: u32) -> Leaf {
        Leaf { class, count, stride: size, size, last_size: size, pair: false, mask: None }
    }

    fn piece_size(&self, piece: u32) -> u32 {
        if piece + 1 == self.count { self.last_size } else { self.size }
    }

    fn kind(&self) -> PartKind {
        match self.mask {
            None => PartKind::Value,
            Some((lane, len)) => PartKind::Lanes { bits: 1, lane, first: 0, len },
        }
    }
}

/// §4: the pieces of a leaf type (not a structure or an array), for a
/// function compiled with `features`. This is the classification hook
/// the shared leaf walk calls.
///
/// # Errors
/// The name of §10 for a type that is not a leaf or not supported.
pub fn classify(types: &TypeTable, ty: TypeId, features: Features) -> Abi<Leaf> {
    Ok(match types.kind(ty) {
        TypeKind::Int(bits) => {
            let bytes = bits.div_ceil(8);
            if bits <= 64 {
                Leaf::one(Class::Gpr, bytes)
            } else {
                let count = bits.div_ceil(64);
                Leaf {
                    class: Class::Gpr,
                    count,
                    stride: 8,
                    size: 8,
                    last_size: bytes - 8 * (count - 1),
                    pair: bits == 128,
                    mask: None,
                }
            }
        }
        TypeKind::Ptr { addr_space: 0 } => Leaf::one(Class::Gpr, 8),
        TypeKind::Ptr { .. } => return no(ADDRSPACE),
        TypeKind::Half | TypeKind::BFloat => Leaf::one(Class::Half, 2),
        TypeKind::Float => Leaf::one(Class::Float, 4),
        TypeKind::Double => Leaf::one(Class::Float, 8),
        TypeKind::Fp128 => Leaf::one(Class::Quad, 16),
        TypeKind::X86Fp80 => Leaf::one(Class::X87, 10),
        TypeKind::Vector { elem, len } => vector_leaf(types, elem, len, features)?,
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
fn vector_leaf(types: &TypeTable, elem: TypeId, len: u32, features: Features) -> Abi<Leaf> {
    let (scalar, bytes) = match types.kind(elem) {
        TypeKind::Int(8) => (Class::Gpr, 1),
        TypeKind::Int(16) => (Class::Gpr, 2),
        TypeKind::Int(32) => (Class::Gpr, 4),
        TypeKind::Int(64) | TypeKind::Ptr { addr_space: 0 } => (Class::Gpr, 8),
        TypeKind::Half | TypeKind::BFloat => (Class::Half, 2),
        TypeKind::Float => (Class::Float, 4),
        TypeKind::Double => (Class::Float, 8),
        // Element by element, each an `fp128`.
        TypeKind::Fp128 => return Ok(Leaf::each(Class::Quad, len, 16)),
        TypeKind::Int(1) => return mask_leaf(len, features),
        TypeKind::Ptr { .. } => return no(ADDRSPACE),
        _ => return no(VECTOR_ELEMENT),
    };
    if len == 1 {
        // Scalarized, except `<1 x half>` and `<1 x bfloat>`, which LLVM
        // widens to a 128-bit vector (a 16-byte stack slot, `xmm3` among
        // the return registers).
        let class = if scalar == Class::Half { Class::Vector } else { scalar };
        return Ok(Leaf::one(class, bytes));
    }
    let Some(total) = len.checked_mul(bytes) else { return no(SIZE_LIMIT) };
    let widened = len.checked_next_power_of_two().and_then(|n| n.checked_mul(bytes));
    // A legal 128-bit vector, or one LLVM widens to one in a single step.
    let one_register = if len.is_power_of_two() {
        total <= 16
    } else {
        widened == Some(16) || (scalar == Class::Half && len == 3)
    };
    if one_register {
        return Ok(Leaf::one(Class::Vector, total));
    }
    if features.avx && total > 16 {
        // §1.3: `ymm`/`zmm`.
        return no(WIDE_VECTOR_ABI);
    }
    Ok(if len.is_power_of_two() {
        // Split into 128-bit vectors.
        Leaf::each(Class::Vector, total / 16, 16)
    } else {
        // Element by element.
        Leaf::each(scalar, len, bytes)
    })
}

/// §4.3: `<len x i1>`: one element as an `i1`; 2, 4, 8 or 16 promoted to
/// the lanes of one `xmm` (`v2i64`, `v4i32`, `v8i16`, `v16i8`).
fn mask_leaf(len: u32, features: Features) -> Abi<Leaf> {
    if features.avx512 {
        return no(WIDE_VECTOR_ABI);
    }
    Ok(match len {
        1 => Leaf::one(Class::Gpr, 1),
        2 | 4 | 8 | 16 => {
            Leaf { mask: Some(((16 / len) as u8, len as u8)), ..Leaf::one(Class::Vector, 16) }
        }
        _ => return no(MASK_VECTOR),
    })
}

// TEMPORARY: replaced by hod_cg's walk when WP-117 lands. The walk over
// the leaves of an aggregate (§4.4), a copy of the one `hod_a64` uses.
mod walk {
    use hod_ir::{TypeId, TypeKind, TypeTable};

    /// Whether `ty` has at least one leaf, in time proportional to the
    /// type's description.
    pub(super) fn has_leaf(types: &TypeTable, ty: TypeId) -> bool {
        match types.kind(ty) {
            TypeKind::Struct { fields, .. } => fields.iter().any(|&f| has_leaf(types, f)),
            TypeKind::Array { elem, len } => len > 0 && has_leaf(types, elem),
            _ => true,
        }
    }

    /// Calls `check` with each distinct leaf type of `ty`, in time
    /// proportional to the type's description.
    pub(super) fn each_leaf_type<E>(
        types: &TypeTable,
        ty: TypeId,
        check: &mut impl FnMut(TypeId) -> Result<(), E>,
    ) -> Result<(), E> {
        match types.kind(ty) {
            TypeKind::Struct { fields, .. } => {
                fields.iter().try_for_each(|&f| each_leaf_type(types, f, check))
            }
            TypeKind::Array { elem, .. } => each_leaf_type(types, elem, check),
            _ => check(ty),
        }
    }

    /// Calls `visit` with each leaf of `ty` and its byte offset in the
    /// value, in offset order. The caller bounds the work: `visit` is
    /// called once per leaf and may fail.
    pub(super) fn leaves<E>(
        types: &TypeTable,
        ty: TypeId,
        base: u64,
        visit: &mut impl FnMut(TypeId, u64) -> Result<(), E>,
    ) -> Result<(), E> {
        match types.kind(ty) {
            TypeKind::Struct { fields, .. } => {
                let offsets = types.field_offsets(ty).unwrap_or(&[]);
                for (&field, &offset) in fields.iter().zip(offsets) {
                    leaves(types, field, base.saturating_add(offset), visit)?;
                }
                Ok(())
            }
            TypeKind::Array { elem, len } => {
                if !has_leaf(types, elem) {
                    return Ok(());
                }
                let stride = types.alloc_size_of(elem);
                for index in 0..len {
                    leaves(types, elem, base.saturating_add(index.saturating_mul(stride)), visit)?;
                }
                Ok(())
            }
            _ => visit(ty, base),
        }
    }
}

/// The allocation state of §3.1: the next general register, the next
/// vector register and the next stack offset.
#[derive(Default)]
struct State {
    gpr: u8,
    xmm: u8,
    stack: u64,
    /// The signature's return type is not `void`.
    returns_value: bool,
}

/// Where a piece goes, before the offsets are checked against `u32`.
#[derive(Clone, Copy)]
enum Loc {
    Reg(PReg),
    Stack(u64),
}

impl State {
    fn stack(&mut self, size: u64, align: u64) -> u64 {
        let at = self.stack.next_multiple_of(align);
        self.stack = at.saturating_add(size);
        at
    }

    fn va_info(&self) -> VaInfo {
        VaInfo {
            gpr_used: self.gpr,
            fpr_used: self.xmm,
            stack_offset: u32::try_from(self.stack.next_multiple_of(8)).unwrap_or(u32::MAX),
        }
    }

    /// §3.2: one piece.
    fn piece(&mut self, class: Class) -> Loc {
        match class {
            Class::Gpr if (self.gpr as usize) < ARG_GPRS.len() => {
                self.gpr += 1;
                return Loc::Reg(ARG_GPRS[self.gpr as usize - 1]);
            }
            _ if class.is_xmm() && self.xmm < ARG_XMMS => {
                self.xmm += 1;
                return Loc::Reg(xmm(self.xmm - 1));
            }
            _ => {}
        }
        let slot = class.slot();
        Loc::Stack(self.stack(slot, slot))
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
        && kind == PartKind::Value
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
/// has. `inreg` is ignored (§6.5).
fn check_flags(types: &TypeTable, ty: TypeId, attrs: ParamAttrs) -> Abi<()> {
    if attrs.is_none() {
        return Ok(());
    }
    if attrs.has(ParamAttrs::NEST) {
        return no(NEST);
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

fn is_aggregate(types: &TypeTable, ty: TypeId) -> bool {
    matches!(types.kind(ty), TypeKind::Struct { .. } | TypeKind::Array { .. })
}

/// Refuses a type with a leaf §4 does not pass.
fn check_type(types: &TypeTable, ty: TypeId, features: Features) -> Abi<()> {
    walk::each_leaf_type(types, ty, &mut |leaf| classify(types, leaf, features).map(|_| ()))
}

/// The return value does not fit the return registers.
struct Memory;

/// The return registers of §5.1 as they are taken.
#[derive(Default)]
struct RetState {
    /// How many of `xmm0`.. a `float` or `double` may take: 2, or 3
    /// under `fastcc` (§5.1).
    float_xmm: usize,
    gpr: usize,
    /// `xmm0`..`xmm3`, taken.
    xmm: [bool; 4],
    x87: u8,
    /// A `float` or `double` went to the x87 stack.
    float_in_x87: bool,
}

impl RetState {
    /// The first of `xmm0`..`xmm<count - 1>` not taken.
    fn xmm(&mut self, count: usize) -> Option<PReg> {
        let free = self.xmm[..count].iter().position(|taken| !taken)?;
        self.xmm[free] = true;
        Some(xmm(free as u8))
    }

    fn x87(&mut self) -> Option<PReg> {
        (self.x87 < 2).then(|| {
            self.x87 += 1;
            st(self.x87 - 1)
        })
    }

    fn reg(&mut self, class: Class) -> Option<PReg> {
        match class {
            Class::Gpr => {
                let reg = RET_GPRS.get(self.gpr).copied()?;
                self.gpr += 1;
                Some(reg)
            }
            Class::Half => self.xmm(3),
            Class::Float => self.xmm(self.float_xmm).or_else(|| {
                self.float_in_x87 = true;
                self.x87()
            }),
            Class::Quad => self.xmm(2),
            Class::Vector => self.xmm(4),
            Class::X87 => self.x87(),
        }
    }
}

/// §5: the return value. `Ok(true)` when it travels in memory.
fn assign_return(
    types: &TypeTable,
    sig: &Sig<'_>,
    features: Features,
    parts: &mut Vec<Part>,
) -> Abi<bool> {
    let attrs = sig.ret_attrs;
    check_flags(types, sig.ret, attrs)?;
    if attrs.mem() != ParamMem::None {
        return no(INVALID_SIGNATURE);
    }
    if sig.ret == TypeId::VOID {
        return Ok(false);
    }

    let float_xmm = if sig.cc == CallConv::FAST { 3 } else { 2 };
    let mut regs = RetState { float_xmm, ..RetState::default() };
    let mut place = |leaf: Leaf, offset: u64, parts: &mut Vec<Part>| -> Result<(), Memory> {
        for piece in 0..leaf.count {
            let reg = regs.reg(leaf.class).ok_or(Memory)?;
            let at = offset.saturating_add(u64::from(piece) * u64::from(leaf.stride));
            push(parts, 0, leaf.kind(), at, leaf.piece_size(piece), Loc::Reg(reg))
                .map_err(|_| Memory)?;
        }
        Ok(())
    };
    let fits = if is_aggregate(types, sig.ret) {
        check_type(types, sig.ret, features)?;
        walk::leaves(types, sig.ret, 0, &mut |leaf_ty, offset| {
            let leaf = classify(types, leaf_ty, features).map_err(|_| Memory)?;
            place(leaf, offset, parts)
        })
    } else {
        place(classify(types, sig.ret, features)?, 0, parts)
    };
    if fits.is_err() {
        // §5.2.
        parts.clear();
        return Ok(true);
    }
    if regs.float_in_x87 {
        parts.clear();
        return no(RET_X87_PART);
    }
    Ok(false)
}

/// §6: one argument.
#[allow(clippy::too_many_arguments)]
fn assign_argument(
    types: &TypeTable,
    ty: TypeId,
    attrs: ParamAttrs,
    arg: u32,
    features: Features,
    state: &mut State,
    parts: &mut Vec<Part>,
    budget: &mut u32,
) -> Abi<()> {
    if attrs.is_none() {
        return assign_value(types, ty, arg, features, state, parts, budget);
    }
    check_flags(types, ty, attrs)?;
    match attrs.mem() {
        ParamMem::None => {}
        // §6.4: an ordinary pointer.
        ParamMem::ByRef if types.is_ptr(ty) => {}
        ParamMem::ByRef => return no(INVALID_SIGNATURE),
        ParamMem::InAlloca => return no(INALLOCA),
        ParamMem::Preallocated => return no(PREALLOCATED),
        ParamMem::ElementType => return no(ELEMENTTYPE),
        ParamMem::Sret => {
            // §6.1: LLVM's verifier wants one `sret` at most, on the first
            // or second parameter, and a `void` return with it; it is an
            // ordinary pointer otherwise.
            if !is_plain_pointer(types, ty) || arg > 1 || state.returns_value {
                return no(INVALID_SIGNATURE);
            }
        }
        ParamMem::ByVal => {
            // §6.2.
            let pointee = attrs.mem_type().unwrap_or(TypeId::VOID);
            if !is_plain_pointer(types, ty) || !types.is_sized(pointee) {
                return no(INVALID_SIGNATURE);
            }
            if types.is_scalable(pointee) {
                return no(SCALABLE_VECTOR);
            }
            let align = attrs
                .stack_align()
                .or(attrs.align())
                .unwrap_or_else(|| types.align_of(pointee).max(8))
                .max(8);
            if align > 16 {
                return no(BYVAL_ALIGN);
            }
            let size = types.alloc_size_of(pointee);
            let Ok(part_size) = u32::try_from(size) else { return no(SIZE_LIMIT) };
            let at = state.stack(size.max(8), align);
            charge(budget, 1)?;
            return push(parts, arg, PartKind::Pointee, 0, part_size, Loc::Stack(at));
        }
    }
    assign_value(types, ty, arg, features, state, parts, budget)
}

/// §3: an argument passed by value.
fn assign_value(
    types: &TypeTable,
    ty: TypeId,
    arg: u32,
    features: Features,
    state: &mut State,
    parts: &mut Vec<Part>,
    budget: &mut u32,
) -> Abi<()> {
    if !is_aggregate(types, ty) {
        let leaf = classify(types, ty, features)?;
        charge(budget, leaf.count)?;
        if leaf.pair {
            // §4.2: two registers, or 16 bytes of stack aligned to 16; the
            // registers left stay for the next arguments.
            if usize::from(state.gpr) + 2 <= ARG_GPRS.len() {
                for (piece, reg) in [0, 1].into_iter().zip(&ARG_GPRS[state.gpr as usize..]) {
                    push(parts, arg, PartKind::Value, 8 * piece, 8, Loc::Reg(*reg))?;
                }
                state.gpr += 2;
            } else {
                let at = state.stack(16, 16);
                push(parts, arg, PartKind::Value, 0, 16, Loc::Stack(at))?;
            }
            return Ok(());
        }
        for piece in 0..leaf.count {
            let loc = state.piece(leaf.class);
            let at = u64::from(piece) * u64::from(leaf.stride);
            push(parts, arg, leaf.kind(), at, leaf.piece_size(piece), loc)?;
        }
        return Ok(());
    }
    // §4.4: an aggregate leaf by leaf, each piece on its own.
    check_type(types, ty, features)?;
    walk::leaves(types, ty, 0, &mut |leaf_ty, offset| {
        let leaf = classify(types, leaf_ty, features)?;
        charge(budget, leaf.count)?;
        for piece in 0..leaf.count {
            let loc = state.piece(leaf.class);
            let at = offset.saturating_add(u64::from(piece) * u64::from(leaf.stride));
            push(parts, arg, leaf.kind(), at, leaf.piece_size(piece), loc)?;
        }
        Ok(())
    })
}
