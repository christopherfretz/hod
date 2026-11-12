//! The short tail of first refusals (WP-084, `spec/codegen.md` §5.5.1
//! "WP-084"): constructs each of which stopped a handful of modules.
//! Each is reached by one call from the module that refused it before.
//!
//! - Conversions between floating point and integers of 65 to 128 bits
//!   ([`fp_int_cast`], from `aggr::cast`): the calls `llc` makes,
//!   `__fix[uns]{h,s,d,t}fti` and `__float[un]ti{s,d,t}f`; to `half`
//!   through `float` (two roundings, as `llc`). An operand narrower than
//!   128 bits is extended into a 16-byte slot first. Wider integers and
//!   `bfloat` stay refused (`aggr::WIDE_FP_CAST`).
//! - `llvm.{s,u}mul.with.overflow` wider than 64 bits
//!   ([`mul_overflow_wide`], from `aggr::overflow_intrinsic`): the
//!   operands extended to twice the width, multiplied exactly, and the
//!   flag read from the product's upper half.
//! - `switch` on an integer of class `Mem` ([`switch_wide`], from
//!   `control::switch`): one compare chain per case, in the IR's order.
//! - `llvm.readcyclecounter` ([`read_cycle_counter`]): `mrs CNTVCT_EL0`.
//! - `llvm.is.constant` ([`is_manifest`]): `llc`'s `isManifestConstant`,
//!   aggregates of manifest constants included.
//! - `llvm.{s,u}shl.sat` and `llvm.aarch64.crc32*` ([`intrinsic`]).
//! - In a function that calls `llvm.is.constant` or `llvm.objectsize`,
//!   the instructions `llc` deletes once those calls are constants are
//!   not emitted ([`begin`], [`dropped`]).

use std::cell::RefCell;

use hod_cg::{CallArg, CgResult, Class, FnCx, RuntimeFn};
use hod_ir::{
    Body, CallSite, Const, ConstId, Inst, InstKind, Intrinsic, Opcode, Switch, TypeId, TypeKind,
    Value, ValueKind,
};
use hod_mc_a64 as mc;
use hod_mc_a64::{Cond, Crc32, FpFormat, MemSize, RegZr, Size, SysReg};

use crate::aggr::{
    MAX_QUADRATIC_BITS, WIDE_FP_CAST, WIDE_MUL_OVERFLOW, at, extend, ld, ld_ext, limbs, multiply,
    st, top_bits,
};
use crate::common::{
    Cursor, X16, X17, branch_cond, deferred_first, mov_imm, put, store_gpr_at, unsupported,
    value_ty, vr, xr,
};
use crate::target::A64;
use crate::{intrinsic as intr, term};

// ---- conversions ----

/// `fptosi`, `fptoui`, `sitofp`, `uitofp` between a floating-point type
/// and an integer of 65 to 128 bits (`deferred_first` is done).
///
/// # Errors
/// [`WIDE_FP_CAST`] for a wider integer or `bfloat`; what the operand
/// and the runtime call report.
pub fn fp_int_cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let op = inst.opcode();
    let to_int = matches!(op, Opcode::FPToSI | Opcode::FPToUI);
    let signed = matches!(op, Opcode::FPToSI | Opcode::SIToFP);
    let (int_ty, fp_ty) =
        if to_int { (inst.ty(), value_ty(cx, arg)) } else { (value_ty(cx, arg), inst.ty()) };
    let bits = match cx.types().int_bits(int_ty) {
        Some(bits) if (65..=128).contains(&bits) => bits,
        _ => return unsupported(WIDE_FP_CAST),
    };
    // 0 half, 1 float, 2 double, 3 fp128.
    let fp = match cx.types().kind(fp_ty) {
        TypeKind::Half => 0,
        TypeKind::Float => 1,
        TypeKind::Double => 2,
        TypeKind::Fp128 => 3,
        _ => return unsupported(WIDE_FP_CAST),
    };
    if to_int {
        let func = match (signed, fp) {
            (true, 0) => RuntimeFn::FixHfTi,
            (true, 1) => RuntimeFn::FixSfTi,
            (true, 2) => RuntimeFn::FixDfTi,
            (true, _) => RuntimeFn::FixTfTi,
            (false, 0) => RuntimeFn::FixunsHfTi,
            (false, 1) => RuntimeFn::FixunsSfTi,
            (false, 2) => RuntimeFn::FixunsDfTi,
            (false, _) => RuntimeFn::FixunsTfTi,
        };
        // The bits of a narrower result above its width are unspecified
        // (§2.2): the 128-bit result is its home as it is.
        let r = cx.def_mem(inst)?;
        cx.call_runtime(func, &[CallArg::Value(arg)], Some(r))?;
        return Ok(());
    }
    let func = match (signed, fp) {
        (true, 0 | 1) => RuntimeFn::FloatTiSf,
        (true, 2) => RuntimeFn::FloatTiDf,
        (true, _) => RuntimeFn::FloatTiTf,
        (false, 0 | 1) => RuntimeFn::FloatunTiSf,
        (false, 2) => RuntimeFn::FloatunTiDf,
        (false, _) => RuntimeFn::FloatunTiTf,
    };
    let slot = cx.use_mem(arg)?;
    let src = if bits == 128 {
        slot
    } else {
        let wide = cx.alloc_slot(16, 16)?;
        ld(cx, X16, slot, 0);
        st(cx, X16.zr(), X17, wide, 0);
        ld_ext(cx, X16, slot, bits, 1, signed);
        st(cx, X16.zr(), X17, wide, 1);
        wide
    };
    let Some(reg) = cx.call_runtime(func, &[CallArg::Mem(src)], None)? else {
        return cx.bug("a runtime function returned nothing");
    };
    if fp == 0 {
        // As `llc`: the `float` result rounded again to `half`.
        put(cx, mc::fcvt(FpFormat::H, vr(reg), FpFormat::S, vr(reg)));
    }
    cx.finish(inst, reg)
}

