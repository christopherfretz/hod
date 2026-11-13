//! Atomic memory operations (`spec/codegen.md` §5.6.10): atomic `load`
//! and `store`, `atomicrmw`, `cmpxchg`, `fence`.
//!
//! **Owner: WP-133** (X-atomic; design 0006 §2.7, §10.2). Tests:
//! `tests/atomic.rs`. `target.rs` sends every atomic `load` and `store`
//! here, whatever the value's class, and every `fence`, `cmpxchg` and
//! `atomicrmw`. The decisions that are not instruction selection are
//! AArch64's (`hod_a64::atomic`, WP-074); x86-64 is TSO, so most
//! orderings cost nothing.
//!
//! # Sequences (1 to 8 bytes)
//!
//! - **load**: a plain `mov` (`movzx` below 4 bytes) for every ordering,
//!   as `llc` writes it: an aligned access is single-copy atomic, and a
//!   load is never reordered with an older load or store on x86-64 except
//!   a store-to-load pair, which only a `seq_cst` store closes.
//! - **store**: `mov` for `unordered`, `monotonic` and `release`; `xchg`
//!   for `seq_cst` (as `llc` 19 and 22 do; `xchg` with memory is locked
//!   by the processor, a full barrier).
//! - **`atomicrmw`**: `xchg` for `xchg`, `lock xadd` for `add` and `sub`
//!   (the operand negated); when the result has no use, `lock add`,
//!   `lock sub`, `lock and`, `lock or`, `lock xor` with the operand. Every
//!   other operation is a `lock cmpxchg` loop: the old value in `rax`, the
//!   new one computed in `r10`, `jne` back on failure (which reloads
//!   `rax`). Inside the loop any instruction may run (a `cmpxchg` loop has
//!   no monitor to clear), but every register is requested before it.
//! - **`cmpxchg`**: `lock cmpxchg` with the expected value in `rax`; the
//!   flag is `sete`. A `weak` one is the same (it never fails
//!   spuriously, which the language reference allows).
//! - **`fence`**: `mfence` for `seq_cst`, nothing for `acquire`,
//!   `release`, `acq_rel` (TSO gives them for free) and nothing for any
//!   `syncscope("singlethread")` fence.
//! - `half`, `bfloat`, `float` and `double` move as their bits through
//!   `rax`; `fadd`, `fsub`, `fmax`, `fmin`, `fmaximum`, `fminimum` of
//!   `half`, `float` and `double` compute inside a `lock cmpxchg` loop
//!   (`half` in `float`, converted inline; WP-144, §5.6.14).
//!
//! # 16 bytes
//!
//! With `+cx16` in the function's `target-features` (the last `±cx16`
//! item decides), `i128` goes through `lock cmpxchg16b`: a load is a
//! `cmpxchg16b` of zero by zero (as `llc`: it writes, so a read-only cell
//! faults); a store, an `atomicrmw` and a `cmpxchg` are `cmpxchg16b`
//! loops. `rbx` is taken for the lowering (`FnCx::take_reg`), `rcx:rbx`
//! is the new value, `rdx:rax` the old one. Without `+cx16`, `llc` calls
//! libatomic's `__atomic_*_16`; `hod_cg::RuntimeFn` has no such member
//! yet, so hod refuses as [`LIBATOMIC`].
//!
//! # Refused
//!
//! [`UNALIGNED`]: an access whose alignment is below its size. [`TYPE`]:
//! `fp128`, `x86_fp80`, vectors, an integer that is not 8, 16, 32, 64 or
//! 128 bits wide, an operation whose type does not fit it.
//! [`BFLOAT_OP`]: arithmetic on `bfloat` (as AArch64). [`LIBATOMIC`]:
//! 16 bytes without `+cx16`. [`FLOAT_OP`] is no longer returned (WP-144).

use hod_cg::{CgResult, Class, FnCx, FrameRef, PReg};
use hod_ir::{
    AtomicOrdering, FnStr, Inst, Intrinsic, MemFlags, RmwOp, SymbolKind, SyncScope, TypeId,
    TypeKind, Value,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Inst as McInst, Mem, Reg, Size, Xmm};

use crate::common::{
    R10, R11, XMM14, XMM15, branch_cond, deferred_first, frame_inst, gp, gpr_load, put,
    unsupported, value_ty, xm,
};
use crate::fpintr::min_max_reg;
use crate::target::X64;

/// The group's refusal before WP-133. Nothing is refused under it any
/// more (`mem.rs` names it for an atomic access the dispatch would send
/// here); kept for the coverage log's history.
pub const ATOMIC: &str = "x64:todo:atomic";
/// An atomic access whose alignment is below its size.
pub const UNALIGNED: &str = "x64:atomic-unaligned";
/// An atomic access of a type hod has no sequence for: `fp128`,
/// `x86_fp80`, a vector, an integer that is not 8, 16, 32, 64 or 128
/// bits wide; an operation whose type does not fit it.
pub const TYPE: &str = "x64:atomic-type";
/// `atomicrmw fadd` and the other floating-point operations on `bfloat`
/// (AArch64's `a64:atomic-bfloat-op`).
pub const BFLOAT_OP: &str = "x64:atomic-bfloat-op";
/// `atomicrmw fmax`, `fmin`, `fmaximum`, `fminimum` of any type, and
/// `fadd`/`fsub` of `half`, before WP-144. Nothing is refused under it
/// any more; kept for the coverage log's history.
pub const FLOAT_OP: &str = "x64:atomic-float-op";
/// A 16-byte atomic in a function without `+cx16`: `llc` calls
/// libatomic's `__atomic_*_16`, which `hod_cg::RuntimeFn` does not have.
pub const LIBATOMIC: &str = "x64:atomic-libatomic";

