//! Memory instructions (`spec/codegen.md` §5.2, §5.6.3 "Memory"):
//! `load` and `store` of `Gpr`- and `Fpr`-class values that are not
//! atomic, `alloca`, `ptradd`, `llvm.stacksave` and `llvm.stackrestore`.
//!
//! **Owner: WP-123** (X-mem). Tests: `tests/mem.rs`. The decisions are
//! AArch64's (WP-036, `hod_a64::mem`); only the instructions differ.
//!
//! # Addresses
//!
//! An address is a frame place, or `[base + index * scale + disp32]`
//! ([`At`]). A `ptradd` whose only use is the next instruction is
//! deferred (§4.6), and the `load` or `store` through it folds its
//! constant offset and constant indices into the displacement and its
//! variable indices into the index register. Unfused, `ptradd` computes
//! the same address with `lea`.
//!
//! The registers an address is built in are the ones that never hold a
//! value (§5.6) and the scratch registers: variable indices in `rcx`
//! (the sum) and `rdx` (the next term), a base without a register of its
//! own (a symbol, a constant address, a slot that is not addressed from
//! the frame directly) in `r11`, a scale beyond 32 bits in `r10`. Every
//! operand is asked for before the first of these is written, so no
//! glue runs in between; `rax` is left to the access itself.
//!
//! # Widths
//!
//! An `iN` occupies its store size, ⌈N/8⌉ bytes, with the bits above `N`
//! zero (§2.2): `store` masks a width that is not a multiple of 8 and
//! writes exactly the store size; `load` reads exactly the store size,
//! zero-extended. A store size of 3, 5, 6 or 7 bytes is two or three
//! accesses, as `llc` does. An `Fpr` value is one access of its store
//! size: `pinsrw`/`pextrw` for 2 bytes, `movss`, `movsd`, `movups`.

use hod_cg::{CgError, CgResult, Class, FnCx, FrameRef, LIMIT_FRAME_SIZE, SlotPlace, Target};
use hod_ir::{Const, Inst, InstKind, Intrinsic, MemFlags, PtrAdd, SymId, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{FpSize, Inst as McInst, Mem, Reg, Scale, Size, Xmm};

use crate::common::{
    R10, R11, RSP, add_addend, deferred_first, gp, gpr_load, materialize_gpr_const, mov_imm, put,
    slot_address, sym_address, unsupported, value_ty, xm,
};
use crate::target::X64;

/// The group's refusal before WP-123. Nothing is refused under it any
/// more; kept for the coverage log's history.
pub const MEM: &str = "x64:todo:mem";
/// A `ptradd` whose result is a vector of pointers: the vector element
/// operations' (as `a64:ptradd-vector` on AArch64).
pub const PTRADD_VECTOR: &str = "x64:ptradd-vector";

const RAX: Reg = Reg::Rax;
/// The sum of the variable indices.
const RCX: Reg = Reg::Rcx;
/// The next variable index.
const RDX: Reg = Reg::Rdx;

/// Where an access goes.
#[derive(Clone, Copy, Debug)]
pub enum At {
    /// A frame place (a stack slot addressed directly, its offset
    /// folded): through `common::frame_inst`.
    Frame(FrameRef),
    /// `[base + index * scale + disp]`. `disp` leaves room for the last
    /// piece of a split access (at most `+15`).
    Mem {
        /// The base register.
        base: Reg,
        /// The index register and its scale.
        index: Option<(Reg, Scale)>,
        /// The displacement.
        disp: i32,
    },
}

/// Emits the instruction `make` builds over the memory operand `k` bytes
/// into `at`.
fn access(cx: &mut FnCx<'_, X64>, at: At, k: u32, make: impl FnOnce(Mem) -> McInst) {
    match at {
        At::Frame(place) => crate::common::frame_inst(cx, place.offset_by(k), make),
        At::Mem { base, index, disp } => {
            let disp = disp.wrapping_add(k as i32);
            let m = match index.and_then(|(r, s)| r.index().map(|i| (i, s))) {
                Some((i, s)) => Mem::base_index(base, i, s, disp),
                None => Mem::base(base, disp),
            };
            put(cx, make(m));
        }
    }
}

// ---- operands ----

/// The deferred `ptradd` that defines `ptr`, if there is one to fuse.
fn fusable<'a>(cx: &FnCx<'a, X64>, ptr: Value) -> Option<PtrAdd<'a>> {
    match cx.deferred(ptr)?.kind() {
        InstKind::PtrAdd(add) => Some(add),
        _ => None,
    }
}