// ---- multiplication with overflow ----

/// `llvm.{s,u}mul.with.overflow` of `bits` > 64 bits (`deferred_first`
/// is done): the product of the operands extended to `2 * bits` is
/// exact; the unsigned flag is any bit of it at or above `bits`, the
/// signed one any such bit that differs from bit `bits - 1`. The result
/// `{iN, i1}` is written as `aggr::overflow_intrinsic` writes it.
///
/// # Errors
/// [`WIDE_MUL_OVERFLOW`] beyond half of [`MAX_QUADRATIC_BITS`]; what the
/// operands report.
#[allow(
    clippy::many_single_char_names,
    reason = "operands a, b; product p; result r; scratch x, y"
)]
pub fn mul_overflow_wide<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    bits: u32,
    [lhs, rhs]: [Value; 2],
    flag_at: u32,
) -> CgResult<()> {
    if bits > MAX_QUADRATIC_BITS / 2 {
        return unsupported(WIDE_MUL_OVERFLOW);
    }
    let signed = which == Intrinsic::SmulWithOverflow;
    let wide = 2 * bits;
    let n = limbs(wide);
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let r = cx.def_mem(inst)?;
    let ea = cx.alloc_slot(u64::from(8 * n), 16)?;
    let eb = cx.alloc_slot(u64::from(8 * n), 16)?;
    let p = cx.alloc_slot(u64::from(8 * n), 16)?;
    for limb in 0..n {
        ld_ext(cx, X16, a, bits, limb, signed);
        st(cx, X16.zr(), X17, ea, limb);
        ld_ext(cx, X16, b, bits, limb, signed);
        st(cx, X16.zr(), X17, eb, limb);
    }
    multiply(cx, wide, p, ea, eb)?;
    let (x, y) = (X16.zr(), X17.zr());
    let flag = xr(cx.temp(Class::Gpr)?);
    let (lb, off) = (bits / 64, bits % 64);
    ld(cx, X16, p, lb);
    if signed {
        // `flag` = bit `bits - 1` everywhere.
        ld(cx, flag, p, (bits - 1) / 64);
        put(cx, mc::sbfx(Size::X, flag.zr(), flag.zr(), (bits - 1) % 64, 1));
        if off != 0 {
            put(cx, mc::asr_imm(Size::X, x, x, off));
        }
        put(cx, mc::eor(Size::X, x, x, flag.zr()));
        for k in lb + 1..n {
            ld(cx, X17, p, k);
            put(cx, mc::eor(Size::X, y, y, flag.zr()));
            put(cx, mc::orr(Size::X, x, x, y));
        }
    } else {
        if off != 0 {
            put(cx, mc::lsr_imm(Size::X, x, x, off));
        }
        for k in lb + 1..n {
            ld(cx, X17, p, k);
            put(cx, mc::orr(Size::X, x, x, y));
        }
    }
    put(cx, mc::cmp(Size::X, x, RegZr::ZR));
    put(cx, mc::cset(Size::W, flag.zr(), Cond::Ne));
    let m = limbs(bits);
    for limb in 0..m {
        ld(cx, X16, p, limb);
        if limb + 1 == m {
            extend(cx, X16, top_bits(bits), false);
        }
        st(cx, x, X17, r, limb);
    }
    let mut to = Cursor::frame(cx, at(r, flag_at), X17);
    store_gpr_at(cx, MemSize::B, flag.zr(), &mut to);
    Ok(())
}

