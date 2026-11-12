//! What every lowering of this target shares: register conversions, frame
//! addressing, the moves between registers and memory, constant and
//! symbol materialization, and branches (`spec/codegen.md` §5.5, §6.2,
//! §7.4, §7.5).
//!
//! Owned by WP-033. The instruction-group modules build on these helpers
//! and do not re-implement them; a helper a group needs and this module
//! lacks is added here, in its own function, with a doc comment.
//!
//! # Conventions
//!
//! - `x16`, `x17`, `v30` and `v31` are the scratch registers
//!   ([`X16`], [`X17`], [`V30`], [`V31`]). A helper that needs one for a far
//!   offset takes one *other than* the registers it was given
//!   ([`other_scratch`]).
//! - A [`Cursor`] is a base register plus a byte offset into memory; it
//!   rebases itself into its scratch register when the next access's
//!   offset is out of reach of the immediate forms, so a walk over any
//!   range costs one instruction per access plus one per rebase.

use std::cell::RefCell;

use hod_cg::{
    Access, CgError, CgResult, FixupKind, FnCx, FrameRef, Label, PReg, SlotPlace, SymRef,
};
use hod_ir::{
    CodeModel, Const, ConstId, FixupTarget, Inst, SymId, TypeId, TypeKind, Value, ValueKind,
};
use hod_mc_a64 as mc;
use hod_mc_a64::{Fixup, Load, MemSize, Reg, RegSp, RegZr, Size, VMemSize, VReg};
use hod_obj::{A64Reloc, RelocKind};

use crate::reg;
use crate::target::A64;

/// The context of a lowering of this target.
pub type Cx<'c, 'a> = &'c mut FnCx<'a, A64>;

/// The first general scratch register: what the policy passes to glue,
/// and the indirect callee's register (`RegInfo::callee`).
pub const X16: Reg = Reg::X16;
/// The second general scratch register.
pub const X17: Reg = Reg::X17;
/// The first vector scratch register: what the policy passes to glue.
pub const V30: VReg = VReg::V30;
/// The second vector scratch register: the data register of copies.
pub const V31: VReg = VReg::V31;
/// The frame pointer as a base register.
pub const FP: RegSp = RegSp::new(29).expect("x29");
/// The stack pointer as a base register.
pub const SP: RegSp = RegSp::SP;

// ---- fixup kinds (`Target::patch_branch`) ----
//
// The `at` of a fixup is the offset of the instruction that holds the
// field; the displacement is the label's offset minus `at`, which is what
// every A64 PC-relative field counts from.

/// `imm26` of `b` and `bl`.
pub const JUMP26: FixupKind = FixupKind(0);
/// `imm19` of `b.cond`, `cbz` and `cbnz`.
pub const BRANCH19: FixupKind = FixupKind(1);
/// `imm14` of `tbz` and `tbnz`.
pub const TEST14: FixupKind = FixupKind(2);
/// `imm19` of a literal load (`ldr` from a data island).
pub const LITERAL19: FixupKind = FixupKind(3);
/// `immhi:immlo` of `adr`.
pub const ADR21: FixupKind = FixupKind(4);

/// The encoder's field for one of the fixup kinds above.
pub fn fixup_field(kind: FixupKind) -> Option<Fixup> {
    Some(match kind {
        JUMP26 => Fixup::Jump26,
        BRANCH19 => Fixup::Branch19,
        TEST14 => Fixup::Test14,
        LITERAL19 => Fixup::Literal19,
        ADR21 => Fixup::Adr21,
        _ => return None,
    })
}

// ---- errors ----

/// `Err(Unsupported(construct))`: what an instruction group's entry
/// point returns for what it does not implement yet. `construct` is a
/// stable `a64:<name>` the coverage log reports.
#[cold]
pub fn unsupported<R>(construct: &'static str) -> CgResult<R> {
    Err(CgError::unsupported(construct))
}

// ---- registers ----

/// The general register `reg` as an encoder operand.
///
/// # Panics
/// If `reg` is not one of `x0`..`x30`: a lowering asked for a register
/// of the wrong class, a bug.
#[inline]
pub fn xr(reg: PReg) -> Reg {
    assert!(reg::is_gpr(reg), "x{} is not a general register", reg.index());
    Reg::new(reg.index()).expect("checked above")
}

