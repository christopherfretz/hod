//! Argument assignment for `x86_64-unknown-linux-gnu`: `spec/abi-x64.md`.
//!
//! [`assign`] answers where each byte of each argument and of the return
//! value of a signature travels, exactly as `llc` 22 lowers the same
//! IR-level signature, so that hod-compiled and LLVM-compiled code can
//! call each other. The section numbers in the comments are the spec's.
//!
//! The work is split as `hod_cg`'s shared walk splits it
//! (`spec/codegen.md` §7.2, "The leaf walk"): [`classify`] is the
//! per-target hook ([`Leaves`], `hod_cg::abi::Classify`) that turns one
//! leaf type into pieces of a register class, `hod_cg::abi::place_leaves`
//! visits the leaves of an aggregate, and placing the pieces (registers,
//! stack slots, the return registers) is this file's. The registers are
//! `crate::reg`'s.

use hod_cg::PReg;
use hod_cg::abi::{
    AbiUnsupported, Assignment, Classify, LaneLeaf, Part, PartKind, PieceLoc, Place, RetMemory,
    Sig, VaInfo, charge, check_type, place_leaf, place_leaves, push, walk,
};
use hod_ir::{CallConv, ParamAttrs, ParamMem, TypeId, TypeKind, TypeTable};

pub use crate::reg::is_x87;
use crate::reg::{R8, R9, R10, RAX, RCX, RDI, RDX, RSI, ST0, st, xmm};

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

/// The first x87 return register (§5.1): an `x86_fp80`, or a `float` or
/// `double` piece beyond the vector return registers.
pub const X87_RET: PReg = ST0;

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