// ---- switch ----

/// Limb `k` of the integer constant `id`; `None` if it is no integer.
fn const_limb(cx: &FnCx<'_, A64>, id: ConstId, k: u32) -> Option<u64> {
    match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(if k == 0 { value } else { 0 }),
        Const::WideInt { words, .. } => Some(words.get(k as usize).copied().unwrap_or(0)),
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// `switch` on an integer of class `Mem` (`deferred_first` is done): for
/// each case not to the default, in the IR's order (of two equal cases
/// the first wins, as in the interpreter), the limbs compared by `cmp`
/// and `ccmp` and a `b.eq`; then the default.
///
/// # Errors
/// What the condition and the branches' fixups report.
pub fn switch_wide<'a>(cx: &mut FnCx<'a, A64>, switch: &Switch<'a>) -> CgResult<()> {
    let Some(bits) = cx.types().int_bits(value_ty(cx, switch.cond)) else {
        return cx.bug("a switch on something that is no integer");
    };
    let n = limbs(bits);
    if let ValueKind::Const(id) = switch.cond.kind()
        && const_limb(cx, id, 0).is_some()
    {
        let same = |cx: &FnCx<'a, A64>, case| {
            (0..n).all(|k| const_limb(cx, case, k) == const_limb(cx, id, k))
        };
        let target =
            switch.cases().find(|&(case, _)| same(cx, case)).map_or(switch.default, |(_, b)| b);
        return term::br(cx, target);
    }
    let c = cx.use_mem(switch.cond)?;
    let (x, y) = (X16.zr(), X17.zr());
    for (case, block) in switch.cases() {
        if block == switch.default {
            continue;
        }
        for k in 0..n {
            let Some(word) = const_limb(cx, case, k) else {
                return cx.bug("a switch case that is no integer");
            };
            ld_ext(cx, X16, c, bits, k, false);
            mov_imm(cx, Size::X, X17, word);
            put(
                cx,
                if k == 0 { mc::cmp(Size::X, x, y) } else { mc::ccmp(Size::X, x, y, 0, Cond::Eq) },
            );
        }
        branch_cond(cx, Cond::Eq, cx.block_label(block))?;
    }
    term::br(cx, switch.default)
}

// ---- intrinsics ----

/// `llvm.readcyclecounter`: `mrs CNTVCT_EL0`, as `llc`.
///
/// # Errors
/// What placing the result reports.
pub fn read_cycle_counter<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let rd = cx.def_reg(inst)?;
    put(cx, mc::mrs(xr(rd).zr(), SysReg::CntvctEl0));
    cx.finish(inst, rd)
}

/// Whether `llvm.is.constant` of `value` is true, as `llc`'s
/// `isManifestConstant` decides it: a constant that is data (a number,
/// `null`, a zero, `undef`, `poison`, `none`, a string of bytes), or an
/// aggregate, splat or constant expression all of whose operands are.
/// The address of a symbol or block, and so anything built on one, is
/// not.
pub fn is_manifest(cx: &FnCx<'_, A64>, value: Value) -> bool {
    let Some(id) = value.as_const() else { return false };
    let mut pending: Vec<ConstId> = Vec::new();
    let mut next = Some(id);
    while let Some(id) = next {
        match cx.module.consts.get(id) {
            Const::Int { .. }
            | Const::WideInt { .. }
            | Const::Float { .. }
            | Const::WideFloat { .. }
            | Const::Null(_)
            | Const::Zero(_)
            | Const::Undef(_)
            | Const::Poison(_)
            | Const::NoneToken
            | Const::Bytes { .. }
            // `inttoptr` of an integer: an expression of data.
            | Const::AddrOfInt { .. } => {}
            Const::Aggregate { elems: args, .. } | Const::Expr { args, .. } => {
                pending.extend_from_slice(args);
            }
            Const::Splat { elem, .. } => pending.push(elem),
            _ => return false,
        }
        next = pending.pop();
    }
    true
}