/// `value` sign-extended from `bits` (1 to 64).
fn sext(value: u64, bits: u32) -> i64 {
    let unused = 64 - bits.clamp(1, 64);
    ((value << unused) as i64) >> unused
}

/// The value of an integer constant operand as its low 64 bits and its
/// width (at most 64), if `value` is one that folds; zero-like constants
/// are 0.
fn const_int(cx: &FnCx<'_, X64>, value: Value) -> Option<(u64, u32)> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Int { ty, value } => Some((value, cx.types().int_bits(ty).unwrap_or(64).min(64))),
        Const::WideInt { words, .. } => Some((words.first().copied().unwrap_or(0), 64)),
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) | Const::Null(_) => Some((0, 64)),
        _ => None,
    }
}

/// The image of a floating-point constant of `bytes` bytes (2, 4, 8 or
/// 16) as two 64-bit words, low first, if `value` is one.
fn const_fpr(cx: &FnCx<'_, X64>, value: Value) -> Option<(u64, u64)> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Float { bits, .. } => Some((bits, 0)),
        Const::WideFloat { bits, .. } => Some((bits as u64, (bits >> 64) as u64)),
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some((0, 0)),
        _ => None,
    }
}

/// Puts the integer operand `value` in `reg` as a full 64-bit register
/// and returns the width of its meaningful low bits (at most 64). A
/// `Mem`-class one (`i128`) contributes its low 64 bits: everything an
/// address or a size computed modulo 2^64 needs.
fn int_into(cx: &mut FnCx<'_, X64>, value: Value, reg: Reg) -> CgResult<u32> {
    if cx.class_of(value) == Class::Mem {
        let slot = cx.use_mem(value)?;
        crate::common::frame_inst(cx, slot, |m| mc::mov_rm(Size::Q, reg, m));
        return Ok(64);
    }
    let bits = cx.types().int_bits(value_ty(cx, value)).unwrap_or(64).min(64);
    cx.put(value, crate::reg::gpr(reg))?;
    Ok(bits)
}

/// `reg` sign-extended from its low `bits` bits to 64 (flags written
/// only for a width that is not 8, 16, 32 or 64).
pub fn sign_extend(cx: &mut FnCx<'_, X64>, reg: Reg, bits: u32) {
    match bits {
        64.. => {}
        32 => put(cx, mc::movsxd_rr(reg, reg)),
        16 => put(cx, mc::movsx_rr(Size::Q, reg, Size::W, reg)),
        8 => put(cx, mc::movsx_rr(Size::Q, reg, Size::B, reg)),
        _ => {
            let unused = (64 - bits) as u8;
            put(cx, mc::shl_ri(Size::Q, reg, unused));
            put(cx, mc::sar_ri(Size::Q, reg, unused));
        }
    }
}

/// `reg` zero-extended from its low `bits` bits to 64.
pub fn zero_extend(cx: &mut FnCx<'_, X64>, reg: Reg, bits: u32) {
    match bits {
        64.. => {}
        32 => put(cx, mc::mov_rr(Size::D, reg, reg)),
        16 => put(cx, mc::movzx_rr(Size::D, reg, Size::W, reg)),
        8 => put(cx, mc::movzx_rr(Size::D, reg, Size::B, reg)),
        1..32 => put(cx, mc::and_ri(Size::D, reg, ((1u64 << bits) - 1) as i64)),
        _ => {
            let unused = (64 - bits) as u8;
            put(cx, mc::shl_ri(Size::Q, reg, unused));
            put(cx, mc::shr_ri(Size::Q, reg, unused));
        }
    }
}