/// The vector register `reg` as an encoder operand.
///
/// # Panics
/// If `reg` is not one of `v0`..`v31`.
#[inline]
pub fn vr(reg: PReg) -> VReg {
    assert!(reg::is_fpr(reg), "register {} is not a vector register", reg.index());
    VReg::new(reg::number(reg)).expect("checked above")
}

/// A general scratch register other than `reg`: `x16`, or `x17` when
/// `reg` is `x16`.
#[inline]
pub fn other_scratch(reg: Reg) -> Reg {
    if reg == X16 { X17 } else { X16 }
}

/// Emits one instruction word.
#[inline]
pub fn put(cx: &mut FnCx<'_, A64>, word: u32) {
    cx.emit_u32(word);
}

/// Emits a sequence of instruction words.
#[inline]
pub fn put_all(cx: &mut FnCx<'_, A64>, words: &[u32]) {
    for &word in words {
        cx.emit_u32(word);
    }
}

// ---- types ----

/// The value's type.
#[inline]
pub fn value_ty(cx: &FnCx<'_, A64>, value: Value) -> TypeId {
    cx.body.value_type(cx.module, value)
}

/// The alloc size of `ty` in bytes, for a frame copy.
///
/// # Errors
/// `Unsupported("limit:frame-size")` beyond 4 GiB.
pub fn mem_bytes(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<u32> {
    u32::try_from(cx.types().alloc_size_of(ty))
        .map_err(|_| CgError::unsupported(hod_cg::LIMIT_FRAME_SIZE))
}

/// The register size for an integer of `bits` bits: `W` up to 32.
#[inline]
pub fn size_for_bits(bits: u32) -> Size {
    if bits <= 32 { Size::W } else { Size::X }
}

// ---- deferred operands (§4.6) ----

/// The lowering's part of §4.6: asks for every operand of `inst` that is
/// deferred, other than `fused` (the operand the instruction fuses, if
/// any), before any other operand pins a register: emitting a deferred
/// instruction needs registers of its own. Every group's entry point calls
/// this first, naming what it fuses.
///
/// # Errors
/// What emitting the deferred instruction reports.
pub fn deferred_first<'a>(cx: Cx<'_, 'a>, inst: Inst<'a>, fused: Option<Value>) -> CgResult<()> {
    for &operand in inst.operands() {
        if Some(operand) != fused && cx.deferred(operand).is_some() {
            cx.use_reg(operand)?;
        }
    }
    Ok(())
}

// ---- immediates ----

/// `rd = rn + value` (64-bit), with `rd` the only register written: one
/// or two `add`/`sub` immediates up to 2^24 - 1, more for larger values,
/// `mov rd, rn` for 0 between distinct registers, nothing for 0 in place.
pub fn add_const(cx: &mut FnCx<'_, A64>, rd: RegSp, rn: RegSp, value: i64) {
    let negative = value < 0;
    let mut left = value.unsigned_abs();
    let op = |rd, rn, imm| {
        if negative { mc::sub_imm(Size::X, rd, rn, imm) } else { mc::add_imm(Size::X, rd, rn, imm) }
    };
    let mut src = rn;
    while left > 0xff_ffff {
        put(cx, op(rd, src, 0x00ff_f000));
        src = rd;
        left -= 0x00ff_f000;
    }
    let (hi, lo) = (left & 0x00ff_f000, left & 0xfff);
    if hi != 0 {
        put(cx, op(rd, src, hi));
        src = rd;
    }
    if lo != 0 || src != rd {
        put(cx, op(rd, src, lo));
    }
}

/// Puts the integer `value` in `rd`, by the encoder's shortest sequence
/// (§7.5): its low 32 bits with a `W` sequence for `Size::W`.
#[inline]
pub fn mov_imm(cx: &mut FnCx<'_, A64>, sz: Size, rd: Reg, value: u64) {
    put_all(cx, mc::mov_imm(sz, rd, value).as_slice());
}

// ---- memory ----

/// A base register and an offset from it: where the next access of a
/// walk over memory is. `tmp` is the scratch register it rebases into.
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    /// The base register.
    pub base: RegSp,
    /// The byte offset from it.
    pub off: i64,
    /// The scratch register a rebase writes.
    pub tmp: Reg,
    /// The sum of the offsets rebased into `tmp`: what a walk whose base
    /// *is* `tmp` subtracts at its end to leave the base unchanged.
    pub drift: i64,
}

