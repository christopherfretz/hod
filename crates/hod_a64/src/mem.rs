//! Memory instructions (`spec/codegen.md` §5.2, §5.5.1 "Memory"):
//! `load` and `store` of `Gpr`-class values, `alloca`, `ptradd`,
//! `llvm.stacksave` and `llvm.stackrestore`.
//!
//! **Owner: WP-036.** Tests: `tests/mem.rs`. Each entry point starts with
//! [`deferred_first`](crate::common::deferred_first), naming the address
//! operand when it fuses a deferred `ptradd` into the addressing mode.
//! A lowering that leaves the stack pointer below the frame sets
//! `cx.state.frame.dynamic` (`crate::frame::FrameState::dynamic`).
//!
//! # Addresses
//!
//! An address is a base register, a constant offset and at most one index
//! register ([`Addr`]). The base is the frame pointer for a stack slot,
//! `x17` (the load or store) or the result's register (`ptradd`) for a
//! symbol, a constant address or an over-aligned slot, and otherwise the
//! register of the pointer value. A `ptradd` whose only use is the next
//! instruction is deferred (§4.6); a `load` or `store` through it folds its
//! constant offset and constant indices into the offset and its other
//! indices into the index register, so that one access is
//! `[base, #off]`, `[base, index, lsl #log2]` or `[base, wIndex, sxtw
//! #log2]`. Unfused, `ptradd` computes the same sum into a register.
//!
//! # Widths
//!
//! An `iN` occupies its store size, ⌈N/8⌉ bytes, with the bits above `N`
//! zero (§2.2): `store` masks a width that is not a multiple of 8 and
//! writes exactly the store size; `load` reads exactly the store size,
//! zero-extended. A store size of 3, 5, 6 or 7 bytes is two or three
//! accesses, as `llc` does; nothing outside the store size is read or
//! written.

use hod_cg::{CgError, CgResult, Class, FnCx, FrameRef, LIMIT_FRAME_SIZE, PReg, SlotPlace, Target};
use hod_ir::{Const, Inst, InstKind, Intrinsic, MemFlags, PtrAdd, Value, ValueKind};
use hod_mc_a64 as mc;
use hod_mc_a64::{Extend, Index, Load, MemSize, Reg, RegZr, Shift, Size};

use crate::common::{
    Cursor, SP, X16, X17, add_const, deferred_first, frame_address, load_gpr_at,
    materialize_gpr_const, mov_imm, put, slot_address, store_gpr_at, sym_address, unsupported,
    value_ty, xr,
};
use crate::target::{A64, ATOMIC};

/// `load` of a register value: the group's name. Nothing is refused
/// under it since WP-036.
pub const LOAD: &str = "a64:load";
/// `store` of a register value: the group's name.
pub const STORE: &str = "a64:store";
/// `alloca`: the group's name.
pub const ALLOCA: &str = "a64:alloca";
/// `ptradd`: the group's name.
pub const PTRADD: &str = "a64:ptradd";
/// `llvm.stacksave`, `llvm.stackrestore`: the group's name.
pub const STACK_INTRINSIC: &str = "a64:stack-intrinsic";
/// A `load` of a floating-point or vector-register value: WP-039.
pub const LOAD_FPR: &str = "a64:load-fpr";
/// A `store` of a floating-point or vector-register value: WP-039.
pub const STORE_FPR: &str = "a64:store-fpr";
/// A `ptradd` whose result is a vector of pointers: no package yet (the
/// vector element operations, `target::VECTOR`).
pub const PTRADD_VECTOR: &str = "a64:ptradd-vector";

/// The largest symbol offset folded into the relocation's addend, as
/// `llc` folds (2^20): beyond it the offset is added after the address.
const MAX_FOLDED_ADDEND: i64 = 1 << 20;

/// An index register: `(sxtw ? sext(reg<31:0>) : reg) << shift`, with
/// `shift <= 4`.
#[derive(Clone, Copy, Debug)]
struct Idx {
    reg: PReg,
    sxtw: bool,
    shift: u32,
}