/// The scale of an addressing mode for a factor of 1, 2, 4 or 8.
fn sib_scale(factor: u64) -> Option<Scale> {
    Scale::ALL.into_iter().find(|s| u64::from(s.factor()) == factor)
}

/// `reg *= scale` (64-bit, wrapping) where the addressing mode cannot
/// scale; returns the scale left for the addressing mode.
fn scale_term(cx: &mut FnCx<'_, X64>, reg: Reg, scale: u64) -> Scale {
    if let Some(s) = sib_scale(scale) {
        return s;
    }
    if scale.is_power_of_two() {
        put(cx, mc::shl_ri(Size::Q, reg, scale.trailing_zeros() as u8));
    } else if let Ok(small) = i32::try_from(scale as i64) {
        put(cx, mc::imul_rri(Size::Q, reg, reg, i64::from(small)));
    } else {
        put(cx, mc::mov_ri(Size::Q, R10, scale as i64));
        put(cx, mc::imul_rr(Size::Q, reg, R10));
    }
    Scale::S1
}

/// The index register `rcx` holding its plain value (scale 1).
fn settle(cx: &mut FnCx<'_, X64>, s: Scale) {
    if s != Scale::S1
        && let Some(i) = RCX.index()
    {
        put(cx, mc::lea(Size::Q, RCX, Mem::index(i, s, 0)));
    }
}

/// `rcx * s1 + rdx * s2` into `rcx`, scale 1.
fn combine(cx: &mut FnCx<'_, X64>, s1: Scale, s2: Scale) {
    let (Some(ic), Some(id)) = (RCX.index(), RDX.index()) else { return };
    let m = if s1 == Scale::S1 {
        Mem::base_index(RCX, id, s2, 0)
    } else if s2 == Scale::S1 {
        Mem::base_index(RDX, ic, s1, 0)
    } else {
        settle(cx, s1);
        Mem::base_index(RCX, id, s2, 0)
    };
    put(cx, mc::lea(Size::Q, RCX, m));
}

/// How the base of an address is reached.
enum BaseOf {
    /// A register value's register.
    Reg(Reg),
    /// A stack slot.
    Slot(hod_ir::SlotId),
    /// A symbol and an addend.
    Sym(SymId, i64),
    /// Another constant (a number as a pointer, null).
    Const(hod_ir::ConstId),
}

/// The largest displacement kept in an addressing mode: room is left
/// for the pieces of a split access.
const MAX_DISP: i64 = i32::MAX as i64 - 16;
/// The largest offset folded into a frame place.
const MAX_FRAME_OFFSET: i64 = 1 << 24;

