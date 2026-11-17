//! The i386 System V calling convention as LLVM implements it
//! (`spec/abi-x86.md`, the normative text): [`assign`] answers, for an
//! IR signature, where each byte of each argument and of the result
//! travels. Owned by design 0008's I6; I7, I9, I11 and I12 extend the
//! parts of their types.
//!
//! In short (`spec/abi-x86.md` §3, §5): every argument goes on the stack,
//! in order, each leaf of it in a slot that starts at a multiple of 4
//! (a `byval` copy at its alignment); `inreg` integers take `eax`,
//! `edx`, `ecx` (`fastcc` integers `ecx`, `edx`), a 64-bit one a word
//! at a time; `nest` is `ecx` (`eax` under `fastcc`). A result of up to
//! three 32-bit integer words returns in `eax`, `edx`, `ecx`, one
//! floating-point leaf in `st0` ([`ST0`]); a larger one, `i128` and
//! `fp128`, through a hidden pointer that the callee pops, as it pops a
//! stack `sret` pointer ([`callee_pop`]).

use hod_cg::abi::{AbiUnsupported, Assignment, Part, PartKind, Place, RetMemory, Sig};
use hod_cg::{CgResult, FnCx, FrameRef, PReg};
use hod_ir::{
    CallConv, CallSite, ParamAttrs, ParamMem, TailKind, TypeId, TypeKind, TypeTable, ValueKind,
    X86Features,
};
use hod_mc_x64 as mc;
use hod_mc_x64::X87Size;

use crate::common;
use crate::reg::{EAX, ECX, EDX};
use crate::target::X86;

/// A calling convention other than C, `coldcc` (C's argument passing on
/// i386) and `fastcc` without floating point (`spec/abi-x86.md` §1.2).
pub const CC: &str = "x86:abi:cc";
/// An argument or result attribute hod does not pass (§6.6): `inalloca`,
/// `preallocated`, `alignstack`, the Swift ones, `sret` that is not the
/// first parameter, `inreg` of a floating-point value or in a variadic
/// signature, a `byval` alignment above 16, `nest` in a register an
/// `inreg` argument took, an attribute of the result.
pub const ATTR: &str = "x86:abi:attr";
/// An `i128` or `fp128` argument: LLVM 22 aligns its slot to 16, LLVM 19
/// to 4 (§12).
pub const WIDE_ARG: &str = "x86:abi:wide-arg";
/// An IR aggregate result with two floating-point leaves (`st0`, `st1`):
/// the call sequence moves the parts of a result in one order on both
/// sides, and the x87 stack needs opposite orders.
pub const X87_PAIR: &str = "x86:abi:x87-pair";
/// An indirect call whose arguments use `edx`, the register the callee's
/// address is put in.
pub const INDIRECT_EDX: &str = "x86:abi:indirect-edx";

/// `st0` as the place of a floating-point result part: glue `load` is
/// `fld`, glue `store` `fstp` (the part's size picks the precision:
/// 4, 8, 10). No value lives in it. Its index follows the `xmm`
/// registers (`reg.rs`).
pub const ST0: PReg = PReg::new(16);

/// The `inreg` registers of the C convention, in order.
const INREG: [PReg; 3] = [EAX, EDX, ECX];
/// The x87 width of a part of `bytes` bytes.
const fn x87_size(bytes: u32) -> X87Size {
    match bytes {
        4 => X87Size::S,
        8 => X87Size::D,
        _ => X87Size::T,
    }
}

/// Glue `load` into [`ST0`]: `fld` of the `bytes` (4, 8, 10) at `src`,
/// the one value the x87 stack holds at a return.
pub fn load_st0(cx: &mut FnCx<'_, X86>, src: FrameRef, bytes: u32) {
    common::frame_inst(cx, src, |m| mc::fld_m(x87_size(bytes), m));
}