/// `llvm.sshl.sat`, `llvm.ushl.sat` (at most 64 bits) and
/// `llvm.aarch64.crc32*`.
///
/// # Errors
/// `intrinsic::WIDE` and `intrinsic::VECTOR` for a saturating shift of
/// another type; what the operands report.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    let &[a, b] = args else {
        return cx.bug("a two-operand intrinsic without two operands");
    };
    let kind = match which {
        I::SshlSat | I::UshlSat => return shl_sat(cx, inst, which == I::SshlSat, a, b),
        I::Aarch64Crc32b => Crc32::B,
        I::Aarch64Crc32h => Crc32::H,
        I::Aarch64Crc32w => Crc32::W,
        I::Aarch64Crc32x => Crc32::X,
        I::Aarch64Crc32cb => Crc32::Cb,
        I::Aarch64Crc32ch => Crc32::Ch,
        I::Aarch64Crc32cw => Crc32::Cw,
        I::Aarch64Crc32cx => Crc32::Cx,
        _ => return unsupported(intr::INTRINSIC),
    };
    // `crc32{,c}{b,h}` read only the low bits of the data register.
    deferred_first(cx, inst, None)?;
    let pa = cx.use_reg(a)?;
    let rb = xr(cx.use_reg(b)?);
    put(cx, mc::crc32(kind, xr(pa).zr(), xr(pa).zr(), rb.zr()));
    cx.finish(inst, pa)
}

/// `{s,u}shl.sat` of `bits` <= 64 bits: the operand moved to the top of
/// the register, shifted, shifted back and compared; on a difference the
/// saturated value (all ones, or the extreme of the operand's sign);
/// then moved back down. An amount of at least `bits` is poison.
#[allow(
    clippy::many_single_char_names,
    reason = "operands a, b; their registers r, k; scratch x, y"
)]
fn shl_sat<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    signed: bool,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let bits = match cx.types().kind(inst.ty()) {
        TypeKind::Int(bits) if (1..=64).contains(&bits) => bits,
        TypeKind::Int(_) => return unsupported(intr::WIDE),
        _ => return unsupported(intr::VECTOR),
    };
    deferred_first(cx, inst, None)?;
    let pa = cx.use_reg(a)?;
    let rb = xr(cx.use_reg(b)?);
    let (r, k) = (xr(pa).zr(), rb.zr());
    let (x, y) = (X16.zr(), X17.zr());
    let up = 64 - bits;
    if up != 0 {
        put(cx, mc::lsl_imm(Size::X, r, r, up));
        // The amount's bits above its width are unspecified.
        put(cx, mc::ubfx(Size::X, k, k, 0, bits));
    }
    put(cx, mc::lslv(Size::X, x, r, k));
    put(cx, if signed { mc::asrv(Size::X, y, x, k) } else { mc::lsrv(Size::X, y, x, k) });
    put(cx, mc::cmp(Size::X, y, r));
    if signed {
        put(cx, mc::asr_imm(Size::X, y, r, 63));
        put(cx, mc::eor_imm(Size::X, X17.sp(), y, i64::MAX as u64));
    } else {
        put(cx, mc::movn(Size::X, y, 0, 0));
    }
    put(cx, mc::csel(Size::X, x, x, y, Cond::Eq));
    if up == 0 {
        put(cx, mc::mov(Size::X, r, x));
    } else if signed {
        put(cx, mc::asr_imm(Size::X, r, x, up));
    } else {
        put(cx, mc::lsr_imm(Size::X, r, x, up));
    }
    cx.finish(inst, pa)
}

// ---- the operands of folded is.constant and objectsize ----

/// What this module remembers about the function being compiled (part
/// of `target::FnState`): whether [`begin`] found instructions that are
/// not emitted.
#[derive(Clone, Copy, Default, Debug)]
pub struct TailState {
    /// Whether [`dropped`] may be true for an instruction of the function.
    pub drops: bool,
}

/// The high bit of a use count: a use outside the block, or by a `phi`.
const EXPORTED: u32 = 1 << 31;