/// Every refusal of this module.
pub const REASONS: [&str; 5] = [UNALIGNED, TYPE, BFLOAT_OP, FLOAT_OP, LIBATOMIC];

const RAX: Reg = Reg::Rax;
const RBX: Reg = Reg::Rbx;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;

// ---- shapes ----

/// A floating-point format of an atomic value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fp {
    H,
    B,
    S,
    D,
}

/// What an atomic value is, for the sequences.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    /// An integer or pointer of 1, 2, 4 or 8 bytes in a general register.
    Int(Size),
    /// A floating-point value in a vector register.
    Float(Fp),
    /// `i128`, of class `Mem`.
    Wide,
}

impl Fp {
    fn size(self) -> Size {
        match self {
            Fp::H | Fp::B => Size::W,
            Fp::S => Size::D,
            Fp::D => Size::Q,
        }
    }
}

const fn bytes(size: Size) -> u32 {
    match size {
        Size::B => 1,
        Size::W => 2,
        Size::D => 4,
        Size::Q => 8,
    }
}

impl Shape {
    fn bytes(self) -> u64 {
        match self {
            Shape::Int(size) => u64::from(bytes(size)),
            Shape::Float(fp) => u64::from(bytes(fp.size())),
            Shape::Wide => 16,
        }
    }
}

/// The shape of a value of `ty` accessed with `mem`.
fn shape(cx: &FnCx<'_, X64>, ty: TypeId, mem: MemFlags) -> CgResult<Shape> {
    let shape = match cx.types().kind(ty) {
        TypeKind::Int(8) => Shape::Int(Size::B),
        TypeKind::Int(16) => Shape::Int(Size::W),
        TypeKind::Int(32) => Shape::Int(Size::D),
        TypeKind::Int(64) | TypeKind::Ptr { .. } => Shape::Int(Size::Q),
        TypeKind::Int(128) => Shape::Wide,
        TypeKind::Half => Shape::Float(Fp::H),
        TypeKind::BFloat => Shape::Float(Fp::B),
        TypeKind::Float => Shape::Float(Fp::S),
        TypeKind::Double => Shape::Float(Fp::D),
        _ => return unsupported(TYPE),
    };
    if mem.align() < shape.bytes() {
        return unsupported(UNALIGNED);
    }
    Ok(shape)
}

/// Whether the function's `target-features` turn `cx16` on: the last
/// `±cx16` item decides.
pub fn has_cx16(cx: &FnCx<'_, X64>) -> bool {
    let SymbolKind::Function { attrs, .. } = cx.module.symbol(cx.body.symbol()).kind() else {
        return false;
    };
    let Some(features) = cx.module.fn_attrs(attrs).string(FnStr::TargetFeatures) else {
        return false;
    };
    let mut on = false;
    for item in features.split(|&b| b == b',') {
        match item {
            b"+cx16" => on = true,
            b"-cx16" => on = false,
            _ => {}
        }
    }
    on
}

/// The ordering of an atomic access (`seq_cst` if the flags carry none).
fn ordering(mem: MemFlags) -> AtomicOrdering {
    mem.ordering().unwrap_or(AtomicOrdering::SeqCst)
}

/// `value` in a register, or the register of an earlier operand that is
/// the same value (a register value may be asked for once).
fn operand(cx: &mut FnCx<'_, X64>, value: Value, earlier: &[(Value, PReg)]) -> CgResult<PReg> {
    match earlier.iter().find(|(v, _)| *v == value) {
        Some(&(_, reg)) => Ok(reg),
        None => cx.use_reg(value),
    }
}

/// The 32-bit form of an operation on a value of `size`: 8- and 16-bit
/// values compute in 32 bits (the low bits are the same), 64-bit ones in
/// 64.
const fn wide(size: Size) -> Size {
    if matches!(size, Size::Q) { Size::Q } else { Size::D }
}

/// `rd` = the bits of the floating-point value in `x` (zero-extended for
/// 2 bytes).
fn to_gpr(cx: &mut FnCx<'_, X64>, fp: Fp, rd: Reg, x: Xmm) {
    match fp {
        Fp::H | Fp::B => put(cx, mc::pextrw_rri(rd, x, 0)),
        Fp::S => put(cx, mc::movd_rx(Size::D, rd, x)),
        Fp::D => put(cx, mc::movd_rx(Size::Q, rd, x)),
    }
}