impl Cursor {
    /// A cursor at `of`, a frame place (§5.5): `Local(o)` is `[x29, #o]`,
    /// `Outgoing(o)` is `[sp, #o]`, `Incoming(n)` is `n` past the
    /// incoming base, which this loads into `tmp`.
    pub fn frame(cx: &mut FnCx<'_, A64>, of: FrameRef, tmp: Reg) -> Cursor {
        match of {
            FrameRef::Local(at) => Cursor { base: FP, off: i64::from(at), tmp, drift: 0 },
            FrameRef::Outgoing(at) => Cursor { base: SP, off: i64::from(at), tmp, drift: 0 },
            FrameRef::Incoming(at) => {
                // `begin_function` reserves the slot for every function
                // with stack-passed arguments (§5.5); without one this is
                // a bug in that decision, and glue cannot fail: trap.
                match cx.state.frame.incoming_base {
                    Some(slot) => put(cx, mc::ldr_off(Load::X, tmp.zr(), FP, i64::from(slot))),
                    None => put(cx, mc::brk(0xf000)),
                }
                Cursor { base: tmp.sp(), off: i64::from(at), tmp, drift: 0 }
            }
        }
    }

    /// A cursor at `offset` bytes past the address in `ptr`. With `tmp`
    /// equal to `ptr` a rebase moves `ptr` itself: undo it with
    /// [`Cursor::restore`].
    pub fn at_reg(ptr: Reg, offset: i64, tmp: Reg) -> Cursor {
        Cursor { base: ptr.sp(), off: offset, tmp, drift: 0 }
    }

    /// Moves `tmp` back by what the rebases added to it.
    pub fn restore(&self, cx: &mut FnCx<'_, A64>) {
        if self.drift != 0 {
            add_const(cx, self.tmp.sp(), self.tmp.sp(), -self.drift);
        }
    }

    /// The base and offset for an access of `1 << log2` bytes at the
    /// cursor: as it is when the scaled unsigned or the unscaled signed
    /// immediate form takes the offset, else rebased into `tmp` first.
    pub fn reach(&mut self, cx: &mut FnCx<'_, A64>, log2: u32) -> (RegSp, i64) {
        let scaled =
            self.off >= 0 && self.off & ((1 << log2) - 1) == 0 && (self.off >> log2) <= 4095;
        if !scaled && !(-256..=255).contains(&self.off) {
            add_const(cx, self.tmp.sp(), self.base, self.off);
            if self.base == self.tmp.sp() {
                self.drift += self.off;
            } else {
                self.drift = self.off;
            }
            self.base = self.tmp.sp();
            self.off = 0;
        }
        (self.base, self.off)
    }

    /// Moves the cursor `bytes` further on.
    #[inline]
    pub fn advance(&mut self, bytes: u32) {
        self.off += i64::from(bytes);
    }
}

/// Whether `off` is the scaled unsigned form's for an access of
/// `1 << log2` bytes; else the access uses the unscaled form.
#[inline]
fn is_scaled(off: i64, log2: u32) -> bool {
    off >= 0 && off & ((1 << log2) - 1) == 0 && (off >> log2) <= 4095
}

/// `ldr`/`ldur` of `op` into `rt` at the cursor.
pub fn load_gpr_at(cx: &mut FnCx<'_, A64>, op: Load, rt: Reg, at: &mut Cursor) {
    let log2 = op.mem_size().log2();
    let (base, off) = at.reach(cx, log2);
    let word = if is_scaled(off, log2) {
        mc::ldr_off(op, rt.zr(), base, off)
    } else {
        mc::ldur(op, rt.zr(), base, off)
    };
    put(cx, word);
}

/// `str`/`stur` of the low `ms` bytes of `rt` (or of zero) at the cursor.
pub fn store_gpr_at(cx: &mut FnCx<'_, A64>, ms: MemSize, rt: RegZr, at: &mut Cursor) {
    let log2 = ms.log2();
    let (base, off) = at.reach(cx, log2);
    let word = if is_scaled(off, log2) {
        mc::str_off(ms, rt, base, off)
    } else {
        mc::stur(ms, rt, base, off)
    };
    put(cx, word);
}

/// `ldr`/`ldur` of the `sz` view of `rt` at the cursor.
pub fn load_fpr_at(cx: &mut FnCx<'_, A64>, sz: VMemSize, rt: VReg, at: &mut Cursor) {
    let log2 = sz.log2();
    let (base, off) = at.reach(cx, log2);
    let word = if is_scaled(off, log2) {
        mc::ldr_fp_off(sz, rt, base, off)
    } else {
        mc::ldur_fp(sz, rt, base, off)
    };
    put(cx, word);
}