impl Idx {
    fn x(self) -> Reg {
        xr(self.reg)
    }

    fn extend(self) -> Extend {
        if self.sxtw { Extend::Sxtw } else { Extend::Uxtx }
    }
}

/// `base + off (+ index)`. `owned` says that `base` is a register this
/// instruction may overwrite (an operand's, from `use_reg`).
#[derive(Clone, Copy, Debug)]
struct Addr {
    base: Reg,
    owned: Option<PReg>,
    off: i64,
    index: Option<Idx>,
}

// ---- operands ----

/// The deferred `ptradd` that defines `ptr`, if there is one to fuse.
fn fusable<'a>(cx: &FnCx<'a, A64>, ptr: Value) -> Option<PtrAdd<'a>> {
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
/// width (at most 64), if `value` is one that folds.
fn const_int(cx: &FnCx<'_, A64>, value: Value) -> Option<(u64, u32)> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Int { ty, value } => Some((value, cx.types().int_bits(ty).unwrap_or(64).min(64))),
        Const::WideInt { words, .. } => Some((words.first().copied().unwrap_or(0), 64)),
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) | Const::Null(_) => Some((0, 64)),
        _ => None,
    }
}

/// An integer operand in a general register, pinned and writable, with
/// the width of its meaningful low bits (at most 64). A `Mem`-class one
/// (`i128`) contributes its low 64 bits: everything an address or a size
/// computed modulo 2^64 needs.
fn int_in_reg(cx: &mut FnCx<'_, A64>, value: Value) -> CgResult<(PReg, u32)> {
    if cx.class_of(value) == Class::Mem {
        let slot = cx.use_mem(value)?;
        let reg = cx.temp(Class::Gpr)?;
        let mut at = Cursor::frame(cx, slot, X16);
        load_gpr_at(cx, Load::X, xr(reg), &mut at);
        return Ok((reg, 64));
    }
    let bits = cx.types().int_bits(value_ty(cx, value)).unwrap_or(64).min(64);
    Ok((cx.use_reg(value)?, bits))
}

// ---- address arithmetic ----

/// `index * scale` with the index sign-extended from `bits`, as an
/// [`Idx`]: a 32-bit index stays a `W` register to be `sxtw`-extended,
/// a power-of-two scale up to 16 stays a shift.
fn scaled(cx: &mut FnCx<'_, A64>, reg: PReg, bits: u32, scale: u64) -> Idx {
    let r = xr(reg);
    let w32 = bits == 32;
    if bits < 64 && !w32 {
        put(cx, mc::sbfx(Size::X, r.zr(), r.zr(), 0, bits));
    }
    if scale.is_power_of_two() && scale.trailing_zeros() <= 4 {
        return Idx { reg, sxtw: w32, shift: scale.trailing_zeros() };
    }
    if w32 {
        put(cx, mc::sxtw(r.zr(), r.zr()));
    }
    if scale.is_power_of_two() {
        put(cx, mc::lsl_imm(Size::X, r.zr(), r.zr(), scale.trailing_zeros()));
    } else {
        mov_imm(cx, Size::X, X16, scale);
        put(cx, mc::mul(Size::X, r.zr(), r.zr(), X16.zr()));
    }
    Idx { reg, sxtw: false, shift: 0 }
}

/// The index register holding its plain 64-bit value.
fn settle(cx: &mut FnCx<'_, A64>, i: Idx) -> Idx {
    let r = i.x();
    if i.sxtw {
        put(cx, mc::sxtw(r.zr(), r.zr()));
    }
    if i.shift != 0 {
        put(cx, mc::lsl_imm(Size::X, r.zr(), r.zr(), i.shift));
    }
    Idx { reg: i.reg, sxtw: false, shift: 0 }
}