/// Every name [`assign`] reports, in the order of §10. [`RET_X87_PART`]
/// is not one: `assign` places such a part in `st0`/`st1`, and
/// `X64::assign` refuses it until WP-131 lowers it.
pub const UNSUPPORTED: [&str; 19] = [
    CC,
    INVALID_SIGNATURE,
    SCALABLE_VECTOR,
    TARGET_EXT_TYPE,
    ADDRSPACE,
    VECTOR_ELEMENT,
    MASK_VECTOR,
    WIDE_VECTOR_ABI,
    NEST,
    SWIFTSELF,
    SWIFTERROR,
    SWIFTASYNC,
    INALLOCA,
    PREALLOCATED,
    ELEMENTTYPE,
    BYVAL_ALIGN,
    I128_VECTOR_SPLIT,
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
/// is not an `x86_fp80` (§5.1): `assign` places it ([`x87_float_part`]),
/// `X64::assign` refuses it under this name until WP-131.
pub const RET_X87_PART: &str = "x64:ret-x87-part";
/// A `nest` argument (§6.3).
pub const NEST: &str = "x64:nest";
/// An assignment that is right but that no lowering honours yet: the
/// groups that consume it remove each use as they land (`call::guard`
/// for WP-125 and WP-131, `term::ret` for WP-126).
pub const ABI: &str = "x64:todo:abi";
const SWIFTSELF: &str = "abi:swiftself";
const SWIFTERROR: &str = "abi:swifterror";
const SWIFTASYNC: &str = "abi:swiftasync";
const INALLOCA: &str = "abi:inalloca";
const PREALLOCATED: &str = "abi:preallocated";
const ELEMENTTYPE: &str = "abi:elementtype";
const BYVAL_ALIGN: &str = "abi:byval-align";
/// A vector of `i128` argument whose first element would lie partly in
/// registers and partly on the stack, or start on the stack at an offset
/// that is not a multiple of 16 (§4.3): `llc` 19 keeps the first element
/// a pair, `llc` 22 splits it (WP-152, WP-171).
pub const I128_VECTOR_SPLIT: &str = "x64:vector-i128-split";
/// The largest `byval` alignment (§6.2): the caller aligns `rsp` with an
/// `and` of a 32-bit immediate.
pub const MAX_BYVAL_ALIGN: u64 = 1 << 30;
const TOO_MANY_PIECES: &str = hod_cg::abi::TOO_MANY_PIECES;
const SIZE_LIMIT: &str = hod_cg::abi::SIZE_LIMIT;

type Abi<T> = Result<T, AbiUnsupported>;

fn no<T>(name: &'static str) -> Abi<T> {
    Err(AbiUnsupported(name))
}

/// Whether `cc` passes arguments, returns values and preserves registers
/// exactly as `ccc` does on this target (§1.2).
pub fn cc_is_c(cc: CallConv) -> bool {
    cc == CallConv::C || cc == CallConv::FAST || cc == CallConv::X86_64_SYSV
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

/// How one leaf type travels (§4): `hod_cg`'s leaf of this target's
/// [`Class`]. An `i128` has `split` 2 (a pair never split, §4.2); a
/// vector of `i1` has `lanes` (§4.3).
pub type Leaf = hod_cg::abi::Leaf<Class>;

/// The classification hook of the shared walk (`hod_cg::abi::Classify`)
/// for a function compiled with these features: [`classify`].
#[derive(Clone, Copy, Debug)]
pub struct Leaves(pub Features);

impl Classify for Leaves {
    type Class = Class;

    fn leaf(&self, types: &TypeTable, ty: TypeId) -> Abi<Leaf> {
        classify(types, ty, self.0)
    }
}

/// §4: the pieces of a leaf type (not a structure or an array), for a
/// function compiled with `features`. This is the classification hook
/// the shared leaf walk calls ([`Leaves`]).
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
                    last_size: bytes - 8 * (count - 1),
                    split: if bits == 128 { 2 } else { 0 },
                    ..Leaf::each(Class::Gpr, count, 8)
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
        // WP-152: element by element, each an `x86_fp80` (a 16-byte slot
        // as an argument, `st0`/`st1` as a result); the elements are 10
        // bytes apart in the vector.
        TypeKind::X86Fp80 => return Ok(Leaf::each(Class::X87, len, 10)),
        // WP-152: element by element as LLVM 22 scalarizes it: each
        // `i128` two ordinary general pieces (no pair rule, §4.3); where
        // `llc` 19 places the first element otherwise, [`assign_argument`]
        // refuses (`I128_VECTOR_SPLIT`).
        TypeKind::Int(128) => {
            let Some(count) = len.checked_mul(2) else { return no(SIZE_LIMIT) };
            return Ok(Leaf::each(Class::Gpr, count, 8));
        }
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
            let lanes = LaneLeaf { bits: 1, lane: (16 / len) as u8, len: len as u8, each: false };
            Leaf { lanes: Some(lanes), ..Leaf::one(Class::Vector, 16) }
        }
        _ => return no(MASK_VECTOR),
    })
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
    /// The stack offset of the `i128` being placed on the stack whole
    /// (§4.2): its second piece follows its first.
    pair_at: Option<u64>,
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

    /// §3.2, §4.2: piece `piece` of `leaf`. `whole`: the leaf is the
    /// argument itself, not a member of an aggregate, so an `i128` is a
    /// pair: two general registers, or 16 bytes of stack aligned to 16
    /// (the registers left stay for the next arguments). A member of an
    /// aggregate is placed piece by piece (§4.4).
    fn piece(&mut self, leaf: &Leaf, piece: u32, whole: bool) -> PieceLoc {
        if whole && leaf.split != 0 {
            if leaf.split_at(piece) {
                self.pair_at = (usize::from(self.gpr) + 2 > ARG_GPRS.len())
                    .then(|| self.stack(u64::from(leaf.split) * 8, 16));
            }
            if let Some(at) = self.pair_at {
                let index = u64::from(piece % leaf.split);
                return PieceLoc::Stack(at + 8 * index);
            }
        }
        let class = leaf.class;
        match class {
            Class::Gpr if usize::from(self.gpr) < ARG_GPRS.len() => {
                self.gpr += 1;
                return PieceLoc::Reg(ARG_GPRS[usize::from(self.gpr) - 1]);
            }
            _ if class.is_xmm() && self.xmm < ARG_XMMS => {
                self.xmm += 1;
                return PieceLoc::Reg(xmm_reg(self.xmm - 1));
            }
            _ => {}
        }
        let slot = class.slot();
        PieceLoc::Stack(self.stack(slot, slot))
    }
}