/// `str`/`stur` of the `sz` view of `rt` at the cursor.
pub fn store_fpr_at(cx: &mut FnCx<'_, A64>, sz: VMemSize, rt: VReg, at: &mut Cursor) {
    let log2 = sz.log2();
    let (base, off) = at.reach(cx, log2);
    let word = if is_scaled(off, log2) {
        mc::str_fp_off(sz, rt, base, off)
    } else {
        mc::stur_fp(sz, rt, base, off)
    };
    put(cx, word);
}

/// The general load that reads at least `bytes` (1 to 8) bytes: sizes
/// that are not a power of two read up to the next one, which glue may
/// (§2.2: the bits above are unspecified).
pub fn gpr_load_op(bytes: u32) -> Load {
    match bytes {
        0 | 1 => Load::B,
        2 => Load::H,
        3 | 4 => Load::W,
        _ => Load::X,
    }
}

/// The vector view that reads at least `bytes` (1 to 16) bytes.
pub fn fpr_view(bytes: u32) -> VMemSize {
    match bytes {
        0 | 1 => VMemSize::B,
        2 => VMemSize::H,
        3 | 4 => VMemSize::S,
        5..=8 => VMemSize::D,
        _ => VMemSize::Q,
    }
}

/// The power-of-two pieces of `bytes` (at most 15) in decreasing size,
/// as `(offset, log2 size)`: each offset is a multiple of its size.
fn pieces(bytes: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut at = 0;
    [3u32, 2, 1, 0].into_iter().filter_map(move |log2| {
        if bytes & (1 << log2) != 0 {
            let piece = (at, log2);
            at += 1 << log2;
            Some(piece)
        } else {
            None
        }
    })
}

/// The element view of `1 << log2` bytes.
fn elem(log2: u32) -> mc::Elem {
    match log2 {
        0 => mc::Elem::B,
        1 => mc::Elem::H,
        2 => mc::Elem::S,
        _ => mc::Elem::D,
    }
}

/// The vector memory view of `1 << log2` bytes.
fn vmem(log2: u32) -> VMemSize {
    match log2 {
        0 => VMemSize::B,
        1 => VMemSize::H,
        2 => VMemSize::S,
        3 => VMemSize::D,
        _ => VMemSize::Q,
    }
}

/// Stores the low `bytes` bytes of vector register `src` at the cursor.
/// A size that is not a power of two is stored piece by piece, each
/// piece above the first moved to the scalar view of `extract` first
/// (`extract` is not `src`, and is a scratch register).
pub fn store_fpr_bytes(
    cx: &mut FnCx<'_, A64>,
    src: VReg,
    bytes: u32,
    at: &mut Cursor,
    extract: VReg,
) {
    if bytes.is_power_of_two() && bytes <= 16 {
        store_fpr_at(cx, fpr_view(bytes), src, at);
        return;
    }
    // A 16-byte register holds at most 15 bytes here. The pieces are in
    // increasing offset order and contiguous, so one cursor walks them.
    for (offset, log2) in pieces(bytes.min(15)) {
        if offset == 0 {
            store_fpr_at(cx, vmem(log2), src, at);
        } else {
            put(cx, mc::dup_scalar(elem(log2), extract, src, offset >> log2));
            store_fpr_at(cx, vmem(log2), extract, at);
        }
        at.advance(1 << log2);
    }
}

/// Stores the low `bytes` (1 to 8) bytes of general register `src` at
/// the cursor. A size that is not a power of two stores its largest
/// piece from `src` and the others through `v31` and `v30`.
pub fn store_gpr_bytes(cx: &mut FnCx<'_, A64>, src: Reg, bytes: u32, at: &mut Cursor) {
    let ms = match bytes {
        1 => MemSize::B,
        2 => MemSize::H,
        4 => MemSize::W,
        8 => MemSize::X,
        _ => {
            put(cx, mc::fmov_gpr_to_fp(Size::X, V31, src.zr()));
            store_fpr_bytes(cx, V31, bytes.min(8), at, V30);
            return;
        }
    };
    store_gpr_at(cx, ms, src.zr(), at);
}