/// Glue `store` from [`ST0`]: `fstp` to `dst`, which also pops the
/// result a caller does not use (`spec/abi-x86.md` §5.1).
pub fn store_st0(cx: &mut FnCx<'_, X86>, dst: FrameRef, bytes: u32) {
    common::frame_inst(cx, dst, |m| mc::fstp_m(x87_size(bytes), m));
}

/// The integer argument registers of `fastcc`.
const FASTCC: [PReg; 2] = [ECX, EDX];
/// The integer result registers.
const RET: [PReg; 3] = [EAX, EDX, ECX];
/// The most leaves an IR aggregate argument or result may have.
const MAX_LEAVES: usize = 16;

/// The ABI's checks of a call before the call sequence runs
/// (`call::check`, `spec/abi-x86.md` §8.1, §8.5):
///
/// - An indirect call: the callee's address travels in `edx`
///   (`RegInfo::callee`), so an argument that the signature puts in
///   `edx` is refused as [`INDIRECT_EDX`].
/// - A `musttail` call with stack arguments: the call sequence copies
///   them over the incoming ones after the register arguments are
///   placed, through `ecx`, so an argument in `ecx` is refused as
///   `x86:abi:musttail`.
///
/// Only a call with `fastcc`, an `inreg` or a `nest` parameter puts
/// anything in a register; the others are not assigned twice. A
/// signature `assign` refuses is left to the call sequence, which refuses
/// it by its own name.
///
/// # Errors
/// [`INDIRECT_EDX`], `x86:abi:musttail`.
pub fn check_call(cx: &FnCx<'_, X86>, site: &CallSite<'_>) -> CgResult<()> {
    let musttail = site.tail() == TailKind::MustTail;
    let indirect = !matches!(site.callee_value().kind(), ValueKind::Sym(_));
    if !musttail && !indirect {
        return Ok(());
    }
    let types = cx.types();
    let TypeKind::Function { ret, params, variadic } = types.kind(site.fn_type()) else {
        return Ok(());
    };
    if variadic {
        // §6.5: a variadic call passes nothing in registers.
        return Ok(());
    }
    let mut attrs = Vec::new();
    for index in 0..params.len() {
        let Ok(a) = hod_ir::call_arg_attrs(cx.module, site, index) else { return Ok(()) };
        attrs.push(a);
    }
    let in_reg = attrs.iter().any(|a| a.has(ParamAttrs::INREG) || a.has(ParamAttrs::NEST));
    if site.cc() != CallConv::FAST && !in_reg {
        return Ok(());
    }
    let Ok(ret_attrs) = hod_ir::call_ret_attrs(cx.module, site) else { return Ok(()) };
    let sig = Sig {
        cc: site.cc(),
        ret,
        ret_attrs,
        params,
        param_attrs: &attrs,
        fixed: params.len() as u32,
        variadic,
    };
    let mut asg = Assignment::default();
    if assign_for(types, &sig, &mut asg, cx.target.features()).is_err() {
        return Ok(());
    }
    let uses = |reg| asg.args.iter().any(|p| p.place == Place::Reg(reg));
    if indirect && uses(EDX) {
        return common::unsupported(INDIRECT_EDX);
    }
    if musttail && asg.stack_size != 0 && uses(ECX) {
        return common::unsupported(crate::frame::MUSTTAIL);
    }
    Ok(())
}

/// The bytes a callee of `asg` pops with `ret $n`: 4 for a hidden result
/// pointer or an `sret` pointer on the stack (`spec/abi-x86.md` §5.2),
/// else 0.
pub fn callee_pop(asg: &Assignment) -> u32 {
    let hidden = matches!(asg.ret_memory, Some(RetMemory { address: Place::Stack(0), .. }));
    let sret = asg.sret_returned_in.is_some()
        && asg.args.first().is_some_and(|p| p.arg == 0 && p.place == Place::Stack(0));
    if hidden || sret { 4 } else { 0 }
}