/// The address `base + offset + Σ index × scale` (the operands of a
/// `ptradd`, or a pointer alone with `add` `None`), as an [`At`]. The
/// base register is asked for first (`use_reg`), then the indices are
/// put in `rcx` and `rdx`, then a base without a register is formed in
/// `r11`. `rax` is not written.
///
/// # Errors
/// What asking for the operands and forming a symbol's or a constant's
/// address report.
pub fn compute<'a>(
    cx: &mut FnCx<'a, X64>,
    base: Value,
    offset: i64,
    add: Option<&PtrAdd<'a>>,
) -> CgResult<At> {
    let base_of = match base.kind() {
        ValueKind::Slot(slot) => BaseOf::Slot(slot),
        ValueKind::Sym(sym) => BaseOf::Sym(sym, 0),
        ValueKind::Const(id) => match cx.module.consts.get(id) {
            Const::Addr { sym, addend } => BaseOf::Sym(sym, addend),
            _ => BaseOf::Const(id),
        },
        _ => BaseOf::Reg(gp(cx.use_reg(base)?)),
    };
    let mut off = offset;
    let mut acc: Option<Scale> = None;
    if let Some(add) = add {
        for (index, scale) in add.indices() {
            if scale == 0 {
                continue;
            }
            if let Some((value, bits)) = const_int(cx, index) {
                off = off.wrapping_add(sext(value, bits).wrapping_mul(scale as i64));
                continue;
            }
            let reg = if acc.is_none() { RCX } else { RDX };
            let bits = int_into(cx, index, reg)?;
            sign_extend(cx, reg, bits);
            let s = scale_term(cx, reg, scale);
            acc = Some(match acc {
                None => s,
                Some(first) => {
                    combine(cx, first, s);
                    Scale::S1
                }
            });
        }
    }
    let base_of = match base_of {
        BaseOf::Sym(sym, addend) => {
            // The whole constant offset goes to the symbol's address.
            sym_address(cx, R11, sym, addend.wrapping_add(off))?;
            return Ok(At::Mem { base: R11, index: acc.map(|s| (RCX, s)), disp: 0 });
        }
        BaseOf::Slot(slot) if acc.is_none() && (0..MAX_FRAME_OFFSET).contains(&off) => {
            if let SlotPlace::Direct(place) = cx.slot_place(slot)? {
                return Ok(At::Frame(place.offset_by(off as u32)));
            }
            BaseOf::Slot(slot)
        }
        other => other,
    };
    if !(i64::from(i32::MIN)..=MAX_DISP).contains(&off) {
        // Into the index before `r11` is written (`add_addend` may use it).
        match acc {
            Some(s) => {
                settle(cx, s);
                add_addend(cx, RCX, off);
            }
            None => put(cx, mc::mov_ri(Size::Q, RCX, off)),
        }
        acc = Some(Scale::S1);
        off = 0;
    }
    let base = match base_of {
        BaseOf::Reg(reg) => reg,
        BaseOf::Slot(slot) => {
            slot_address(cx, R11, slot)?;
            R11
        }
        BaseOf::Const(id) => {
            materialize_gpr_const(cx, R11, id)?;
            R11
        }
        BaseOf::Sym(..) => return cx.bug("a symbol base after its address was formed"),
    };
    Ok(At::Mem { base, index: acc.map(|s| (RCX, s)), disp: off as i32 })
}

/// The address of a `load` or `store` operand, fusing a deferred
/// `ptradd`.
///
/// # Errors
/// As [`compute`].
pub fn address(cx: &mut FnCx<'_, X64>, ptr: Value) -> CgResult<At> {
    match fusable(cx, ptr) {
        Some(add) => compute(cx, add.base, add.offset, Some(&add)),
        None => compute(cx, ptr, 0, None),
    }
}

// ---- load and store ----

fn check_access(mem: MemFlags) -> CgResult<()> {
    if mem.ordering().is_some() {
        return unsupported(crate::atomic::ATOMIC);
    }
    Ok(())
}

/// The size of an `Fpr` value's single access.
fn fpr_bytes(cx: &FnCx<'_, X64>, bytes: u64) -> CgResult<u32> {
    match bytes {
        2 | 4 | 8 | 16 => Ok(bytes as u32),
        _ => cx.bug("an Fpr-class value whose store size is no register view"),
    }
}

/// `%r = load <ty>, ptr %ptr` of class `Gpr` or `Fpr`.
///
/// # Errors
/// `x64:todo:atomic` for an atomic load (the dispatch sends those to
/// `atomic`); what asking for the operands reports.
pub fn load<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    check_access(mem)?;
    let fused = fusable(cx, ptr).map(|_| ptr);
    deferred_first(cx, inst, fused)?;
    let class = X64.class_of(cx.types(), inst.ty());
    let bytes = cx.types().size_of(inst.ty());
    let rd = cx.def_reg(inst)?;
    let at = address(cx, ptr)?;
    if class == Class::Fpr {
        let bytes = fpr_bytes(cx, bytes)?;
        load_fpr(cx, at, xm(rd), bytes);
        return cx.finish(inst, rd);
    }
    load_gpr(cx, at, gp(rd), bytes as u32);
    cx.finish(inst, rd)
}