/// `rd = rn + index` (64-bit), `rn` any general register or the frame
/// pointer.
fn add_index(cx: &mut FnCx<'_, A64>, rd: Reg, rn: Reg, i: Idx) {
    put(cx, mc::add_ext(Size::X, rd.sp(), rn.sp(), i.x().zr(), i.extend(), i.shift));
}

/// The address `base + offset + Σ index × scale` (the operands of a
/// `ptradd`, or a pointer alone with `add` `None`), as an [`Addr`]. A
/// base that is a symbol, a constant address or an over-aligned slot is
/// put in `into`, after every operand has been asked for: `into` is a
/// scratch register or a register the caller already allocated, and no
/// glue runs after it is written.
fn compute<'a>(
    cx: &mut FnCx<'a, A64>,
    base: Value,
    offset: i64,
    add: Option<&PtrAdd<'a>>,
    into: Reg,
) -> CgResult<Addr> {
    let owned = if has_no_register(cx, base) { None } else { Some(cx.use_reg(base)?) };
    let mut off = offset;
    let mut acc: Option<Idx> = None;
    if let Some(add) = add {
        for (index, scale) in add.indices() {
            if scale == 0 {
                continue;
            }
            if let Some((value, bits)) = const_int(cx, index) {
                off = off.wrapping_add(sext(value, bits).wrapping_mul(scale as i64));
                continue;
            }
            let (reg, bits) = int_in_reg(cx, index)?;
            let term = scaled(cx, reg, bits, scale);
            acc = Some(match acc {
                None => term,
                Some(first) => {
                    let first = settle(cx, first);
                    add_index(cx, first.x(), first.x(), term);
                    cx.release(index);
                    cx.free_temp(reg);
                    first
                }
            });
        }
    }
    let base = match (owned, base.kind()) {
        (Some(reg), _) => xr(reg),
        (None, ValueKind::Slot(slot)) => match cx.slot_place(slot)? {
            SlotPlace::Direct(FrameRef::Local(at)) => {
                off = off.wrapping_add(i64::from(at));
                Reg::FP
            }
            SlotPlace::Direct(place) => {
                frame_address(cx, into, place);
                into
            }
            SlotPlace::Indirect(_) => {
                slot_address(cx, into, slot)?;
                into
            }
        },
        (None, ValueKind::Sym(sym)) => {
            off = symbol(cx, into, sym, 0, off)?;
            into
        }
        (None, ValueKind::Const(id)) => {
            if let Const::Addr { sym, addend } = cx.module.consts.get(id) {
                off = symbol(cx, into, sym, addend, off)?;
            } else {
                materialize_gpr_const(cx, into, id)?;
            }
            into
        }
        (None, _) => return cx.bug("an address operand that is no value"),
    };
    Ok(Addr { base, owned, off, index: acc })
}

/// Whether the base `value` is formed in a register of the lowering's
/// choice rather than asked for (`use_reg`): a stack slot, a symbol, a
/// constant symbol address. Their offsets fold.
fn has_no_register(cx: &FnCx<'_, A64>, value: Value) -> bool {
    match value.kind() {
        ValueKind::Slot(_) | ValueKind::Sym(_) => true,
        ValueKind::Const(id) => matches!(cx.module.consts.get(id), Const::Addr { .. }),
        _ => false,
    }
}

/// Puts `sym + addend + off` in `rd`, folding `off` into the relocation
/// when the sum is small; returns what is left to add.
fn symbol(
    cx: &mut FnCx<'_, A64>,
    rd: Reg,
    sym: hod_ir::SymId,
    addend: i64,
    off: i64,
) -> CgResult<i64> {
    let total = addend.wrapping_add(off);
    if (-MAX_FOLDED_ADDEND..MAX_FOLDED_ADDEND).contains(&total) {
        sym_address(cx, rd, sym, total)?;
        Ok(0)
    } else {
        sym_address(cx, rd, sym, addend)?;
        Ok(off)
    }
}