/// The refusal of a value of type `ty` that is no `Gpr` value: by the
/// family that owns it (an integer wider than 64 bits:
/// [`crate::wideint::boundary_refusal`]).
pub fn type_refusal(types: &TypeTable, ty: TypeId) -> &'static str {
    match types.kind(ty) {
        TypeKind::Int(bits) if bits > 64 => crate::wideint::boundary_refusal(bits),
        TypeKind::Int(_) => crate::int::I64,
        TypeKind::Half
        | TypeKind::BFloat
        | TypeKind::Float
        | TypeKind::Double
        | TypeKind::Fp128
        | TypeKind::X86Fp80 => crate::x87::FLOAT,
        TypeKind::Vector { .. } | TypeKind::ScalableVector { .. } => crate::target::VECTOR,
        _ => crate::target::AGGREGATE,
    }
}

/// Whether `ty` is a `Gpr` value: `i1`..`i32`, `ptr`.
pub fn is_gpr(types: &TypeTable, ty: TypeId) -> bool {
    match types.kind(ty) {
        TypeKind::Int(bits) => bits <= 32,
        TypeKind::Ptr { .. } => true,
        _ => false,
    }
}

/// Whether `attrs` has an attribute hod does not pass at all.
fn odd(attrs: ParamAttrs) -> bool {
    attrs.has(ParamAttrs::SWIFT_SELF)
        || attrs.has(ParamAttrs::SWIFT_ERROR)
        || attrs.has(ParamAttrs::SWIFT_ASYNC)
        || matches!(attrs.mem(), ParamMem::InAlloca | ParamMem::Preallocated)
        || attrs.stack_align().is_some()
}

/// A vector at the boundary without SSE (WP-338): the number of its
/// leaves, the distance between them in its image, and the leaf (an
/// `i64` lane is two words). `<N x i1>`: [`crate::vecscalar::ABI`].
fn vector_lanes(types: &TypeTable, ty: TypeId) -> Result<(u32, u32, Leaf), &'static str> {
    let TypeKind::Vector { elem, len } = types.kind(ty) else {
        return Err(crate::vector::SCALABLE);
    };
    let (parts, leaf) = match types.kind(elem) {
        TypeKind::Int(1) => return Err(crate::vecscalar::ABI),
        TypeKind::Int(bits @ (8 | 16 | 32)) => (1, Leaf::Int(bits / 8)),
        TypeKind::Int(64) => (2, Leaf::Int(4)),
        TypeKind::Ptr { addr_space: 0 } => (1, Leaf::Int(4)),
        TypeKind::Float => (1, Leaf::Fp { size: 4, slot: 4 }),
        TypeKind::Double => (1, Leaf::Fp { size: 8, slot: 8 }),
        _ => return Err(crate::vector::ELEMENT),
    };
    if len > crate::vector::MAX_LANES {
        return Err(crate::vector::LENGTH);
    }
    let (Leaf::Int(stride) | Leaf::Fp { size: stride, .. }) = leaf;
    Ok((len * parts, stride, leaf))
}

/// One leaf of a value at the boundary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Leaf {
    /// An integer or pointer of `size` (1 to 4) bytes: a 32-bit word.
    Int(u32),
    /// A floating-point value of `size` bytes (4, 8, 10) in a slot of
    /// `slot` bytes (4, 8, 12).
    Fp { size: u32, slot: u32 },
}

/// The leaves of a value, offset first.
struct Leaves {
    at: [(u32, Leaf); MAX_LEAVES],
    len: usize,
    /// Whether the value has more than [`MAX_LEAVES`] leaves (then only
    /// the first ones are kept).
    full: bool,
}

impl Leaves {
    fn new() -> Leaves {
        Leaves { at: [(0, Leaf::Int(0)); MAX_LEAVES], len: 0, full: false }
    }