/// `xd` = the floating-point value whose bits are in `r` (the lanes
/// above unspecified).
fn from_gpr(cx: &mut FnCx<'_, X64>, fp: Fp, xd: Xmm, r: Reg) {
    match fp {
        Fp::H | Fp::B => put(cx, mc::pinsrw_rri(xd, r, 0)),
        Fp::S => put(cx, mc::movd_xr(Size::D, xd, r)),
        Fp::D => put(cx, mc::movd_xr(Size::Q, xd, r)),
    }
}

/// The memory operand `[a]`.
const fn at(a: Reg) -> Mem {
    Mem::base(a, 0)
}

/// `inst` with a `lock` prefix.
fn locked(inst: McInst) -> McInst {
    mc::lock(inst)
}

/// Takes `rbx` for a 16-byte sequence (before anything else is asked
/// for: at the start of a lowering nothing is pinned) and checks that the
/// function has `+cx16`.
///
/// # Errors
/// [`LIBATOMIC`] without `+cx16`; what displacing `rbx` reports.
fn take_rbx(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    if !has_cx16(cx) {
        return unsupported(LIBATOMIC);
    }
    cx.take_reg(crate::reg::gpr(RBX))
}

/// The two halves of the 16 bytes at frame place `slot` into `lo`, `hi`.
fn load_pair(cx: &mut FnCx<'_, X64>, slot: FrameRef, lo: Reg, hi: Reg) {
    frame_inst(cx, slot, |m| mc::mov_rm(Size::Q, lo, m));
    frame_inst(cx, slot.offset_by(8), |m| mc::mov_rm(Size::Q, hi, m));
}

/// `rdx:rax` to the 16 bytes at frame place `slot`.
fn store_old(cx: &mut FnCx<'_, X64>, slot: FrameRef) {
    frame_inst(cx, slot, |m| mc::mov_mr(Size::Q, m, RAX));
    frame_inst(cx, slot.offset_by(8), |m| mc::mov_mr(Size::Q, m, RDX));
}

/// `again: lock cmpxchg16b [a]; jne again` around what `compute` emits
/// at the top of the loop (`rcx:rbx` from `rdx:rax`).
fn wide_loop(
    cx: &mut FnCx<'_, X64>,
    a: Reg,
    compute: impl FnOnce(&mut FnCx<'_, X64>) -> CgResult<()>,
) -> CgResult<()> {
    let again = cx.new_label();
    cx.bind(again)?;
    compute(cx)?;
    put(cx, locked(mc::cmpxchg16b_m(at(a))));
    branch_cond(cx, Cond::Ne, again)
}

// ---- load, store, fence ----

/// `%r = load atomic <ty>, ptr %ptr <ordering>`.
///
/// # Errors
/// [`UNALIGNED`], [`TYPE`], [`LIBATOMIC`]; what asking for the operands
/// reports.
pub fn load<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    let shape = shape(cx, inst.ty(), mem)?;
    if shape == Shape::Wide {
        take_rbx(cx)?;
    }
    deferred_first(cx, inst, None)?;
    match shape {
        Shape::Int(size) => {
            let a = gp(cx.use_reg(ptr)?);
            let rd = cx.def_reg(inst)?;
            put(cx, gpr_load(gp(rd), at(a), bytes(size)));
            cx.finish(inst, rd)
        }
        Shape::Float(fp) => {
            let a = gp(cx.use_reg(ptr)?);
            let rd = cx.def_reg(inst)?;
            put(cx, gpr_load(RAX, at(a), bytes(fp.size())));
            from_gpr(cx, fp, xm(rd), RAX);
            cx.finish(inst, rd)
        }
        Shape::Wide => {
            // `lock cmpxchg16b` of zero by zero: `rdx:rax` is the value.
            let a = gp(cx.use_reg(ptr)?);
            let r = cx.def_mem(inst)?;
            for reg in [RAX, RDX, RBX, RCX] {
                put(cx, mc::mov_ri(Size::D, reg, 0));
            }
            put(cx, locked(mc::cmpxchg16b_m(at(a))));
            store_old(cx, r);
            Ok(())
        }
    }
}

/// `store atomic <ty> %value, ptr %ptr <ordering>`.
///
/// # Errors
/// [`UNALIGNED`], [`TYPE`], [`LIBATOMIC`]; what asking for the operands
/// reports.
pub fn store<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let shape = shape(cx, value_ty(cx, value), mem)?;
    if shape == Shape::Wide {
        take_rbx(cx)?;
    }
    deferred_first(cx, inst, None)?;
    let seq_cst = ordering(mem) == AtomicOrdering::SeqCst;
    match shape {
        Shape::Int(size) => {
            let a = gp(cx.use_reg(ptr)?);
            cx.put(value, crate::reg::gpr(RAX))?;
            store_rax(cx, a, size, seq_cst);
        }
        Shape::Float(fp) => {
            let a = gp(cx.use_reg(ptr)?);
            let x = xm(cx.use_reg(value)?);
            to_gpr(cx, fp, RAX, x);
            store_rax(cx, a, fp.size(), seq_cst);
        }
        Shape::Wide => {
            // A `cmpxchg16b` loop from a plain read of the cell (any first
            // guess is correct; a good one saves a pass).
            let src = cx.use_mem(value)?;
            let a = gp(cx.use_reg(ptr)?);
            load_pair(cx, src, RBX, RCX);
            put(cx, mc::mov_rm(Size::Q, RAX, at(a)));
            put(cx, mc::mov_rm(Size::Q, RDX, Mem::base(a, 8)));
            wide_loop(cx, a, |_| Ok(()))?;
        }
    }
    Ok(())
}