/// The access of an `Fpr` value of `bytes` bytes into `x`.
fn load_fpr(cx: &mut FnCx<'_, X64>, at: At, x: Xmm, bytes: u32) {
    match bytes {
        2 => access(cx, at, 0, |m| mc::pinsrw_rmi(x, m, 0)),
        4 => access(cx, at, 0, |m| mc::movs_rm(FpSize::S, x, m)),
        8 => access(cx, at, 0, |m| mc::movs_rm(FpSize::D, x, m)),
        _ => access(cx, at, 0, |m| mc::movup_rm(FpSize::S, x, m)),
    }
}

/// Exactly `bytes` bytes (1 to 8) at `at` into `rd`, zero-extended: one
/// access for 1, 2, 4 and 8; for 3, 5, 6 and 7 the head and one or two
/// pieces through `rax` and `rdx`, or-ed in.
fn load_gpr(cx: &mut FnCx<'_, X64>, at: At, rd: Reg, bytes: u32) {
    match bytes {
        1 | 2 | 4 | 8 => access(cx, at, 0, |m| gpr_load(rd, m, bytes)),
        3 => {
            access(cx, at, 0, |m| gpr_load(rd, m, 2));
            access(cx, at, 2, |m| gpr_load(RAX, m, 1));
            put(cx, mc::shl_ri(Size::D, RAX, 16));
            put(cx, mc::or_rr(Size::D, rd, RAX));
        }
        _ => {
            access(cx, at, 0, |m| gpr_load(rd, m, 4));
            match bytes {
                5 => access(cx, at, 4, |m| gpr_load(RAX, m, 1)),
                6 => access(cx, at, 4, |m| gpr_load(RAX, m, 2)),
                _ => {
                    access(cx, at, 6, |m| gpr_load(RAX, m, 1));
                    access(cx, at, 4, |m| gpr_load(RDX, m, 2));
                    put(cx, mc::shl_ri(Size::D, RAX, 16));
                    put(cx, mc::or_rr(Size::D, RAX, RDX));
                }
            }
            put(cx, mc::shl_ri(Size::Q, RAX, 32));
            put(cx, mc::or_rr(Size::Q, rd, RAX));
        }
    }
}

/// The source of a store.
#[derive(Clone, Copy)]
enum Src {
    /// A register value of class `Gpr`.
    Gpr(Reg),
    /// A register value of class `Fpr`.
    Fpr(Xmm),
    /// A constant's image, low word first (the bits above the width
    /// clear).
    Imm(u64, u64),
}

/// `store <ty> %value, ptr %ptr` of class `Gpr` or `Fpr`.
///
/// # Errors
/// `x64:todo:atomic` for an atomic store (the dispatch sends those to
/// `atomic`); what asking for the operands reports.
pub fn store<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    check_access(mem)?;
    let fused = fusable(cx, ptr).map(|_| ptr);
    deferred_first(cx, inst, fused)?;
    let ty = value_ty(cx, value);
    let bytes = cx.types().size_of(ty) as u32;
    let class = cx.class_of(value);
    let src = if class == Class::Fpr {
        fpr_bytes(cx, u64::from(bytes))?;
        match const_fpr(cx, value) {
            Some((lo, hi)) => Src::Imm(lo, hi),
            None => Src::Fpr(xm(cx.use_reg(value)?)),
        }
    } else {
        match const_int(cx, value) {
            Some((v, _)) => Src::Imm(v, 0),
            None => Src::Gpr(gp(cx.use_reg(value)?)),
        }
    };
    let at = address(cx, ptr)?;
    match src {
        Src::Fpr(x) => match bytes {
            2 => {
                put(cx, mc::pextrw_rri(RAX, x, 0));
                access(cx, at, 0, |m| mc::mov_mr(Size::W, m, RAX));
            }
            4 => access(cx, at, 0, |m| mc::movs_mr(FpSize::S, m, x)),
            8 => access(cx, at, 0, |m| mc::movs_mr(FpSize::D, m, x)),
            _ => access(cx, at, 0, |m| mc::movup_mr(FpSize::S, m, x)),
        },
        Src::Imm(lo, hi) => {
            if bytes == 16 {
                store_imm(cx, at, 0, lo, 8);
                store_imm(cx, at, 8, hi, 8);
            } else if crate::common::exact_size(bytes).is_some() {
                store_imm(cx, at, 0, lo, bytes);
            } else {
                mov_imm(cx, RAX, lo, 64);
                store_pieces(cx, at, bytes);
            }
        }
        Src::Gpr(r) => {
            let bits = cx.types().int_bits(ty).unwrap_or(64);
            match crate::common::exact_size(bytes) {
                Some(size) if bits.is_multiple_of(8) => {
                    access(cx, at, 0, |m| mc::mov_mr(size, m, r));
                }
                _ => {
                    // §2.2: the bits above the width are unspecified in
                    // the register and zero in memory.
                    put(cx, mc::mov_rr(Size::Q, RAX, r));
                    if !bits.is_multiple_of(8) {
                        zero_extend(cx, RAX, bits);
                    }
                    store_pieces(cx, at, bytes);
                }
            }
        }
    }
    Ok(())
}