    fn push(&mut self, offset: u64, leaf: Leaf) -> Result<(), &'static str> {
        let Ok(offset) = u32::try_from(offset) else { return Err(hod_cg::abi::SIZE_LIMIT) };
        match self.at.get_mut(self.len) {
            Some(slot) => {
                *slot = (offset, leaf);
                self.len += 1;
            }
            None => self.full = true,
        }
        Ok(())
    }

    fn iter(&self) -> impl Iterator<Item = (u32, Leaf)> + '_ {
        self.at[..self.len].iter().copied()
    }

    /// The leaves of `ty` at byte `base` of the value: an `i64` is two
    /// words; `i128`/`fp128` and other types are refused by name.
    fn walk(&mut self, types: &TypeTable, ty: TypeId, base: u64) -> Result<(), &'static str> {
        match types.kind(ty) {
            TypeKind::Int(bits) if bits <= 32 => self.push(base, Leaf::Int(bits.div_ceil(8))),
            // An integer of 33 to 64 bits is two words (`llc` promotes
            // it to `i64`, the bits above its width unspecified); the
            // high one moves the store size's bytes (WP-315).
            TypeKind::Int(bits @ 33..=64) => {
                self.push(base, Leaf::Int(4))?;
                self.push(base + 4, Leaf::Int((bits - 32).div_ceil(8)))
            }
            TypeKind::Ptr { .. } => self.push(base, Leaf::Int(4)),
            TypeKind::Float => self.push(base, Leaf::Fp { size: 4, slot: 4 }),
            TypeKind::Double => self.push(base, Leaf::Fp { size: 8, slot: 8 }),
            TypeKind::X86Fp80 => self.push(base, Leaf::Fp { size: 10, slot: 12 }),
            TypeKind::Array { elem, len } => {
                let stride = types.alloc_size_of(elem);
                for index in 0..len {
                    if self.full {
                        return Ok(());
                    }
                    self.walk(types, elem, base + index * stride)?;
                }
                Ok(())
            }
            TypeKind::Struct { fields, .. } => {
                let Some(offsets) = types.field_offsets(ty) else {
                    return Err(crate::target::AGGREGATE);
                };
                for (&field, &offset) in fields.iter().zip(offsets) {
                    self.walk(types, field, base + offset)?;
                }
                Ok(())
            }
            _ => Err(type_refusal(types, ty)),
        }
    }
}

/// The convention family of `cc`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conv {
    C,
    Fast,
}

/// The argument registers and the next one free.
struct Regs {
    list: &'static [PReg],
    next: usize,
    taken: u8,
}

impl Regs {
    fn take(&mut self) -> Option<PReg> {
        let reg = *self.list.get(self.next)?;
        self.next += 1;
        self.taken |= 1 << reg.index();
        Some(reg)
    }

    fn holds(&self, reg: PReg) -> bool {
        self.taken & (1 << reg.index()) != 0
    }
}

/// Appends a part, merged into the argument's previous stack part when
/// both lie at the same distance in the value and in the area (a `{i32,
/// i32}` or an `i64` on the stack is one part, which lets the callee use
/// the incoming slot as the value's home).
fn push_part(out: &mut Assignment, part: Part) {
    if let (Some(last), Place::Stack(at)) = (out.args.last_mut(), part.place)
        && last.arg == part.arg
        && last.kind == PartKind::Value
        && part.kind == PartKind::Value
        && let Place::Stack(last_at) = last.place
        && part.offset >= last.offset + last.size
        && at - last_at == part.offset - last.offset
    {
        last.size = part.offset + part.size - last.offset;
        return;
    }
    out.args.push(part);
}

/// `Target::assign` (`spec/abi-x86.md` §3 to §6).
///
/// # Errors
/// [`CC`], [`ATTR`], [`WIDE_ARG`], [`X87_PAIR`], and the type refusal of
/// a value hod does not pass (`x86:vector`, `x86:float` for `half`,
/// `x86:i64` for an integer wider than 32 bits other than `i64`,
/// `x86:aggregate`).
pub fn assign(
    types: &TypeTable,
    sig: &Sig<'_>,
    out: &mut Assignment,
) -> Result<(), AbiUnsupported> {
    assign_for(types, sig, out, X86Features::NONE)
}