/// `xmm<n>`, `n` < 16.
fn xmm_reg(n: u8) -> PReg {
    let Some(reg) = hod_mc_x64::Xmm::new(n) else { unreachable!("xmm{n} is no register") };
    xmm(reg)
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

/// The return value does not fit the return registers (§5.2).
struct Memory;

impl From<AbiUnsupported> for Memory {
    fn from(_: AbiUnsupported) -> Memory {
        Memory
    }
}

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
}

impl RetState {
    /// The first of `xmm0`..`xmm<count - 1>` not taken.
    fn xmm(&mut self, count: usize) -> Option<PReg> {
        let free = self.xmm[..count].iter().position(|taken| !taken)?;
        self.xmm[free] = true;
        Some(xmm_reg(free as u8))
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
            // Beyond the vector registers, the x87 stack (§5.1).
            Class::Float => self.xmm(self.float_xmm).or_else(|| self.x87()),
            Class::Quad => self.xmm(2),
            Class::Vector => self.xmm(4),
            Class::X87 => self.x87(),
        }
    }
}

/// §5.1: whether `part` of a return value is a `float` or `double` piece
/// in an x87 register (converted to the x87 format there), which
/// `X64::assign` refuses as [`RET_X87_PART`] until WP-131.
pub fn x87_float_part(part: &Part) -> bool {
    matches!(part.place, Place::Reg(reg) if is_x87(reg)) && part.size != 10
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

    let leaves = Leaves(features);
    // A leaf §4 does not pass is refused, not demoted.
    check_type(types, sig.ret, &leaves)?;
    let float_xmm = if sig.cc == CallConv::FAST { 3 } else { 2 };
    let mut regs = RetState { float_xmm, ..RetState::default() };
    let mut place = |leaf: &Leaf, _: u32| regs.reg(leaf.class).map(PieceLoc::Reg).ok_or(Memory);
    let fits = walk(types, sig.ret, 0, &mut |leaf_ty, offset| {
        let leaf = leaves.leaf(types, leaf_ty)?;
        place_leaf(&leaf, 0, offset, parts, &mut place)
    });
    if fits.is_err() {
        // §5.2.
        parts.clear();
        return Ok(true);
    }
    if is_lone_gpr_vector(types, sig.ret) {
        mark_lone(parts);
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
    leaves: Leaves,
    state: &mut State,
    out: &mut Vec<Part>,
    budget: &mut u32,
) -> Abi<()> {
    if !attrs.is_none() {
        check_flags(types, ty, attrs)?;
    }
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
            let align = byval_align(types, pointee, attrs);
            // WP-152: above 16 the caller aligns the stack-argument area
            // (`call::align_stack_area`).
            if align > MAX_BYVAL_ALIGN || !align.is_power_of_two() {
                return no(BYVAL_ALIGN);
            }
            let size = types.alloc_size_of(pointee);
            let Ok(part_size) = u32::try_from(size) else { return no(SIZE_LIMIT) };
            let at = state.stack(size.max(8), align);
            charge(budget, 1)?;
            return push(out, arg, PartKind::Pointee, 0, part_size, PieceLoc::Stack(at));
        }
    }
    // §3, §4.4: by value, an aggregate leaf by leaf.
    let whole = !is_aggregate(types, ty);
    if !whole {
        check_type(types, ty, &leaves)?;
    }
    let first = out.len();
    place_leaves(types, ty, &leaves, arg, budget, out, &mut |leaf, piece| {
        Ok(state.piece(leaf, piece, whole))
    })?;
    if is_i128_vector(types, ty) {
        check_i128_vector(&out[first..])?;
    }
    if whole && is_lone_gpr_vector(types, ty) {
        mark_lone(&mut out[first..]);
    }
    Ok(())
}