/// The address of a `load` or `store` operand, fusing a deferred
/// `ptradd`; a base without a register of its own goes to `x17`.
fn address(cx: &mut FnCx<'_, A64>, ptr: Value) -> CgResult<Addr> {
    match fusable(cx, ptr) {
        Some(add) => compute(cx, add.base, add.offset, Some(&add), X17),
        None => compute(cx, ptr, 0, None, X17),
    }
}

/// Where one access of `1 << log2` bytes goes.
enum Place {
    /// `[base, #off]`, rebased into `x17` when out of reach.
    Imm(Cursor),
    /// `[base, index, <extend> #(scaled ? log2 : 0)]`.
    Reg { base: Reg, index: Reg, ext: Index, scaled: bool },
}

fn place(cx: &mut FnCx<'_, A64>, a: Addr, log2: u32) -> Place {
    let Some(mut i) = a.index else {
        return Place::Imm(Cursor { base: a.base.sp(), off: a.off, tmp: X17, drift: 0 });
    };
    if a.off != 0 || (i.shift != 0 && i.shift != log2) {
        i = settle(cx, i);
        add_const(cx, i.x().sp(), i.x().sp(), a.off);
    }
    let ext = if i.sxtw { Index::Sxtw } else { Index::Lsl };
    Place::Reg { base: a.base, index: i.x(), ext, scaled: i.shift != 0 }
}

/// The address as a cursor with no index: the index, if any, is added
/// to the base into the index register.
fn flat(cx: &mut FnCx<'_, A64>, a: Addr) -> Cursor {
    match a.index {
        None => Cursor { base: a.base.sp(), off: a.off, tmp: X17, drift: 0 },
        Some(i) => {
            add_index(cx, i.x(), a.base, i);
            Cursor { base: i.x().sp(), off: a.off, tmp: X17, drift: 0 }
        }
    }
}

// ---- load and store ----

/// The access of a store size that is a power of two.
fn mem_size(bytes: u64) -> Option<MemSize> {
    Some(match bytes {
        1 => MemSize::B,
        2 => MemSize::H,
        4 => MemSize::W,
        8 => MemSize::X,
        _ => return None,
    })
}

/// The zero-extending load of `ms`.
fn load_op(ms: MemSize) -> Load {
    match ms {
        MemSize::B => Load::B,
        MemSize::H => Load::H,
        MemSize::W => Load::W,
        MemSize::X => Load::X,
    }
}

/// The pieces of a store size of 3, 5, 6 or 7 bytes after the first, as
/// `(offset, size)`; the first is 2 bytes for 3 and 4 bytes otherwise.
fn tail_pieces(bytes: u64) -> &'static [(u32, MemSize)] {
    match bytes {
        3 => &[(2, MemSize::B)],
        5 => &[(4, MemSize::B)],
        6 => &[(4, MemSize::H)],
        _ => &[(4, MemSize::H), (6, MemSize::B)],
    }
}

/// The first piece of a split access.
fn head_piece(bytes: u64) -> MemSize {
    if bytes == 3 { MemSize::H } else { MemSize::W }
}

fn check_access(mem: MemFlags) -> CgResult<()> {
    if mem.ordering().is_some() {
        return unsupported(ATOMIC);
    }
    Ok(())
}