/// Copies `bytes` bytes from `src` to `dst` through `v31`, 16 bytes at a
/// time and the rest in decreasing powers of two. The cursors rebase
/// into their own scratch registers.
pub fn copy_bytes(cx: &mut FnCx<'_, A64>, dst: &mut Cursor, src: &mut Cursor, bytes: u32) {
    let mut left = bytes;
    while left >= 16 {
        load_fpr_at(cx, VMemSize::Q, V31, src);
        store_fpr_at(cx, VMemSize::Q, V31, dst);
        src.advance(16);
        dst.advance(16);
        left -= 16;
    }
    for log2 in [3u32, 2, 1, 0] {
        if left & (1 << log2) != 0 {
            load_fpr_at(cx, vmem(log2), V31, src);
            store_fpr_at(cx, vmem(log2), V31, dst);
            src.advance(1 << log2);
            dst.advance(1 << log2);
        }
    }
}

/// Writes `bytes` zero bytes at the cursor (`xzr` stores).
pub fn zero_bytes(cx: &mut FnCx<'_, A64>, at: &mut Cursor, bytes: u32) {
    let mut left = bytes;
    while left >= 8 {
        store_gpr_at(cx, MemSize::X, RegZr::ZR, at);
        at.advance(8);
        left -= 8;
    }
    for (log2, ms) in [(2u32, MemSize::W), (1, MemSize::H), (0, MemSize::B)] {
        if left & (1 << log2) != 0 {
            store_gpr_at(cx, ms, RegZr::ZR, at);
            at.advance(1 << log2);
        }
    }
}

// ---- symbols and constants (§7.4, §7.5) ----

/// Puts the address of `sym` plus `addend` in `rd`, by the access
/// `sym_access` names (§7.4): `adrp` + `add :lo12:` for a direct one,
/// with the addend in the relocations as `llc` writes it; `adrp` +
/// `ldr :got_lo12:` for one through the GOT, then the addend added.
/// Writes `rd` and, for a GOT addend beyond 24 bits, nothing else:
/// [`add_const`] reaches any addend through `rd` alone.
///
/// # Errors
/// `Internal` for a thread-local symbol (`classify` routes those to
/// `llvm.threadlocal.address`); `Unsupported("a64:code-model")` for a
/// code model other than small.
pub fn sym_address(cx: &mut FnCx<'_, A64>, rd: Reg, sym: SymId, addend: i64) -> CgResult<()> {
    if cx.options.code_model(cx.module) != CodeModel::Small {
        return unsupported(CODE_MODEL);
    }
    let target = SymRef::Module(sym);
    match hod_cg::sym_access(cx.module, cx.options, sym) {
        Access::Direct => {
            let at = cx.offset();
            put(cx, mc::adrp(rd.zr(), 0));
            cx.reloc(at, RelocKind::A64(A64Reloc::AdrPrelPgHi21), target, addend);
            put(cx, mc::add_imm(Size::X, rd.sp(), rd.sp(), 0));
            cx.reloc(at + 4, RelocKind::A64(A64Reloc::AddAbsLo12Nc), target, addend);
        }
        Access::Got => {
            let at = cx.offset();
            put(cx, mc::adrp(rd.zr(), 0));
            cx.reloc(at, RelocKind::A64(A64Reloc::AdrGotPage), target, 0);
            put(cx, mc::ldr_off(Load::X, rd.zr(), rd.sp(), 0));
            cx.reloc(at + 4, RelocKind::A64(A64Reloc::Ld64GotLo12Nc), target, 0);
            if addend != 0 {
                add_const(cx, rd.sp(), rd.sp(), addend);
            }
        }
        Access::Tls(_) => return cx.bug("the address of a thread-local symbol as an operand"),
    }
    Ok(())
}

/// A code model other than small (`spec/codegen.md` §5.5).
pub const CODE_MODEL: &str = "a64:code-model";
/// A `blockaddress` constant: WP-037.
pub const BLOCKADDRESS: &str = "a64:blockaddress";
/// A constant of class `Gpr` or `Fpr` this target does not materialize
/// (a vector constant, a constant expression).
pub const CONSTANT: &str = "a64:constant";
/// A `Mem`-class constant whose image has a relocation other than an
/// aligned 8-byte symbol address.
pub const CONSTANT_RELOC: &str = "a64:constant-relocation";