/// WP-174: the kind of the one part of a lone `<1 x i64>` or `<1 x ptr>`
/// argument or result (§4.3): the value is a vector register (class
/// `Fpr`), the place a general register or an 8-byte stack slot, so the
/// part is a lane move (`vcall.rs`, a `movq`), never a plain `Value`
/// part, which cannot change register class.
pub const LONE_LANE: PartKind = PartKind::Lanes { bits: 64, lane: 8, first: 0, len: 1 };

/// Whether `ty` is a vector of one 64-bit integer or pointer: LLVM
/// scalarizes it into a general piece, while hod keeps its value in an
/// `xmm` (`target.rs` `vector_class`).
pub fn is_lone_gpr_vector(types: &TypeTable, ty: TypeId) -> bool {
    matches!(types.kind(ty), TypeKind::Vector { elem, len: 1 }
        if matches!(types.kind(elem), TypeKind::Int(64) | TypeKind::Ptr { addr_space: 0 }))
}

/// The parts of a lone `<1 x i64>`/`<1 x ptr>` (one, 8 bytes) made lane
/// moves ([`LONE_LANE`]).
fn mark_lone(parts: &mut [Part]) {
    for part in parts {
        part.kind = LONE_LANE;
    }
}

/// §6.2: the alignment of the copy of a `byval(pointee)` argument with
/// the attributes `attrs`: its `alignstack`, else its `align`, else the
/// ABI alignment of `pointee`, and at least 8.
pub fn byval_align(types: &TypeTable, pointee: TypeId, attrs: ParamAttrs) -> u64 {
    attrs.stack_align().or(attrs.align()).unwrap_or_else(|| types.align_of(pointee)).max(8)
}

/// Whether `ty` is a vector of `i128`.
fn is_i128_vector(types: &TypeTable, ty: TypeId) -> bool {
    matches!(types.kind(ty), TypeKind::Vector { elem, .. }
        if matches!(types.kind(elem), TypeKind::Int(128)))
}

/// §4.3 (WP-152, WP-171): the first element of a vector of `i128`
/// argument is in two registers, or on the stack from an offset that is a
/// multiple of 16; the pieces after it are ordinary. `llc` 19 places the
/// first element as a lone `i128` (both halves in registers, else both on
/// the stack aligned to 16) and the rest as `llc` 22 does, so anything
/// else is placed differently by the two.
fn check_i128_vector(parts: &[Part]) -> Abi<()> {
    match (parts.first().map(|p| p.place), parts.get(1).map(|p| p.place)) {
        (Some(Place::Reg(_)), Some(Place::Reg(_))) => Ok(()),
        (Some(Place::Stack(at)), _) if at % 16 == 0 => Ok(()),
        _ => no(I128_VECTOR_SPLIT),
    }
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
    // WP-181 (§1.2): `preserve_mostcc` and `preserve_allcc` pass and
    // return as `ccc`; only the callee's saved set differs (`frame.rs`).
    let as_c;
    let sig = if matches!(sig.cc, CallConv::PRESERVE_MOST | CallConv::PRESERVE_ALL) {
        as_c = Sig { cc: CallConv::C, ..*sig };
        &as_c
    } else {
        sig
    };
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

    let leaves = Leaves(features);
    let mut budget = MAX_PIECES;
    for (index, &ty) in sig.params.iter().enumerate() {
        if index == fixed {
            out.va = state.va_info();
        }
        let attrs = sig.attrs(index);
        if attrs.mem() == ParamMem::Sret {
            out.sret_returned_in = Some(RETURNS_ADDRESS);
        }
        let arg = index as u32;
        assign_argument(types, ty, attrs, arg, leaves, &mut state, &mut out.args, &mut budget)?;
    }
    if sig.variadic && fixed == sig.params.len() {
        out.va = state.va_info();
    }
    out.vector_regs_used = state.xmm;
    out.stack_size = u32::try_from(state.stack).or(no(SIZE_LIMIT))?;
    Ok(())
}