/// `[a]` = the low `size` bytes of `rax`: `xchg` for `seq_cst`, `mov`
/// otherwise.
fn store_rax(cx: &mut FnCx<'_, X64>, a: Reg, size: Size, seq_cst: bool) {
    if seq_cst {
        put(cx, mc::xchg_mr(size, at(a), RAX));
    } else {
        put(cx, mc::mov_mr(size, at(a), RAX));
    }
}

/// `fence [syncscope("singlethread")] <ordering>`.
///
/// # Errors
/// None.
pub fn fence(cx: &mut FnCx<'_, X64>, ordering: AtomicOrdering, scope: SyncScope) -> CgResult<()> {
    if scope != SyncScope::SingleThread && ordering == AtomicOrdering::SeqCst {
        put(cx, mc::mfence());
    }
    Ok(())
}

// ---- atomicrmw ----

/// `%r = atomicrmw <op> ptr %ptr, <ty> %value <ordering>`.
///
/// # Errors
/// [`UNALIGNED`], [`TYPE`], [`BFLOAT_OP`], [`FLOAT_OP`], [`LIBATOMIC`];
/// what asking for the operands reports.
pub fn rmw<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    op: RmwOp,
    ptr: Value,
    value: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let shape = shape(cx, value_ty(cx, value), mem)?;
    let float_shape = matches!(shape, Shape::Float(_));
    // Anything else is invalid IR (LLVM's verifier).
    if op.is_float() != float_shape && !(op == RmwOp::Xchg && float_shape) {
        return unsupported(TYPE);
    }
    if op.is_float() && shape == Shape::Float(Fp::B) {
        return unsupported(BFLOAT_OP);
    }
    if shape == Shape::Wide {
        take_rbx(cx)?;
    }
    deferred_first(cx, inst, None)?;
    match shape {
        Shape::Wide => rmw_wide(cx, inst, op, ptr, value),
        Shape::Float(fp) => rmw_float(cx, inst, op, fp, ptr, value),
        Shape::Int(size) => rmw_int(cx, inst, op, size, ptr, value),
    }
}

/// An integer `atomicrmw` of up to 8 bytes.
#[allow(clippy::too_many_lines, reason = "one arm per operation")]
#[allow(clippy::many_single_char_names, reason = "register names")]
fn rmw_int<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    op: RmwOp,
    size: Size,
    ptr: Value,
    value: Value,
) -> CgResult<()> {
    let pa = cx.use_reg(ptr)?;
    let pv = operand(cx, value, &[(ptr, pa)])?;
    let (a, v) = (gp(pa), gp(pv));
    let dead = cx.is_dead(inst);
    // The result's register; `rax` when the result has no use.
    let rd = if dead { None } else { Some(cx.def_reg(inst)?) };
    let out = rd.map_or(RAX, gp);
    let m = at(a);
    let w = wide(size);
    match op {
        RmwOp::Add | RmwOp::Sub | RmwOp::And | RmwOp::Or | RmwOp::Xor if dead => {
            let form = match op {
                RmwOp::Add => mc::add_mr,
                RmwOp::Sub => mc::sub_mr,
                RmwOp::And => mc::and_mr,
                RmwOp::Or => mc::or_mr,
                _ => mc::xor_mr,
            };
            put(cx, locked(form(size, m, v)));
            return Ok(());
        }
        RmwOp::Xchg => {
            put(cx, mc::mov_rr(Size::Q, out, v));
            put(cx, mc::xchg_mr(size, m, out));
        }
        RmwOp::Add | RmwOp::Sub => {
            put(cx, mc::mov_rr(Size::Q, out, v));
            if op == RmwOp::Sub {
                put(cx, mc::neg_r(Size::Q, out));
            }
            put(cx, locked(mc::xadd_mr(size, m, out)));
        }
        _ => {
            // The `lock cmpxchg` loop: old in `rax`, new in `r10`.
            put(cx, gpr_load(RAX, m, bytes(size)));
            let again = cx.new_label();
            cx.bind(again)?;
            let n = R10;
            match op {
                RmwOp::And | RmwOp::Or | RmwOp::Xor | RmwOp::Nand => {
                    put(cx, mc::mov_rr(Size::Q, n, RAX));
                    let f = match op {
                        RmwOp::Or => mc::or_rr,
                        RmwOp::Xor => mc::xor_rr,
                        _ => mc::and_rr,
                    };
                    put(cx, f(w, n, v));
                    if op == RmwOp::Nand {
                        put(cx, mc::not_r(w, n));
                    }
                }
                RmwOp::Max | RmwOp::Min | RmwOp::UMax | RmwOp::UMin => {
                    // `v` replaces the old value when it wins.
                    let cond = match op {
                        RmwOp::Max => Cond::L,
                        RmwOp::Min => Cond::G,
                        RmwOp::UMax => Cond::B,
                        _ => Cond::A,
                    };
                    put(cx, mc::mov_rr(Size::Q, n, RAX));
                    put(cx, mc::cmp_rr(size, n, v));
                    put(cx, mc::cmovcc_rr(cond, w, n, v));
                }
                RmwOp::UIncWrap => {
                    // old >= v ? 0 : old + 1
                    put(cx, mc::lea(Size::Q, n, Mem::base(RAX, 1)));
                    put(cx, mc::cmp_rr(size, RAX, v));
                    put(cx, mc::mov_ri(Size::D, RCX, 0));
                    put(cx, mc::cmovcc_rr(Cond::Ae, w, n, RCX));
                }
                RmwOp::UDecWrap => {
                    // old == 0 || old > v ? v : old - 1
                    put(cx, mc::lea(Size::Q, n, Mem::base(RAX, -1)));
                    put(cx, mc::cmp_rr(size, RAX, v));
                    put(cx, mc::cmovcc_rr(Cond::A, w, n, v));
                    put(cx, mc::test_rr(size, RAX, RAX));
                    put(cx, mc::cmovcc_rr(Cond::E, w, n, v));
                }
                RmwOp::USubCond | RmwOp::USubSat => {
                    // old >= v ? old - v : old (cond) or 0 (sat)
                    put(cx, mc::mov_rr(Size::Q, n, RAX));
                    put(cx, mc::sub_rr(size, n, v));
                    let keep = if op == RmwOp::USubCond {
                        RAX
                    } else {
                        put(cx, mc::mov_ri(Size::D, RCX, 0));
                        RCX
                    };
                    put(cx, mc::cmovcc_rr(Cond::B, w, n, keep));
                }
                _ => return cx.bug("an atomicrmw operation rmw refused"),
            }
            put(cx, locked(mc::cmpxchg_mr(size, m, n)));
            branch_cond(cx, Cond::Ne, again)?;
            if rd.is_some() {
                put(cx, mc::mov_rr(Size::Q, out, RAX));
            }
        }
    }
    match rd {
        Some(rd) => cx.finish(inst, rd),
        None => Ok(()),
    }
}