/// `%r = load <ty>, ptr %ptr` of class `Gpr` or `Fpr`.
///
/// # Errors
/// [`LOAD_FPR`] for an `Fpr`-class value; `a64:atomic` for an atomic
/// load; what asking for the operands reports.
pub fn load<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    check_access(mem)?;
    if A64.class_of(cx.types(), inst.ty()) != Class::Gpr {
        return unsupported(LOAD_FPR);
    }
    let fused = fusable(cx, ptr).map(|_| ptr);
    deferred_first(cx, inst, fused)?;
    let bytes = cx.types().size_of(inst.ty());
    let rd = cx.def_reg(inst)?;
    let rt = xr(rd);
    let a = address(cx, ptr)?;
    if let Some(ms) = mem_size(bytes) {
        match place(cx, a, ms.log2()) {
            Place::Imm(mut at) => load_gpr_at(cx, load_op(ms), rt, &mut at),
            Place::Reg { base, index, ext, scaled } => {
                put(cx, mc::ldr_reg(load_op(ms), rt.zr(), base.sp(), index.zr(), ext, scaled));
            }
        }
    } else {
        // `rt` is a fresh temporary: no part of the address is in it.
        let mut at = flat(cx, a);
        load_gpr_at(cx, load_op(head_piece(bytes)), rt, &mut at);
        let mut done = 0;
        for &(offset, ms) in tail_pieces(bytes) {
            at.advance(offset - done);
            done = offset;
            load_gpr_at(cx, load_op(ms), X16, &mut at);
            put(cx, mc::orr_shifted(Size::X, rt.zr(), rt.zr(), X16.zr(), Shift::Lsl, offset * 8));
        }
    }
    cx.finish(inst, rd)
}

/// Whether `value` is a constant whose image is all zero bits.
fn is_zero(cx: &FnCx<'_, A64>, value: Value) -> bool {
    matches!(const_int(cx, value), Some((0, _)))
}

/// `store <ty> %value, ptr %ptr` of class `Gpr` or `Fpr`.
///
/// # Errors
/// [`STORE_FPR`] for an `Fpr`-class value; `a64:atomic` for an atomic
/// store; what asking for the operands reports.
pub fn store<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    check_access(mem)?;
    if cx.class_of(value) != Class::Gpr {
        return unsupported(STORE_FPR);
    }
    let fused = fusable(cx, ptr).map(|_| ptr);
    deferred_first(cx, inst, fused)?;
    let ty = value_ty(cx, value);
    let bytes = cx.types().size_of(ty);
    let src = if is_zero(cx, value) {
        RegZr::ZR
    } else {
        let r = xr(cx.use_reg(value)?);
        // §2.2: the bits above the width are unspecified in the
        // register and zero in memory. An integer constant has them clear.
        let bits = cx.types().int_bits(ty).unwrap_or(64);
        if !bits.is_multiple_of(8) && !matches!(value.kind(), ValueKind::Const(_)) {
            let sz = if bits <= 32 { Size::W } else { Size::X };
            put(cx, mc::and_imm(sz, r.sp(), r.zr(), (1u64 << bits) - 1));
        }
        r.zr()
    };
    let a = address(cx, ptr)?;
    if let Some(ms) = mem_size(bytes) {
        match place(cx, a, ms.log2()) {
            Place::Imm(mut at) => store_gpr_at(cx, ms, src, &mut at),
            Place::Reg { base, index, ext, scaled } => {
                put(cx, mc::str_reg(ms, src, base.sp(), index.zr(), ext, scaled));
            }
        }
    } else {
        let mut at = flat(cx, a);
        store_gpr_at(cx, head_piece(bytes), src, &mut at);
        let mut done = 0;
        for &(offset, ms) in tail_pieces(bytes) {
            at.advance(offset - done);
            done = offset;
            let part = if src.is_zr() {
                RegZr::ZR
            } else {
                put(cx, mc::lsr_imm(Size::X, X16.zr(), src, offset * 8));
                X16.zr()
            };
            store_gpr_at(cx, ms, part, &mut at);
        }
    }
    Ok(())
}

// ---- alloca, ptradd, the stack intrinsics ----