/// [`assign`] in a function of the feature set `features` (which places
/// a `half`: `sse::half_place`).
///
/// # Errors
/// As [`assign`].
pub fn assign_for(
    types: &TypeTable,
    sig: &Sig<'_>,
    out: &mut Assignment,
    features: X86Features,
) -> Result<(), AbiUnsupported> {
    out.clear();
    match assign_into(types, sig, out, features) {
        Ok(()) => {
            out.callee_pop = callee_pop(out);
            Ok(())
        }
        Err(name) => {
            out.clear();
            Err(AbiUnsupported(name))
        }
    }
}

/// The body of [`assign`]: the arguments in order (§3), after the
/// result (§5).
#[allow(clippy::too_many_lines, reason = "one walk over the parameters, rule by rule")]
fn assign_into(
    types: &TypeTable,
    sig: &Sig<'_>,
    out: &mut Assignment,
    features: X86Features,
) -> Result<(), &'static str> {
    let conv = match sig.cc {
        CallConv::C | CallConv::COLD => Conv::C,
        CallConv::FAST if !sig.variadic => Conv::Fast,
        _ => return Err(CC),
    };
    let mut at = assign_ret(types, sig, conv, out, features)?;
    let mut regs =
        Regs { list: if conv == Conv::Fast { &FASTCC } else { &INREG }, next: 0, taken: 0 };
    let mut xmm = 0;
    for (index, &ty) in sig.params.iter().enumerate() {
        if index == sig.fixed as usize {
            out.va.stack_offset = at;
        }
        let attrs = sig.attrs(index);
        if odd(attrs) {
            return Err(ATTR);
        }
        let arg = index as u32;
        let inreg = attrs.has(ParamAttrs::INREG);
        match attrs.mem() {
            ParamMem::Sret => {
                if index != 0 || !is_gpr(types, ty) {
                    return Err(ATTR);
                }
                if conv == Conv::Fast {
                    return Err(CC);
                }
                out.sret_returned_in = Some(EAX);
            }
            ParamMem::ByVal => {
                let Some(pointee) = attrs.mem_type() else { return Err(ATTR) };
                let align = attrs.align().unwrap_or(4).max(4);
                if inreg || align > 16 || !is_gpr(types, ty) {
                    return Err(ATTR);
                }
                let size = types.size_of(pointee);
                let Ok(size) = u32::try_from(size) else { return Err(hod_cg::abi::SIZE_LIMIT) };
                at = at.next_multiple_of(align as u32);
                if size != 0 {
                    let place = Place::Stack(at);
                    out.args.push(Part { arg, kind: PartKind::Pointee, offset: 0, size, place });
                }
                at = at.checked_add(size.next_multiple_of(4)).ok_or(hod_cg::abi::SIZE_LIMIT)?;
                continue;
            }
            _ => {}
        }
        if attrs.has(ParamAttrs::NEST) {
            let reg = if conv == Conv::Fast { EAX } else { ECX };
            if !is_gpr(types, ty) || regs.holds(reg) {
                return Err(ATTR);
            }
            regs.taken |= 1 << reg.index();
            out.args.push(Part {
                arg,
                kind: PartKind::Value,
                offset: 0,
                size: 4,
                place: Place::Reg(reg),
            });
            continue;
        }
        if types.is_vector(ty) && !features.has(X86Features::SSE) {
            // Without SSE each lane is an argument of its own (WP-338).
            let (n, stride, leaf) = vector_lanes(types, ty)?;
            if inreg {
                return Err(crate::vecscalar::ABI);
            }
            if conv == Conv::Fast && matches!(leaf, Leaf::Fp { .. }) {
                return Err(CC);
            }
            let (size, slot) = match leaf {
                Leaf::Int(size) => (size, 4),
                Leaf::Fp { size, slot } => (size, slot),
            };
            for k in 0..n {
                let offset = k * stride;
                let place = if let Some(reg) = (conv == Conv::Fast).then(|| regs.take()).flatten() {
                    Place::Reg(reg)
                } else {
                    at = at.checked_add(slot).ok_or(hod_cg::abi::SIZE_LIMIT)?;
                    Place::Stack(at - slot)
                };
                push_part(out, Part { arg, kind: PartKind::Value, offset, size, place });
            }
            continue;
        }
        if types.is_vector(ty) {
            let (place, size) =
                crate::sse::vector_place(types, ty, features, sig.variadic, &mut xmm, &mut at)?;
            out.args.push(Part { arg, kind: PartKind::Value, offset: 0, size, place });
            continue;
        }
        if matches!(types.kind(ty), TypeKind::Half) {
            crate::sse::half_place(features, inreg || conv == Conv::Fast, false)?;
            at = at.checked_add(4).ok_or(hod_cg::abi::SIZE_LIMIT)?;
            let place = Place::Stack(at - 4);
            out.args.push(Part { arg, kind: PartKind::Value, offset: 0, size: 2, place });
            continue;
        }
        if matches!(types.kind(ty), TypeKind::Int(128) | TypeKind::Fp128) {
            return Err(WIDE_ARG);
        }
        let mut leaves = Leaves::new();
        leaves.walk(types, ty, 0)?;
        if leaves.full {
            return Err(crate::target::AGGREGATE);
        }
        let aggregate = matches!(types.kind(ty), TypeKind::Struct { .. } | TypeKind::Array { .. });
        let has_fp = leaves.iter().any(|(_, leaf)| matches!(leaf, Leaf::Fp { .. }));
        let in_regs = match conv {
            Conv::C if inreg => {
                if has_fp || sig.variadic || aggregate {
                    return Err(ATTR);
                }
                true
            }
            Conv::C => false,
            // `fastcc` floating point depends on SSE2 (`xmm0`-`xmm2`).
            Conv::Fast if has_fp => return Err(CC),
            // Each leaf of an aggregate is an `i32` of its own (`04-aggr`
            // probe in the grid, WP-297).
            Conv::Fast => true,
        };
        for (offset, leaf) in leaves.iter() {
            let (size, slot) = match leaf {
                Leaf::Int(size) => (size, 4),
                Leaf::Fp { size, slot } => (size, slot),
            };
            let place = if let Some(reg) = in_regs.then(|| regs.take()).flatten() {
                Place::Reg(reg)
            } else {
                at = at.checked_add(slot).ok_or(hod_cg::abi::SIZE_LIMIT)?;
                Place::Stack(at - slot)
            };
            push_part(out, Part { arg, kind: PartKind::Value, offset, size, place });
        }
    }
    if sig.fixed as usize >= sig.params.len() {
        out.va.stack_offset = at;
    }
    out.stack_size = at;
    Ok(())
}