/// A floating-point `atomicrmw` of `half`, `float` or `double`: `xchg`
/// of the bits, or a `lock cmpxchg` loop around the operation (§5.6.14):
///
/// - `fadd`/`fsub` of `float`/`double` in `xmm15`;
/// - `fmax`/`fmin`/`fmaximum`/`fminimum` as `llvm.maxnum`/`minnum`/
///   `maximum`/`minimum` (WP-127's [`min_max_reg`]) on a temporary,
///   the old value's bits kept in `r11` (the sequence writes `rax`);
/// - `half`: in `float`, as `llc` computes it (`__extendhfsf2`, the
///   operation, `__truncsfhf2`), the two conversions inline
///   ([`half_to_float`], [`float_to_half`]: a call cannot sit inside the
///   loop, it displaces the registers the loop holds), the operand
///   widened once before the loop in its own register, whose `half` bits
///   come back after it.
#[allow(clippy::many_single_char_names, reason = "register names")]
fn rmw_float<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    op: RmwOp,
    fp: Fp,
    ptr: Value,
    value: Value,
) -> CgResult<()> {
    let a = gp(cx.use_reg(ptr)?);
    let x = xm(cx.use_reg(value)?);
    let min_max = match op {
        RmwOp::FMax => Some(Intrinsic::MaxNum),
        RmwOp::FMin => Some(Intrinsic::MinNum),
        RmwOp::FMaximum => Some(Intrinsic::Maximum),
        RmwOp::FMinimum => Some(Intrinsic::Minimum),
        _ => None,
    };
    // The operation's left operand (the old value), apart from `x`.
    let tp = if op == RmwOp::Xchg { None } else { Some(cx.temp(Class::Fpr)?) };
    let t = tp.map(xm);
    // `half`: `x` holds the operand in `float` during the loop (no third
    // vector register: the smallest pool has two), its own bits kept in
    // `g` and put back after the loop (the lanes above a `half` are
    // unspecified).
    let g = if fp == Fp::H && t.is_some() {
        let g = gp(cx.temp(Class::Gpr)?);
        put(cx, mc::pextrw_rri(g, x, 0));
        put(cx, mc::mov_rr(Size::D, RCX, g));
        half_to_float(cx, x)?;
        Some(g)
    } else {
        None
    };
    let size = fp.size();
    let m = at(a);
    if let Some(t) = t {
        let fsz = if fp == Fp::D { FpSize::D } else { FpSize::S };
        put(cx, gpr_load(RAX, m, bytes(size)));
        let again = cx.new_label();
        cx.bind(again)?;
        if fp == Fp::H {
            put(cx, mc::mov_rr(Size::D, RCX, RAX));
            half_to_float(cx, t)?;
        } else {
            from_gpr(cx, fp, t, RAX);
        }
        match min_max {
            Some(which) => {
                put(cx, mc::mov_rr(Size::Q, R11, RAX));
                min_max_reg(cx, which, fsz, t, x)?;
                put(cx, mc::mov_rr(Size::Q, RAX, R11));
            }
            None if op == RmwOp::FAdd => put(cx, mc::adds_rr(fsz, t, x)),
            None => put(cx, mc::subs_rr(fsz, t, x)),
        }
        if fp == Fp::H {
            float_to_half(cx, R10, t)?;
        } else {
            to_gpr(cx, fp, R10, t);
        }
        put(cx, locked(mc::cmpxchg_mr(size, m, R10)));
        branch_cond(cx, Cond::Ne, again)?;
        if let Some(g) = g {
            put(cx, mc::pinsrw_rri(x, g, 0));
        }
    } else {
        to_gpr(cx, fp, RAX, x);
        put(cx, mc::xchg_mr(size, m, RAX));
    }
    // The result's register is asked for after the loop (it may displace
    // a value), the temporary given back first: the smallest pool has
    // two vector registers.
    if let Some(tp) = tp {
        cx.free_temp(tp);
    }
    let rd = if cx.is_dead(inst) { None } else { Some(cx.def_reg(inst)?) };
    match rd {
        Some(rd) => {
            from_gpr(cx, fp, xm(rd), RAX);
            cx.finish(inst, rd)
        }
        None => Ok(()),
    }
}