/// Puts the general-class constant `id` in `rd` (§7.5).
///
/// # Errors
/// [`BLOCKADDRESS`] and [`CONSTANT`] for what this does not cover.
pub fn materialize_gpr_const(cx: &mut FnCx<'_, A64>, rd: Reg, id: ConstId) -> CgResult<()> {
    match cx.module.consts.get(id) {
        Const::Int { ty, value } => {
            let bits = cx.types().int_bits(ty).unwrap_or(64);
            mov_imm(cx, size_for_bits(bits), rd, value);
        }
        Const::AddrOfInt { value } => mov_imm(cx, Size::X, rd, value),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => {
            mov_imm(cx, Size::X, rd, 0);
        }
        Const::Addr { sym, addend } => sym_address(cx, rd, sym, addend)?,
        Const::IntOfAddr { addr, .. } => return materialize_gpr_const(cx, rd, addr),
        Const::DsoLocalEquivalent(sym) | Const::NoCfi(sym) => sym_address(cx, rd, sym, 0)?,
        Const::BlockAddr { func, block } => block_address(cx, rd, func, block)?,
        _ => return unsupported(CONSTANT),
    }
    Ok(())
}

/// Puts the vector-class constant `id` in `rd` (§7.5): a scalar float by
/// the encoder's FP materialization through `x16`, `fp128` by its two
/// halves through `x16`, zero by `movi`.
///
/// # Errors
/// [`CONSTANT`] for a vector constant other than zero.
pub fn materialize_fpr_const(cx: &mut FnCx<'_, A64>, rd: VReg, id: ConstId) -> CgResult<()> {
    match cx.module.consts.get(id) {
        Const::Float { ty, bits } => {
            let fmt = match cx.types().kind(ty) {
                TypeKind::Float => mc::FpFormat::S,
                TypeKind::Double => mc::FpFormat::D,
                // `half` and `bfloat` are both a 16-bit pattern here.
                _ => mc::FpFormat::H,
            };
            put_all(cx, mc::fmov_const(fmt, rd, X16, bits).as_slice());
        }
        Const::WideFloat { bits, .. } => {
            mov_imm(cx, Size::X, X16, bits as u64);
            put(cx, mc::fmov_gpr_to_fp(Size::X, rd, X16.zr()));
            mov_imm(cx, Size::X, X16, (bits >> 64) as u64);
            put(cx, mc::fmov_gpr_to_hi(rd, X16.zr()));
        }
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => {
            put(cx, mc::movi_mask(mc::VecBits::V128, rd, 0));
        }
        _ => return unsupported(CONSTANT),
    }
    Ok(())
}

/// Puts the address of body slot `value` in `rd`, or loads it from its
/// pointer slot for an over-aligned one (§5.1).
///
/// # Errors
/// `Internal` for a slot the body does not have.
pub fn slot_address(cx: &mut FnCx<'_, A64>, rd: Reg, slot: hod_ir::SlotId) -> CgResult<()> {
    match cx.slot_place(slot)? {
        SlotPlace::Direct(place) => frame_address(cx, rd, place),
        SlotPlace::Indirect(pointer) => {
            let mut at = Cursor::frame(cx, pointer, rd);
            load_gpr_at(cx, Load::X, rd, &mut at);
        }
    }
    Ok(())
}

/// Puts the address of frame place `of` in `rd`, writing nothing else.
pub fn frame_address(cx: &mut FnCx<'_, A64>, rd: Reg, of: FrameRef) {
    let at = Cursor::frame(cx, of, rd);
    add_const(cx, rd.sp(), at.base, at.off);
}

thread_local! {
    /// The image and relocations of the `Mem`-class constant being
    /// written: one buffer per thread, so a warm worker writes
    /// aggregates without allocating.
    static IMAGE: RefCell<(Vec<u8>, Vec<hod_ir::Fixup>)> = const { RefCell::new((Vec::new(), Vec::new())) };
}