/// The result's parts (§5), or its hidden pointer; answers the offset of
/// the first argument slot.
fn assign_ret(
    types: &TypeTable,
    sig: &Sig<'_>,
    conv: Conv,
    out: &mut Assignment,
    features: X86Features,
) -> Result<u32, &'static str> {
    if sig.ret == TypeId::VOID {
        return Ok(0);
    }
    let attrs = sig.ret_attrs;
    if odd(attrs)
        || attrs.has(ParamAttrs::INREG)
        || attrs.has(ParamAttrs::NEST)
        || !matches!(attrs.mem(), ParamMem::None | ParamMem::ElementType)
    {
        return Err(ATTR);
    }
    if types.is_vector(sig.ret) && !features.has(X86Features::SSE) {
        // Without SSE the lanes are returned as an aggregate's leaves:
        // three words, one x87 value, else in memory (WP-338).
        let (n, stride, leaf) = vector_lanes(types, sig.ret)?;
        let fp = matches!(leaf, Leaf::Fp { .. });
        if fp && conv == Conv::Fast {
            return Err(CC);
        }
        if (fp && n > 2) || (!fp && n as usize > RET.len()) {
            if conv == Conv::Fast {
                return Err(CC);
            }
            out.ret_memory = Some(RetMemory { address: Place::Stack(0), returned_in: Some(EAX) });
            return Ok(4);
        }
        if fp && n == 2 {
            return Err(X87_PAIR);
        }
        let (size, _) = match leaf {
            Leaf::Int(size) => (size, 4),
            Leaf::Fp { size, slot } => (size, slot),
        };
        for (k, &word) in (0..n).zip(RET.iter()) {
            let place = Place::Reg(if fp { ST0 } else { word });
            out.ret.push(Part { arg: 0, kind: PartKind::Value, offset: k * stride, size, place });
        }
        return Ok(0);
    }
    if types.is_vector(sig.ret) {
        let (place, size) =
            crate::sse::vector_place(types, sig.ret, features, false, &mut 0, &mut 0)?;
        out.ret.push(Part { arg: 0, kind: PartKind::Value, offset: 0, size, place });
        return Ok(0);
    }
    if matches!(types.kind(sig.ret), TypeKind::Half) {
        if let Some(reg) = crate::sse::half_place(features, conv == Conv::Fast, true)? {
            out.ret.push(Part {
                arg: 0,
                kind: PartKind::Value,
                offset: 0,
                size: 2,
                place: Place::Reg(reg),
            });
        }
        return Ok(0);
    }
    let mut leaves = Leaves::new();
    let in_memory = match types.kind(sig.ret) {
        TypeKind::Int(128) | TypeKind::Fp128 => true,
        _ => {
            leaves.walk(types, sig.ret, 0)?;
            let fp = leaves.iter().filter(|(_, l)| matches!(l, Leaf::Fp { .. })).count();
            let words = leaves.iter().filter(|(_, l)| matches!(l, Leaf::Int(_))).count();
            if conv == Conv::Fast && fp != 0 {
                return Err(CC);
            }
            if leaves.full || words > RET.len() || fp > 2 {
                true
            } else if fp == 2 {
                return Err(X87_PAIR);
            } else {
                false
            }
        }
    };
    if in_memory {
        if conv == Conv::Fast {
            return Err(CC);
        }
        out.ret_memory = Some(RetMemory { address: Place::Stack(0), returned_in: Some(EAX) });
        return Ok(4);
    }
    let mut words = RET.iter();
    for (offset, leaf) in leaves.iter() {
        let (size, reg) = match leaf {
            Leaf::Int(size) => (size, *words.next().ok_or(crate::target::AGGREGATE)?),
            Leaf::Fp { size, .. } => (size, ST0),
        };
        out.ret.push(Part { arg: 0, kind: PartKind::Value, offset, size, place: Place::Reg(reg) });
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_ir::DataLayout;

    const LAYOUT: &str =
        "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128";

    fn table() -> TypeTable {
        TypeTable::new(&DataLayout::parse(LAYOUT).unwrap())
    }

    fn sig<'a>(
        cc: CallConv,
        ret: TypeId,
        params: &'a [TypeId],
        attrs: &'a [ParamAttrs],
    ) -> Sig<'a> {
        Sig {
            cc,
            ret,
            ret_attrs: ParamAttrs::NONE,
            params,
            param_attrs: attrs,
            fixed: params.len() as u32,
            variadic: false,
        }
    }

    fn places(asg: &Assignment) -> Vec<(u32, u32, u32, Place)> {
        asg.args.iter().map(|p| (p.arg, p.offset, p.size, p.place)).collect()
    }

    /// i386 join C: an integer of 65 to 128 bits at the boundary is
    /// `x86:i64:wide:abi` (WP-346), a wider one `x86:i64:wide:width`, one
    /// of 33 to 63 bits (no `Gpr` value, no ABI leaf) `x86:i64`.
    #[test]
    fn integer_refusals_by_width() {
        let mut types = table();
        assert_eq!(type_refusal(&types, TypeId::I128), "x86:i64:wide:abi");
        let i200 = types.int(200).unwrap();
        assert_eq!(type_refusal(&types, i200), "x86:i64:wide:width");
        let i40 = types.int(40).unwrap();
        assert_eq!(type_refusal(&types, i40), "x86:i64");
    }

    #[test]
    fn stack_slots_and_results() {
        let types = table();
        let params = [TypeId::I8, TypeId::I64, TypeId::DOUBLE, TypeId::X86_FP80, TypeId::I32];
        let mut asg = Assignment::default();
        assign(&types, &sig(CallConv::C, TypeId::I64, &params, &[]), &mut asg).unwrap();
        let s = Place::Stack;
        assert_eq!(
            places(&asg),
            [
                (0, 0, 1, s(0)),
                (1, 0, 8, s(4)),
                (2, 0, 8, s(12)),
                (3, 0, 10, s(20)),
                (4, 0, 4, s(32))
            ]
        );
        assert_eq!(asg.stack_size, 36);
        let ret: Vec<_> = asg.ret.iter().map(|p| (p.offset, p.place)).collect();
        assert_eq!(ret, [(0, Place::Reg(EAX)), (4, Place::Reg(EDX))]);
        assign(&types, &sig(CallConv::C, TypeId::X86_FP80, &[], &[]), &mut asg).unwrap();
        assert_eq!(asg.ret[0].place, Place::Reg(ST0));
        assert_eq!(asg.ret[0].size, 10);
    }

    #[test]
    fn inreg_splits_an_i64_and_sret_pops() {
        let types = table();
        let inreg = ParamAttrs::NONE.with(ParamAttrs::INREG).unwrap();
        let params = [TypeId::I32, TypeId::I32, TypeId::I64, TypeId::I32];
        let mut asg = Assignment::default();
        assign(&types, &sig(CallConv::C, TypeId::VOID, &params, &[inreg; 4]), &mut asg).unwrap();
        let (r, s) = (Place::Reg, Place::Stack);
        assert_eq!(
            places(&asg),
            [
                (0, 0, 4, r(EAX)),
                (1, 0, 4, r(EDX)),
                (2, 0, 4, r(ECX)),
                (2, 4, 4, s(0)),
                (3, 0, 4, s(4))
            ]
        );
        assert_eq!(asg.callee_pop, 0);
        let sret = ParamAttrs::NONE.with_mem(ParamMem::Sret, TypeId::I32).unwrap();
        let ptr = TypeId::PTR;
        assign(&types, &sig(CallConv::C, TypeId::VOID, &[ptr], &[sret]), &mut asg).unwrap();
        assert_eq!(asg.callee_pop, 4);
        let both = sret.with(ParamAttrs::INREG).unwrap();
        assign(&types, &sig(CallConv::C, TypeId::VOID, &[ptr], &[both]), &mut asg).unwrap();
        assert_eq!(asg.callee_pop, 0);
        // A result in memory: the hidden pointer is popped too.
        assign(&types, &sig(CallConv::C, TypeId::I128, &[], &[]), &mut asg).unwrap();
        assert_eq!(asg.callee_pop, 4);
    }

    #[test]
    fn conventions() {
        let types = table();
        let mut asg = Assignment::default();
        let params = [TypeId::I32; 3];
        assign(&types, &sig(CallConv::FAST, TypeId::I32, &params, &[]), &mut asg).unwrap();
        let (r, s) = (Place::Reg, Place::Stack);
        assert_eq!(places(&asg), [(0, 0, 4, r(ECX)), (1, 0, 4, r(EDX)), (2, 0, 4, s(0))]);
        assign(&types, &sig(CallConv::COLD, TypeId::I32, &params, &[]), &mut asg).unwrap();
        assert_eq!(asg.stack_size, 12);
        let err = assign(&types, &sig(CallConv::X86_STDCALL, TypeId::I32, &params, &[]), &mut asg);
        assert_eq!(err, Err(AbiUnsupported(CC)));
        assert!(asg.args.is_empty());
    }
}