/// `xd` = the `half` whose bits are in the low 16 bits of `rcx`, widened
/// to `float` exactly as libgcc's `__extendhfsf2` does: a normal value or
/// an infinity rebiased, a NaN quieted with its payload shifted up, a
/// subnormal one through `float` arithmetic that is exact. Writes `rcx`,
/// `rdx`, `xmm14` and the flags; `xd` is not `xmm14`.
///
/// # Errors
/// What binding a label reports.
pub fn half_to_float(cx: &mut FnCx<'_, X64>, xd: Xmm) -> CgResult<()> {
    let d = Size::D;
    let (special, small, done) = (cx.new_label(), cx.new_label(), cx.new_label());
    put(cx, mc::mov_rr(d, RDX, RCX));
    put(cx, mc::and_ri(d, RDX, 0x8000));
    put(cx, mc::shl_ri(d, RDX, 16));
    put(cx, mc::and_ri(d, RCX, 0x7fff));
    put(cx, mc::shl_ri(d, RCX, 13));
    put(cx, mc::cmp_ri(d, RCX, 0x7c00 << 13));
    branch_cond(cx, Cond::Ae, special)?;
    put(cx, mc::cmp_ri(d, RCX, 0x400 << 13));
    branch_cond(cx, Cond::B, small)?;
    // Normal: the exponent rebiased by 127 - 15.
    put(cx, mc::add_ri(d, RCX, 0x3800_0000));
    crate::common::branch(cx, done)?;
    // An infinity, or a NaN quieted (libgcc's routine quiets it; an
    // `addss` of a signalling NaN would pick another payload).
    cx.bind(special)?;
    put(cx, mc::add_ri(d, RCX, 0x7000_0000));
    put(cx, mc::test_ri(d, RCX, 0x7f_ffff));
    branch_cond(cx, Cond::E, done)?;
    put(cx, mc::or_ri(d, RCX, 0x40_0000));
    crate::common::branch(cx, done)?;
    // Subnormal (or zero): `2^-14 * (1 + m/1024) - 2^-14`, exact.
    cx.bind(small)?;
    put(cx, mc::add_ri(d, RCX, 0x3880_0000));
    put(cx, mc::movd_xr(d, XMM14, RCX));
    put(cx, mc::mov_ri(d, RCX, 0x3880_0000));
    put(cx, mc::movd_xr(d, xd, RCX));
    put(cx, mc::subs_rr(FpSize::S, XMM14, xd));
    put(cx, mc::movd_rx(d, RCX, XMM14));
    cx.bind(done)?;
    put(cx, mc::or_rr(d, RCX, RDX));
    put(cx, mc::movd_xr(d, xd, RCX));
    Ok(())
}