/// Writes the in-memory image of the `Mem`-class constant `value` to
/// `dst` (`spec/ir.md` §4.3): zero-like constants as `xzr` stores, wide
/// integers word by word, anything else from its flattened image, with
/// each aligned 8-byte symbol address materialized. Writes `x16` and
/// `x17` only.
///
/// # Errors
/// [`CONSTANT_RELOC`] for an image whose relocation is not an aligned
/// 8-byte symbol address; `Internal` for something that is no constant.
pub fn materialize_mem_const(cx: &mut FnCx<'_, A64>, dst: FrameRef, value: Value) -> CgResult<()> {
    let ValueKind::Const(id) = value.kind() else {
        return cx.bug("materialize_mem of something that is no constant");
    };
    let ty = cx.module.consts.ty(id);
    let bytes = mem_bytes(cx, ty)?;
    let mut at = Cursor::frame(cx, dst, X16);
    match cx.module.consts.get(id) {
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => {
            zero_bytes(cx, &mut at, bytes);
        }
        Const::Int { value, .. } => write_words(cx, &mut at, &[value], bytes),
        Const::WideInt { words, .. } => write_words(cx, &mut at, words, bytes),
        _ => {
            let (mut image, mut fixups) =
                IMAGE.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
            image.clear();
            fixups.clear();
            let flattened = cx.module.consts.flatten(cx.types(), id, &mut image, &mut fixups);
            let result = match flattened {
                Ok(()) => write_image(cx, &mut at, &image, &fixups),
                Err(_) => unsupported(CONSTANT),
            };
            IMAGE.with(|cell| *cell.borrow_mut() = (image, fixups));
            result?;
        }
    }
    Ok(())
}

/// Writes `bytes` bytes of the little-endian words `words` (zero past
/// their end) at the cursor through `x17`.
fn write_words(cx: &mut FnCx<'_, A64>, at: &mut Cursor, words: &[u64], bytes: u32) {
    let mut left = bytes;
    let mut index = 0;
    while left > 0 {
        let word = words.get(index).copied().unwrap_or(0);
        let take = left.min(8);
        store_word(cx, at, word, take);
        left -= take;
        index += 1;
    }
}

/// Stores the low `take` (1 to 8) bytes of `word` at the cursor and
/// advances it.
fn store_word(cx: &mut FnCx<'_, A64>, at: &mut Cursor, word: u64, take: u32) {
    if take == 8 {
        let rt = if word == 0 {
            RegZr::ZR
        } else {
            mov_imm(cx, Size::X, X17, word);
            X17.zr()
        };
        store_gpr_at(cx, MemSize::X, rt, at);
        at.advance(8);
        return;
    }
    for (offset, log2) in pieces(take) {
        // `take` is below 8 here, so each piece is at most 4 bytes.
        let part = (word >> (offset * 8)) & ((1u64 << (8u32 << log2)) - 1);
        let rt = if part == 0 {
            RegZr::ZR
        } else {
            mov_imm(cx, Size::W, X17, part);
            X17.zr()
        };
        let ms = [MemSize::B, MemSize::H, MemSize::W][log2 as usize];
        store_gpr_at(cx, ms, rt, at);
        at.advance(1 << log2);
    }
}