/// Stores the low `bytes` bytes (1, 2, 4 or 8) of the constant `value`
/// `k` bytes into `at`: `mov m, imm`, through `rax` for a 64-bit value
/// that is no sign-extended 32-bit one.
fn store_imm(cx: &mut FnCx<'_, X64>, at: At, k: u32, value: u64, bytes: u32) {
    let size = crate::common::exact_size(bytes).unwrap_or(Size::Q);
    let imm = match size {
        Size::B => i64::from(value as u8),
        Size::W => i64::from(value as u16),
        Size::D => i64::from(value as u32),
        Size::Q => value as i64,
    };
    if size.fits_imm(imm) {
        access(cx, at, k, |m| mc::mov_mi(size, m, imm));
    } else {
        mov_imm(cx, RAX, value, 64);
        access(cx, at, k, |m| mc::mov_mr(Size::Q, m, RAX));
    }
}

/// Stores the low `bytes` bytes (1 to 8) of `rax`, and no byte more,
/// shifting `rax` down as it goes.
fn store_pieces(cx: &mut FnCx<'_, X64>, at: At, bytes: u32) {
    let store = |cx: &mut FnCx<'_, X64>, k: u32, size: Size| {
        access(cx, at, k, |m| mc::mov_mr(size, m, RAX));
    };
    match bytes {
        1 => store(cx, 0, Size::B),
        2 => store(cx, 0, Size::W),
        3 => {
            store(cx, 0, Size::W);
            put(cx, mc::shr_ri(Size::D, RAX, 16));
            store(cx, 2, Size::B);
        }
        4 => store(cx, 0, Size::D),
        8 => store(cx, 0, Size::Q),
        _ => {
            store(cx, 0, Size::D);
            put(cx, mc::shr_ri(Size::Q, RAX, 32));
            match bytes {
                5 => store(cx, 4, Size::B),
                6 => store(cx, 4, Size::W),
                _ => {
                    store(cx, 4, Size::W);
                    put(cx, mc::shr_ri(Size::D, RAX, 16));
                    store(cx, 6, Size::B);
                }
            }
        }
    }
}

// ---- alloca, ptradd, the stack intrinsics ----