/// `rd` = the `float` in `x` narrowed to `half` (zero-extended), as
/// `__truncsfhf2` does: rounded to nearest even, an overflow to an
/// infinity, a subnormal result rounded by one `addss` of 0.5 (whose
/// ulp is the subnormal `half`'s), a NaN quieted with the top of its
/// payload. Writes `rcx`, `rdx`, `xmm14`, `xmm15` and the flags; `rd`
/// is none of them (`x` may be `xmm15`).
///
/// # Errors
/// What binding a label reports.
pub fn float_to_half(cx: &mut FnCx<'_, X64>, rd: Reg, x: Xmm) -> CgResult<()> {
    let d = Size::D;
    let (nan, inf, small, done) = (cx.new_label(), cx.new_label(), cx.new_label(), cx.new_label());
    put(cx, mc::movd_rx(d, RCX, x));
    put(cx, mc::mov_rr(d, RDX, RCX));
    put(cx, mc::shr_ri(d, RDX, 16));
    put(cx, mc::and_ri(d, RDX, 0x8000));
    put(cx, mc::and_ri(d, RCX, 0x7fff_ffff));
    put(cx, mc::cmp_ri(d, RCX, 0x7f80_0000));
    branch_cond(cx, Cond::A, nan)?;
    // 65520 and above round to infinity.
    put(cx, mc::cmp_ri(d, RCX, 0x477f_f000));
    branch_cond(cx, Cond::Ae, inf)?;
    put(cx, mc::cmp_ri(d, RCX, 0x3880_0000));
    branch_cond(cx, Cond::B, small)?;
    // Normal: add 0xfff and the kept bit's parity, rebias, shift.
    put(cx, mc::mov_rr(d, rd, RCX));
    put(cx, mc::shr_ri(d, rd, 13));
    put(cx, mc::and_ri(d, rd, 1));
    put(cx, mc::add_rr(d, RCX, rd));
    put(cx, mc::add_ri(d, RCX, 0xfff - 0x3800_0000));
    put(cx, mc::shr_ri(d, RCX, 13));
    put(cx, mc::mov_rr(d, rd, RCX));
    crate::common::branch(cx, done)?;
    cx.bind(nan)?;
    put(cx, mc::shr_ri(d, RCX, 13));
    put(cx, mc::and_ri(d, RCX, 0x1ff));
    put(cx, mc::or_ri(d, RCX, 0x7e00));
    put(cx, mc::mov_rr(d, rd, RCX));
    crate::common::branch(cx, done)?;
    cx.bind(inf)?;
    put(cx, mc::mov_ri(d, rd, 0x7c00));
    crate::common::branch(cx, done)?;
    cx.bind(small)?;
    put(cx, mc::movd_xr(d, XMM14, RCX));
    put(cx, mc::mov_ri(d, RCX, 0x3f00_0000));
    put(cx, mc::movd_xr(d, XMM15, RCX));
    put(cx, mc::adds_rr(FpSize::S, XMM14, XMM15));
    put(cx, mc::movd_rx(d, rd, XMM14));
    put(cx, mc::sub_ri(d, rd, 0x3f00_0000));
    cx.bind(done)?;
    put(cx, mc::or_rr(d, rd, RDX));
    Ok(())
}

/// An `i128` `atomicrmw` as a `lock cmpxchg16b` loop: the operand in two
/// temporaries `t1` (low) and `t2` (high), the old value in `rdx:rax`,
/// the new one computed in `rcx:rbx` (with `r10` as scratch).
#[allow(clippy::many_single_char_names, reason = "register names")]
fn rmw_wide<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    op: RmwOp,
    ptr: Value,
    value: Value,
) -> CgResult<()> {
    let src = cx.use_mem(value)?;
    let a = gp(cx.use_reg(ptr)?);
    let t1 = gp(cx.temp(Class::Gpr)?);
    let t2 = gp(cx.temp(Class::Gpr)?);
    let r = if cx.is_dead(inst) { None } else { Some(cx.def_mem(inst)?) };
    load_pair(cx, src, t1, t2);
    put(cx, mc::mov_rm(Size::Q, RAX, at(a)));
    put(cx, mc::mov_rm(Size::Q, RDX, Mem::base(a, 8)));
    let q = Size::Q;
    wide_loop(cx, a, |cx| {
        let pair = |cx: &mut FnCx<'_, X64>,
                    lo: fn(Size, Reg, Reg) -> McInst,
                    hi: fn(Size, Reg, Reg) -> McInst| {
            put(cx, mc::mov_rr(q, RBX, RAX));
            put(cx, lo(q, RBX, t1));
            put(cx, mc::mov_rr(q, RCX, RDX));
            put(cx, hi(q, RCX, t2));
        };
        // The flags of `old - v` (`b`/`ae`, `l`/`ge`; not `e`).
        let compare = |cx: &mut FnCx<'_, X64>| {
            put(cx, mc::cmp_rr(q, RAX, t1));
            put(cx, mc::mov_rr(q, R10, RDX));
            put(cx, mc::sbb_rr(q, R10, t2));
        };
        let select = |cx: &mut FnCx<'_, X64>, cond: Cond, lo: Reg, hi: Reg| {
            put(cx, mc::cmovcc_rr(cond, q, RBX, lo));
            put(cx, mc::cmovcc_rr(cond, q, RCX, hi));
        };
        match op {
            RmwOp::Xchg => {
                put(cx, mc::mov_rr(q, RBX, t1));
                put(cx, mc::mov_rr(q, RCX, t2));
            }
            RmwOp::Add => pair(cx, mc::add_rr, mc::adc_rr),
            RmwOp::Sub => pair(cx, mc::sub_rr, mc::sbb_rr),
            RmwOp::And => pair(cx, mc::and_rr, mc::and_rr),
            RmwOp::Or => pair(cx, mc::or_rr, mc::or_rr),
            RmwOp::Xor => pair(cx, mc::xor_rr, mc::xor_rr),
            RmwOp::Nand => {
                pair(cx, mc::and_rr, mc::and_rr);
                put(cx, mc::not_r(q, RBX));
                put(cx, mc::not_r(q, RCX));
            }
            RmwOp::Max | RmwOp::Min | RmwOp::UMax | RmwOp::UMin => {
                // `v` replaces the old value when it wins.
                let cond = match op {
                    RmwOp::Max => Cond::L,
                    RmwOp::Min => Cond::Ge,
                    RmwOp::UMax => Cond::B,
                    _ => Cond::Ae,
                };
                compare(cx);
                put(cx, mc::mov_rr(q, RBX, RAX));
                put(cx, mc::mov_rr(q, RCX, RDX));
                select(cx, cond, t1, t2);
            }
            RmwOp::UIncWrap => {
                // old >= v ? 0 : old + 1
                put(cx, mc::mov_rr(q, RBX, RAX));
                put(cx, mc::add_ri(q, RBX, 1));
                put(cx, mc::mov_rr(q, RCX, RDX));
                put(cx, mc::adc_ri(q, RCX, 0));
                compare(cx);
                put(cx, mc::mov_ri(Size::D, R10, 0));
                select(cx, Cond::Ae, R10, R10);
            }
            RmwOp::UDecWrap => {
                // old == 0 || old > v ? v : old - 1
                put(cx, mc::mov_rr(q, RBX, RAX));
                put(cx, mc::sub_ri(q, RBX, 1));
                put(cx, mc::mov_rr(q, RCX, RDX));
                put(cx, mc::sbb_ri(q, RCX, 0));
                // `v - old` borrows: `v < old`.
                put(cx, mc::cmp_rr(q, t1, RAX));
                put(cx, mc::mov_rr(q, R10, t2));
                put(cx, mc::sbb_rr(q, R10, RDX));
                select(cx, Cond::B, t1, t2);
                put(cx, mc::mov_rr(q, R10, RAX));
                put(cx, mc::or_rr(q, R10, RDX));
                select(cx, Cond::E, t1, t2);
            }
            RmwOp::USubCond | RmwOp::USubSat => {
                pair(cx, mc::sub_rr, mc::sbb_rr);
                if op == RmwOp::USubCond {
                    select(cx, Cond::B, RAX, RDX);
                } else {
                    put(cx, mc::mov_ri(Size::D, R10, 0));
                    select(cx, Cond::B, R10, R10);
                }
            }
            _ => return cx.bug("a floating-point atomicrmw of i128"),
        }
        Ok(())
    })?;
    if let Some(r) = r {
        store_old(cx, r);
    }
    Ok(())
}