/// Writes a flattened image at the cursor, word by word.
fn write_image(
    cx: &mut FnCx<'_, A64>,
    at: &mut Cursor,
    image: &[u8],
    fixups: &[hod_ir::Fixup],
) -> CgResult<()> {
    if fixups.iter().any(|f| {
        f.size != 8 || !f.offset.is_multiple_of(8) || !matches!(f.target, FixupTarget::Sym(_))
    }) {
        return unsupported(CONSTANT_RELOC);
    }
    let mut fixup = fixups.iter().peekable();
    for (index, chunk) in image.chunks(8).enumerate() {
        let offset = index as u64 * 8;
        if let Some(f) = fixup.next_if(|f| f.offset == offset)
            && let FixupTarget::Sym(sym) = f.target
        {
            sym_address(cx, X17, sym, f.addend)?;
            store_gpr_at(cx, MemSize::X, X17.zr(), at);
            at.advance(8);
            continue;
        }
        let mut word = [0u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        store_word(cx, at, u64::from_le_bytes(word), chunk.len() as u32);
    }
    Ok(())
}

// ---- branches (§6.2, §6.3) ----

/// `b label`. Its 26-bit field reaches ±128 MiB, in far mode as well: a
/// function whose code a `b` cannot span is `limit:function-size`
/// (`spec/codegen.md` §5.5).
///
/// # Errors
/// What [`FnCx::fixup`] reports.
pub fn branch(cx: &mut FnCx<'_, A64>, label: Label) -> CgResult<()> {
    let at = cx.offset();
    put(cx, mc::b(0));
    cx.fixup(at, JUMP26, label)
}

/// `b.cond label`; in far mode `b.!cond +8` · `b label`.
///
/// # Errors
/// What [`FnCx::fixup`] reports.
pub fn branch_cond(cx: &mut FnCx<'_, A64>, cond: mc::Cond, label: Label) -> CgResult<()> {
    if cx.far() {
        put(cx, mc::b_cond(cond.invert(), 8));
        return branch(cx, label);
    }
    let at = cx.offset();
    put(cx, mc::b_cond(cond, 0));
    cx.fixup(at, BRANCH19, label)
}

/// `cbz`/`cbnz` (`nonzero`) of `rt` to `label`; in far mode the inverse
/// over a `b`.
///
/// # Errors
/// What [`FnCx::fixup`] reports.
pub fn branch_zero(
    cx: &mut FnCx<'_, A64>,
    sz: Size,
    rt: Reg,
    nonzero: bool,
    label: Label,
) -> CgResult<()> {
    let make = |nonzero: bool, disp| {
        if nonzero { mc::cbnz(sz, rt.zr(), disp) } else { mc::cbz(sz, rt.zr(), disp) }
    };
    if cx.far() {
        put(cx, make(!nonzero, 8));
        return branch(cx, label);
    }
    let at = cx.offset();
    put(cx, make(nonzero, 0));
    cx.fixup(at, BRANCH19, label)
}

/// `tbz`/`tbnz` (`set`) of bit `bit` of `rt` to `label`; in far mode the
/// inverse over a `b`.
///
/// # Errors
/// What [`FnCx::fixup`] reports.
pub fn branch_bit(
    cx: &mut FnCx<'_, A64>,
    rt: Reg,
    bit: u32,
    set: bool,
    label: Label,
) -> CgResult<()> {
    let make = |set: bool, disp| {
        if set { mc::tbnz(rt.zr(), bit, disp) } else { mc::tbz(rt.zr(), bit, disp) }
    };
    if cx.far() {
        put(cx, make(!set, 8));
        return branch(cx, label);
    }
    let at = cx.offset();
    put(cx, make(set, 0));
    cx.fixup(at, TEST14, label)
}

// ---- block addresses (§6.3, §9.6): added by WP-037 ----

/// Puts `blockaddress(func, block)` in `rd`, writing `rd` and one
/// scratch register other than it (added by WP-037). The address is
/// formed PC-relative by the function itself (§9.6): `adr rd, label`,
/// whose 21-bit field reaches ±1 MiB. In far mode the distance comes from
/// a `b label` placed in the code and jumped over, whose 26-bit field
/// reaches the whole function (§5.5.1) and which this reads back as data
/// (`.text` is readable on every supported platform):
/// `adr rd, 1f` · `ldr wT, [rd]` · `sbfx xT, xT, #0, #26` ·
/// `add rd, rd, xT, lsl #2` · `b 2f` · `1: b label` · `2:`.
/// The label of a block that is not emitted is bound to the shared trap
/// (§6.1), as the block's offset in data is.
///
/// # Errors
/// [`BLOCKADDRESS`] for a block of another function (`classify` rejects
/// it); what the fixup reports.
pub fn block_address(
    cx: &mut FnCx<'_, A64>,
    rd: Reg,
    func: SymId,
    block: hod_ir::BlockId,
) -> CgResult<()> {
    if func != cx.body.symbol() || block.index() >= cx.body.block_count() {
        return unsupported(BLOCKADDRESS);
    }
    let label = cx.block_label(block);
    if !cx.far() {
        let at = cx.offset();
        put(cx, mc::adr(rd.zr(), 0));
        return cx.fixup(at, ADR21, label);
    }
    let tmp = other_scratch(rd);
    put_all(
        cx,
        &[
            mc::adr(rd.zr(), 20),
            mc::ldr_off(Load::W, tmp.zr(), rd.sp(), 0),
            mc::sbfx(Size::X, tmp.zr(), tmp.zr(), 0, 26),
            mc::add_shifted(Size::X, rd.zr(), rd.zr(), tmp.zr(), mc::Shift::Lsl, 2),
            mc::b(8),
        ],
    );
    branch(cx, label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_are_aligned_and_cover() {
        for bytes in 1..=15u32 {
            let mut total = 0;
            for (offset, log2) in pieces(bytes) {
                assert_eq!(offset % (1 << log2), 0, "{bytes}");
                assert_eq!(offset, total);
                total += 1 << log2;
            }
            assert_eq!(total, bytes);
        }
    }

    #[test]
    fn fixup_kinds_are_distinct() {
        let kinds = [JUMP26, BRANCH19, TEST14, LITERAL19, ADR21];
        for (i, a) in kinds.iter().enumerate() {
            assert!(fixup_field(*a).is_some());
            for b in &kinds[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(fixup_field(FixupKind(9)).is_none());
    }
}