/// `%r = alloca <elem_size>, %count, align <align>` (§5.2): the stack
/// pointer lowered by `count × elem_size` rounded up to 16, then aligned
/// down to `align` when that is above 16; the result is the new stack
/// pointer. The epilogue restores the stack pointer from `rbp`.
///
/// # Errors
/// `limit:frame-size` for a constant size beyond 2^63 or an alignment
/// beyond 2^32 or not a power of two (as AArch64); what asking for the
/// count reports.
pub fn alloca<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    count: Value,
    elem_size: u64,
    align: u64,
) -> CgResult<()> {
    let too_big = || CgError::unsupported(LIMIT_FRAME_SIZE);
    if !align.is_power_of_two() || align > 1 << 32 {
        return Err(too_big());
    }
    deferred_first(cx, inst, None)?;
    let rd = cx.def_reg(inst)?;
    if let Some((value, bits)) = const_int(cx, count) {
        let n = if bits < 64 { value & ((1u64 << bits) - 1) } else { value };
        let size = n
            .checked_mul(elem_size)
            .and_then(|s| s.checked_next_multiple_of(16))
            .filter(|&s| s < 1 << 63)
            .ok_or_else(too_big)?;
        if let Ok(size) = i32::try_from(size) {
            put(cx, mc::lea(Size::Q, R11, Mem::base(RSP, -size)));
        } else {
            put(cx, mc::mov_rr(Size::Q, R11, RSP));
            put(cx, mc::mov_ri(Size::Q, RAX, size as i64));
            put(cx, mc::sub_rr(Size::Q, R11, RAX));
        }
    } else {
        let bits = int_into(cx, count, RAX)?;
        zero_extend(cx, RAX, bits);
        if elem_size == 0 {
            put(cx, mc::mov_ri(Size::D, RAX, 0));
        } else if elem_size.is_power_of_two() {
            if elem_size > 1 {
                put(cx, mc::shl_ri(Size::Q, RAX, elem_size.trailing_zeros() as u8));
            }
        } else if let Ok(small) = i32::try_from(elem_size) {
            put(cx, mc::imul_rri(Size::Q, RAX, RAX, i64::from(small)));
        } else {
            put(cx, mc::mov_ri(Size::Q, R11, elem_size as i64));
            put(cx, mc::imul_rr(Size::Q, RAX, R11));
        }
        put(cx, mc::add_ri(Size::Q, RAX, 15));
        put(cx, mc::and_ri(Size::Q, RAX, -16));
        put(cx, mc::mov_rr(Size::Q, R11, RSP));
        put(cx, mc::sub_rr(Size::Q, R11, RAX));
    }
    if align > 16 {
        let mask = (align as i64).wrapping_neg();
        if Size::Q.fits_imm(mask) {
            put(cx, mc::and_ri(Size::Q, R11, mask));
        } else {
            put(cx, mc::mov_ri(Size::Q, RAX, mask));
            put(cx, mc::and_rr(Size::Q, R11, RAX));
        }
    }
    put(cx, mc::mov_rr(Size::Q, RSP, R11));
    put(cx, mc::mov_rr(Size::Q, gp(rd), R11));
    cx.finish(inst, rd)
}

/// `%r = ptradd %base, <offset> + Σ index × stride`: deferred when its
/// only use is the next instruction (§4.6), else computed with `lea`.
///
/// # Errors
/// [`PTRADD_VECTOR`] for a vector of pointers; what asking for the
/// operands reports.
pub fn ptr_add<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, add: &PtrAdd<'a>) -> CgResult<()> {
    if X64.class_of(cx.types(), inst.ty()) != Class::Gpr {
        return unsupported(PTRADD_VECTOR);
    }
    // A deferred operand is emitted now, so that a load or store that
    // fuses this one finds every operand in a location.
    deferred_first(cx, inst, None)?;
    if cx.may_defer(inst) {
        return cx.defer(inst);
    }
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    match compute(cx, add.base, add.offset, Some(add))? {
        At::Frame(place) => crate::common::frame_address(cx, r, place),
        At::Mem { base, index: None, disp: 0 } => {
            if base != r {
                put(cx, mc::mov_rr(Size::Q, r, base));
            }
        }
        at @ At::Mem { .. } => access(cx, at, 0, |m| mc::lea(Size::Q, r, m)),
    }
    cx.finish(inst, rd)
}

/// `llvm.stacksave` (the stack pointer) and `llvm.stackrestore` (the
/// stack pointer set; the epilogue restores it from `rbp`).
///
/// # Errors
/// What asking for the operand reports; `Internal` for another
/// intrinsic.
pub fn stack_intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    match (which, args) {
        (Intrinsic::StackSave, _) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mov_rr(Size::Q, gp(rd), RSP));
            cx.finish(inst, rd)
        }
        (Intrinsic::StackRestore, &[ptr]) => {
            let r = gp(cx.use_reg(ptr)?);
            put(cx, mc::mov_rr(Size::Q, RSP, r));
            Ok(())
        }
        _ => cx.bug("a stack intrinsic of another shape"),
    }
}