thread_local! {
    /// Per instruction of the function: its uses by instructions other
    /// than folded calls, and [`EXPORTED`]. One buffer per thread.
    static USES: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    /// Two bitsets over the instructions: the folded calls, then the
    /// instructions not emitted.
    static BITS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// Whether the call is one `llc`'s `LowerConstantIntrinsics` replaces by
/// a constant before instruction selection.
fn folds(site: &CallSite<'_>) -> bool {
    matches!(site.intrinsic(), Some(Intrinsic::IsConstant | Intrinsic::ObjectSize))
}

/// Whether `inst` has no effect but its result: the driver's rule for a
/// result nothing uses (`hod_cg`'s `driver.rs`, `has_no_effect`), which is
/// also what `SelectionDAG` deletes when nothing uses its node.
fn removable(inst: Inst<'_>) -> bool {
    let op = inst.opcode();
    if op.is_binary() || op.is_cast() {
        return true;
    }
    match op {
        Opcode::FNeg
        | Opcode::ICmp
        | Opcode::FCmp
        | Opcode::PtrAdd
        | Opcode::Select
        | Opcode::Freeze
        | Opcode::ExtractValue
        | Opcode::InsertValue
        | Opcode::ExtractElement
        | Opcode::InsertElement
        | Opcode::ShuffleVector => true,
        Opcode::Load => matches!(
            inst.kind(),
            InstKind::Load { mem, .. } if !mem.is_volatile() && mem.ordering().is_none()
        ),
        _ => false,
    }
}

/// `Target::begin_function`'s part (WP-084): in a function that calls
/// `llvm.is.constant` or `llvm.objectsize`, decides which instructions
/// are not emitted, as `llc` does. Its IR pass replaces those calls by
/// constants and nothing else; `SelectionDAG` then builds each block's
/// nodes and deletes a node nothing uses and that has no effect, which
/// frees its operands' nodes in turn. A value used in another block (or
/// by a `phi`) is exported and stays. So an instruction is not emitted
/// when it has no effect, no instruction but a folded call uses it
/// outside its block, and every user in its block is not emitted. A
/// function without those calls costs a look at its call sites.
pub fn begin(cx: &mut FnCx<'_, A64>) {
    let body = cx.body;
    cx.state.tail.drops = body.calls().any(|site| folds(&site)) && decide(body);
}

/// Fills [`USES`] and [`BITS`] for `body`; whether any instruction is
/// not emitted.
fn decide(body: &Body) -> bool {
    let n = body.inst_count();
    let words = n.div_ceil(64);
    USES.with(|uses| {
        BITS.with(|bits| {
            let (mut uses, mut bits) = (uses.borrow_mut(), bits.borrow_mut());
            uses.clear();
            uses.resize(n, 0);
            bits.clear();
            bits.resize(2 * words, 0);
            let set =
                |bits: &mut [u64], base: usize, i: usize| bits[base + i / 64] |= 1 << (i % 64);
            let get =
                |bits: &[u64], base: usize, i: usize| bits[base + i / 64] >> (i % 64) & 1 != 0;
            for site in body.calls() {
                if folds(&site) {
                    set(&mut bits, 0, site.inst().index());
                }
            }
            for block in body.blocks() {
                let range = block.inst_range();
                for inst in block.insts() {
                    if get(&bits, 0, inst.id().index()) {
                        continue;
                    }
                    let phi = inst.opcode() == Opcode::Phi;
                    for &op in inst.operands() {
                        if let ValueKind::Inst(x) = op.kind() {
                            let u = &mut uses[x.index()];
                            *u = (*u & EXPORTED) | ((*u & !EXPORTED) + 1).min(!EXPORTED);
                            if phi || !range.contains(&x.raw()) {
                                *u |= EXPORTED;
                            }
                        }
                    }
                }
            }
            let mut any = false;
            for block in body.blocks() {
                for inst in block.insts().rev() {
                    let i = inst.id().index();
                    if uses[i] != 0
                        || inst.ty() == TypeId::VOID
                        || get(&bits, 0, i)
                        || !removable(inst)
                    {
                        continue;
                    }
                    set(&mut bits, words, i);
                    any = true;
                    for &op in inst.operands() {
                        if let ValueKind::Inst(x) = op.kind() {
                            let u = &mut uses[x.index()];
                            if *u & !EXPORTED != 0 {
                                *u -= 1;
                            }
                        }
                    }
                }
            }
            any
        })
    })
}

/// Whether `inst` is not emitted ([`begin`]); asked only when
/// [`TailState::drops`].
pub fn dropped(inst: Inst<'_>) -> bool {
    let i = inst.id().index();
    BITS.with(|bits| {
        let bits = bits.borrow();
        let words = bits.len() / 2;
        bits.get(words + i / 64).is_some_and(|w| w >> (i % 64) & 1 != 0)
    })
}