/// `%r = alloca <elem_size>, %count, align <align>` (§5.2): the stack
/// pointer lowered by `count × elem_size` rounded up to 16, then aligned
/// down to `align` when that is above 16; the result is the new stack
/// pointer.
///
/// # Errors
/// `limit:frame-size` for a constant size beyond 2^63 or an alignment
/// beyond 2^32; what asking for the count reports.
pub fn alloca<'a>(
    cx: &mut FnCx<'a, A64>,
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
        if size < 1 << 24 {
            add_const(cx, X16.sp(), SP, -(size as i64));
        } else {
            mov_imm(cx, Size::X, X17, size);
            put(cx, mc::sub_ext(Size::X, X16.sp(), SP, X17.zr(), Extend::Uxtx, 0));
        }
    } else {
        let (reg, bits) = int_in_reg(cx, count)?;
        let r = xr(reg);
        if bits < 64 {
            put(cx, mc::ubfx(Size::X, r.zr(), r.zr(), 0, bits));
        }
        if elem_size.is_power_of_two() {
            if elem_size > 1 {
                put(cx, mc::lsl_imm(Size::X, r.zr(), r.zr(), elem_size.trailing_zeros()));
            }
        } else {
            // Zero included: `mul` by `xzr`'s value 0 via `x16`.
            mov_imm(cx, Size::X, X16, elem_size);
            put(cx, mc::mul(Size::X, r.zr(), r.zr(), X16.zr()));
        }
        put(cx, mc::add_imm(Size::X, r.sp(), r.sp(), 15));
        put(cx, mc::and_imm(Size::X, r.sp(), r.zr(), !15));
        put(cx, mc::sub_ext(Size::X, X16.sp(), SP, r.zr(), Extend::Uxtx, 0));
    }
    if align > 16 {
        put(cx, mc::and_imm(Size::X, X16.sp(), X16.zr(), !(align - 1)));
    }
    put(cx, mc::mov_sp(Size::X, SP, X16.sp()));
    put(cx, mc::mov(Size::X, xr(rd).zr(), X16.zr()));
    cx.state.frame.dynamic = true;
    cx.finish(inst, rd)
}

/// `%r = ptradd %base, <offset> + Σ index × stride`: deferred when its
/// only use is the next instruction (§4.6), else computed into the
/// register of its index or base, or a new one.
///
/// # Errors
/// [`PTRADD_VECTOR`] for a vector of pointers; what asking for the
/// operands reports.
pub fn ptr_add<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, add: &PtrAdd<'a>) -> CgResult<()> {
    if A64.class_of(cx.types(), inst.ty()) != Class::Gpr {
        return unsupported(PTRADD_VECTOR);
    }
    // A deferred operand is emitted now, so that a load or store that
    // fuses this one finds every operand in a location.
    deferred_first(cx, inst, None)?;
    if cx.may_defer(inst) {
        return cx.defer(inst);
    }
    let rd = if has_no_register(cx, add.base) { Some(cx.def_reg(inst)?) } else { None };
    let into = rd.map_or(X17, xr);
    let a = compute(cx, add.base, add.offset, Some(add), into)?;
    let result = match (a.index, a.owned, rd) {
        (Some(i), _, _) => {
            add_index(cx, i.x(), a.base, i);
            add_const(cx, i.x().sp(), i.x().sp(), a.off);
            i.reg
        }
        (None, Some(reg), _) => {
            add_const(cx, a.base.sp(), a.base.sp(), a.off);
            reg
        }
        (None, None, Some(rd)) => {
            add_const(cx, xr(rd).sp(), a.base.sp(), a.off);
            rd
        }
        (None, None, None) => return cx.bug("a ptradd base with no register"),
    };
    cx.finish(inst, result)
}

/// `llvm.stacksave` (the stack pointer) and `llvm.stackrestore` (the
/// stack pointer set; the epilogue then restores it from the frame
/// pointer).
///
/// # Errors
/// What asking for the operand reports; `Internal` for another
/// intrinsic.
pub fn stack_intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    match (which, args) {
        (Intrinsic::StackSave, _) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mov_sp(Size::X, xr(rd).sp(), SP));
            cx.finish(inst, rd)
        }
        (Intrinsic::StackRestore, &[ptr]) => {
            let r = cx.use_reg(ptr)?;
            put(cx, mc::mov_sp(Size::X, SP, xr(r).sp()));
            cx.state.frame.dynamic = true;
            Ok(())
        }
        _ => cx.bug("a stack intrinsic of another shape"),
    }
}