// ---- cmpxchg ----

/// `%r = cmpxchg [weak] ptr %ptr, <ty> %expected, <ty> %replacement
/// <success> <failure>`: the `{ <ty>, i1 }` result in its slot.
///
/// # Errors
/// [`UNALIGNED`], [`TYPE`], [`LIBATOMIC`]; what asking for the operands
/// reports.
#[allow(
    clippy::too_many_arguments,
    reason = "the instruction's operands, as the dispatch has them"
)]
pub fn cmpxchg<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    ptr: Value,
    expected: Value,
    replacement: Value,
    mem: MemFlags,
    failure: AtomicOrdering,
    weak: bool,
) -> CgResult<()> {
    // Every ordering pair is the same locked instruction; a weak
    // `cmpxchg` is a strong one.
    let _ = (failure, weak);
    let shape = shape(cx, value_ty(cx, expected), mem)?;
    let Ok(flag_at) = u32::try_from(cx.types().field_offset(inst.ty(), 1)) else {
        return unsupported(hod_cg::LIMIT_FRAME_SIZE);
    };
    let size = match shape {
        Shape::Int(size) => size,
        Shape::Wide => {
            take_rbx(cx)?;
            deferred_first(cx, inst, None)?;
            let e = cx.use_mem(expected)?;
            let n = cx.use_mem(replacement)?;
            let a = gp(cx.use_reg(ptr)?);
            let r = cx.def_mem(inst)?;
            load_pair(cx, e, RAX, RDX);
            load_pair(cx, n, RBX, RCX);
            put(cx, locked(mc::cmpxchg16b_m(at(a))));
            put(cx, mc::setcc_r(Cond::E, R10));
            store_old(cx, r);
            frame_inst(cx, r.offset_by(flag_at), |m| mc::mov_mr(Size::B, m, R10));
            return Ok(());
        }
        // LLVM's verifier allows only integers and pointers.
        Shape::Float(_) => return unsupported(TYPE),
    };
    deferred_first(cx, inst, None)?;
    let pa = cx.use_reg(ptr)?;
    let pn = operand(cx, replacement, &[(ptr, pa)])?;
    let r = cx.def_mem(inst)?;
    cx.put(expected, crate::reg::gpr(RAX))?;
    put(cx, locked(mc::cmpxchg_mr(size, at(gp(pa)), gp(pn))));
    put(cx, mc::setcc_r(Cond::E, R10));
    frame_inst(cx, r, |m| mc::mov_mr(size, m, RAX));
    frame_inst(cx, r.offset_by(flag_at), |m| mc::mov_mr(Size::B, m, R10));
    Ok(())
}
