//! Integers of 65 to 128 bits (`spec/codegen-x86.md` §22, WP-346).
//!
//! # What a slot holds
//!
//! An integer of `N` bits, `65 <= N <= 128`, is of class `Mem`: its home
//! is a frame slot of its alloc size (16 bytes) holding `L = ceil(N / 32)`
//! 32-bit limbs, little-endian. The bits above `N` in the top limb are
//! **unspecified**, as in §8: a consumer that depends on them (a compare,
//! an extension, a `store`, the overflow test) extends the top limb first.
//!
//! # Registers
//!
//! Every lowering works limb by limb in `eax`, `ecx` and `edx`, which
//! never hold a value; it asks the pool for nothing but the register of a
//! `Gpr` operand or result, or of a pointer. A carry or borrow travels in
//! `CF` from one limb to the next: between the `add`/`adc` (`sub`/`sbb`,
//! `cmp`/`sbb`) of two limbs only `mov` loads and stores run, which keep
//! the flags; the extension of a top limb (which writes them) is done
//! before the chain starts.
//!
//! # Refused
//!
//! `udiv`/`sdiv`/`urem`/`srem`, the integer intrinsics [`intrinsic`] leaves
//! out, an argument or result of such an integer, and any integer wider
//! than 128 bits: each under its own name below. Shifts and `switch`
//! compile (WP-355); [`SHIFT`] and [`SWITCH`] are no longer reached.

use hod_cg::{CgResult, FnCx, FrameRef, Label};
use hod_ir::{Const, Inst, IntPredicate, Intrinsic, Opcode, Switch, TypeId, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Size};

use crate::alu::{self, Ext};
use crate::common::{
    EAX, ECX, EDX, branch, branch_cond, frame_inst, gp, load_at, put, store_at, unsupported,
    value_ty,
};
use crate::target::X86;

/// `udiv`, `sdiv`, `urem`, `srem` of a wide integer.
pub const DIV: &str = "x86:i64:wide:div";
/// `shl`, `lshr`, `ashr` of a wide integer.
pub const SHIFT: &str = "x86:i64:wide:shift";
/// An integer intrinsic (but the overflow ones) at a wide width.
pub const INTRINSIC: &str = "x86:i64:wide:intrinsic";
/// A `switch` on a wide integer.
pub const SWITCH: &str = "x86:i64:wide:switch";
/// A wide integer passed or returned by value.
pub const ABI: &str = "x86:i64:wide:abi";
/// An integer wider than 128 bits.
pub const WIDTH: &str = "x86:i64:wide:width";

/// The number of limbs of an `N`-bit integer, `65 <= N <= 128`.
///
/// # Errors
/// [`WIDTH`] above 128 bits.
fn limbs(bits: u32) -> CgResult<u32> {
    if bits > 128 { unsupported(WIDTH) } else { Ok(bits.div_ceil(32)) }
}

/// The meaningful bits of the top limb of an `N`-bit integer (1 to 32).
fn top_bits(bits: u32) -> u32 {
    bits - 32 * (bits.div_ceil(32) - 1)
}

/// The width of `ty` if it is an integer wider than 64 bits.
#[inline]
pub fn wide_bits(cx: &FnCx<'_, X86>, ty: TypeId) -> Option<u32> {
    cx.types().int_bits(ty).filter(|&bits| bits > 64)
}

/// The refusal of a value of the integer type of `bits` > 64 bits that
/// crosses a call boundary.
pub fn boundary_refusal(bits: u32) -> &'static str {
    if bits > 128 { WIDTH } else { ABI }
}

// ---- limbs of a slot ----

/// `mov r, [slot + 4 * limb]`.
fn ld(cx: &mut FnCx<'_, X86>, r: Reg, slot: FrameRef, limb: u32) {
    frame_inst(cx, slot.offset_by(4 * limb), |m| mc::mov_rm(Size::D, r, m));
}

/// `mov [slot + 4 * limb], r`.
fn st(cx: &mut FnCx<'_, X86>, r: Reg, slot: FrameRef, limb: u32) {
    frame_inst(cx, slot.offset_by(4 * limb), |m| mc::mov_mr(Size::D, m, r));
}

/// `mov dword [slot + 4 * limb], imm`.
fn st_imm(cx: &mut FnCx<'_, X86>, slot: FrameRef, limb: u32, imm: u32) {
    frame_inst(cx, slot.offset_by(4 * limb), |m| mc::mov_mi(Size::D, m, i64::from(imm as i32)));
}

/// The instruction `make` builds over limb `limb` of `slot`.
fn op_m(cx: &mut FnCx<'_, X86>, slot: FrameRef, limb: u32, make: impl FnOnce(Mem) -> mc::Inst) {
    frame_inst(cx, slot.offset_by(4 * limb), make);
}

/// The top limb of the `bits`-bit integer at `slot` into `r`, extended
/// from its meaningful bits by `ext` (flags written).
fn ld_top(cx: &mut FnCx<'_, X86>, r: Reg, slot: FrameRef, bits: u32, ext: Ext) {
    ld(cx, r, slot, bits.div_ceil(32) - 1);
    alu::extend_to(cx, r, top_bits(bits), ext);
}

/// Copies `count` limbs of `src` to `dst` through `ecx` (flags kept).
fn copy(cx: &mut FnCx<'_, X86>, dst: FrameRef, src: FrameRef, count: u32) {
    if dst == src {
        return;
    }
    for limb in 0..count {
        ld(cx, ECX, src, limb);
        st(cx, ECX, dst, limb);
    }
}

/// Limbs `from..to` of `r`: the sign of `ecx` (`sar ecx, 31` first) or 0.
fn fill(cx: &mut FnCx<'_, X86>, r: FrameRef, from: u32, to: u32, sign: bool) {
    if from >= to {
        return;
    }
    if sign {
        put(cx, mc::sar_ri(Size::D, ECX, 31));
    }
    for limb in from..to {
        if sign { st(cx, ECX, r, limb) } else { st_imm(cx, r, limb, 0) }
    }
}

/// Writes the `bits`-bit integer at `src` to `dst` with exactly its store
/// size, the bits above `N` zero (`llc`'s image in memory): an
/// `insertvalue` or an overflow result. Writes `ecx`, `edx`, the flags.
pub fn store_frame(cx: &mut FnCx<'_, X86>, dst: FrameRef, src: FrameRef, bits: u32) {
    let count = bits.div_ceil(32);
    for limb in 0..count - 1 {
        ld(cx, ECX, src, limb);
        st(cx, ECX, dst, limb);
    }
    ld_top(cx, ECX, src, bits, Ext::Zero);
    let top = bits.div_ceil(8) - 4 * (count - 1);
    crate::common::store_gpr(cx, ECX, dst.offset_by(4 * (count - 1)), top);
}

// ---- constants ----

/// `Target::materialize_mem` for a wide integer constant: one immediate
/// store per limb, or a symbol's address through `ecx` with the other
/// limbs zero. Flags kept.
///
/// # Errors
/// [`WIDTH`]; [`crate::common::CONSTANT`] for another constant.
pub fn materialize_mem(cx: &mut FnCx<'_, X86>, dst: FrameRef, value: Value) -> CgResult<()> {
    let bits = cx.types().int_bits(value_ty(cx, value)).unwrap_or(128);
    let count = limbs(bits)?;
    let ValueKind::Const(id) = value.kind() else {
        return cx.bug("materialize_mem of something that is no constant");
    };
    match cx.module.consts.get(id) {
        Const::WideInt { words, .. } => {
            let words = words.to_vec();
            for limb in 0..count {
                let word = words.get(limb as usize / 2).copied().unwrap_or(0);
                st_imm(cx, dst, limb, (word >> (32 * (limb % 2))) as u32);
            }
            Ok(())
        }
        Const::Int { value, .. } => {
            for limb in 0..count {
                let v = if limb < 2 { (value >> (32 * limb)) as u32 } else { 0 };
                st_imm(cx, dst, limb, v);
            }
            Ok(())
        }
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => {
            for limb in 0..count {
                st_imm(cx, dst, limb, 0);
            }
            Ok(())
        }
        Const::IntOfAddr { .. } | Const::AddrOfInt { .. } => {
            crate::common::materialize_const(cx, ECX, id)?;
            st(cx, ECX, dst, 0);
            for limb in 1..count {
                st_imm(cx, dst, limb, 0);
            }
            Ok(())
        }
        _ => unsupported(crate::common::CONSTANT),
    }
}

// ---- binary operations ----

/// A `reg, mem` instruction's constructor.
type RegMem = fn(Size, Reg, Mem) -> mc::Inst;

/// `%r = <op> %lhs, %rhs`: `add`/`adc`, `sub`/`sbb` and the bitwise
/// operations limb by limb through `eax`.
///
/// # Errors
/// [`MUL`], [`DIV`], [`SHIFT`], [`WIDTH`]; what the operands report.
pub fn binary<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let bits = cx.types().int_bits(inst.ty()).unwrap_or(128);
    let count = limbs(bits)?;
    let op = inst.opcode();
    let (first, rest): (RegMem, RegMem) = match op {
        Opcode::Add => (mc::add_rm, mc::adc_rm),
        Opcode::Sub => (mc::sub_rm, mc::sbb_rm),
        Opcode::And => (mc::and_rm, mc::and_rm),
        Opcode::Or => (mc::or_rm, mc::or_rm),
        Opcode::Xor => (mc::xor_rm, mc::xor_rm),
        Opcode::Mul => return mul(cx, inst, lhs, rhs),
        Opcode::Shl | Opcode::LShr | Opcode::AShr => return shift(cx, inst, lhs, rhs),
        Opcode::UDiv | Opcode::SDiv | Opcode::URem | Opcode::SRem => return unsupported(DIV),
        _ => return unsupported(alu::CAST),
    };
    let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    let r = cx.def_mem(inst)?;
    for limb in 0..count {
        ld(cx, EAX, a, limb);
        let make = if limb == 0 { first } else { rest };
        op_m(cx, b, limb, |m| make(Size::D, EAX, m));
        st(cx, EAX, r, limb);
    }
    Ok(())
}

// ---- shifts ----

/// The limbs of fill beside the value in a shift's buffer: one more than
/// the largest limb distance of an in-range amount (3, at 97 to 128 bits),
/// so that every masked distance reads inside the buffer.
const PAD: u32 = 4;

/// The amount of a shift by the constant `value`, saturated: `u64::MAX`
/// for one with a high limb set; `None` when `value` is no constant.
fn const_amount(cx: &FnCx<'_, X86>, value: Value) -> Option<u64> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(value),
        Const::WideInt { words, .. } => {
            Some(if words.iter().skip(1).all(|&w| w == 0) { words[0] } else { u64::MAX })
        }
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// `%r = shl|lshr|ashr iN %lhs, %rhs` (`llc`'s shift through a stack
/// buffer). The buffer holds the value's `L` limbs, its top limb extended
/// (zero, or sign for `ashr`), and [`PAD`] limbs of fill (0, or the sign)
/// below it for `shl`, above it for the right shifts. Result limb `i` is
/// the double shift (`shld` / `shrd`) of the buffer's limbs `i - q` and
/// `i - q - 1` (`shl`) or `i + q` and `i + q + 1`, `q = amount / 32`, by
/// `amount % 32`. A constant amount picks the limbs at compile time (none
/// at all of the width or more: poison); a variable one is the low limb
/// in `ecx`, `edx = &buffer -/+ 4 ((ecx >> 5) & 3)`, and `shld` / `shrd
/// [r + 4 i], eax, cl` (which count modulo 32). The mask keeps every
/// access inside the buffer for any amount. No branch, no call, no
/// register but `eax`, `ecx`, `edx`.
///
/// # Errors
/// [`WIDTH`]; what the operands report.
fn shift<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let bits = cx.types().int_bits(inst.ty()).unwrap_or(128);
    let count = limbs(bits)?;
    let op = inst.opcode();
    let right = op != Opcode::Shl;
    let ext = if op == Opcode::AShr { Ext::Sign } else { Ext::Zero };
    let amount = const_amount(cx, rhs);
    if amount.is_some_and(|k| k >= u64::from(bits)) {
        return Ok(());
    }
    let a = cx.use_mem(lhs)?;
    let n = match amount {
        Some(_) => None,
        None => Some(cx.use_mem(rhs)?),
    };
    let r = cx.def_mem(inst)?;
    let buf = cx.alloc_slot(u64::from(4 * (count + PAD)), 4)?;
    let (value, pad) =
        if right { (buf, buf.offset_by(4 * count)) } else { (buf.offset_by(4 * PAD), buf) };
    copy(cx, value, a, count - 1);
    ld_top(cx, ECX, a, bits, ext);
    st(cx, ECX, value, count - 1);
    fill(cx, pad, 0, PAD, ext == Ext::Sign);
    // The buffer's limbs read for result limb `i` at limb distance `q`:
    // (the one shifted out of, the one shifted in from).
    let at =
        |i: u32, q: u32| if right { (i + q, i + q + 1) } else { (PAD + i - q, PAD + i - q - 1) };
    let Some(n) = n else {
        let by = amount.unwrap_or(0) as u32;
        let (dist, s) = (by / 32, (by % 32) as u8);
        for i in 0..count {
            let (from, other) = at(i, dist);
            ld(cx, EAX, buf, from);
            if s != 0 {
                ld(cx, EDX, buf, other);
                put(
                    cx,
                    if right {
                        mc::shrd_rri(Size::D, EAX, EDX, s)
                    } else {
                        mc::shld_rri(Size::D, EAX, EDX, s)
                    },
                );
            }
            st(cx, EAX, r, i);
        }
        return Ok(());
    };
    ld(cx, ECX, n, 0);
    put(cx, mc::mov_rr(Size::D, EDX, ECX));
    put(cx, mc::shr_ri(Size::D, EDX, 5));
    put(cx, mc::and_ri(Size::D, EDX, 3));
    put(cx, mc::shl_ri(Size::D, EDX, 2));
    if !right {
        put(cx, mc::neg_r(Size::D, EDX));
    }
    crate::common::frame_address(cx, EAX, buf);
    put(cx, mc::add_rr(Size::D, EDX, EAX));
    for i in 0..count {
        let (from, other) = at(i, 0);
        put(cx, mc::mov_rm(Size::D, EAX, Mem::base(EDX, 4 * from as i32)));
        st(cx, EAX, r, i);
        put(cx, mc::mov_rm(Size::D, EAX, Mem::base(EDX, 4 * other as i32)));
        op_m(cx, r, i, |m| {
            if right { mc::shrd_mrcl(Size::D, m, EAX) } else { mc::shld_mrcl(Size::D, m, EAX) }
        });
    }
    Ok(())
}

// ---- integer intrinsics ----

/// `%r = call iN @llvm.<which>(...)`, `N` from 65 to 128: `ctpop`,
/// `ctlz`, `cttz`, `bswap`, `bitreverse`, `abs`, `smax` / `smin` /
/// `umax` / `umin`. The saturating operations, the funnel and saturating
/// shifts and `scmp` / `ucmp` are [`INTRINSIC`].
///
/// # Errors
/// [`INTRINSIC`], [`WIDTH`]; what the operands report.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    let bits = cx.types().int_bits(inst.ty()).unwrap_or(128);
    let count = limbs(bits)?;
    match (which, args) {
        (I::Ctpop | I::Ctlz | I::Cttz, &[x, ..]) => {
            let a = cx.use_mem(x)?;
            let r = cx.def_mem(inst)?;
            match which {
                I::Ctpop => pop_count(cx, a, bits),
                I::Ctlz => zeros_count(cx, a, bits, true)?,
                _ => zeros_count(cx, a, bits, false)?,
            }
            st(cx, EAX, r, 0);
            for limb in 1..count {
                st_imm(cx, r, limb, 0);
            }
            Ok(())
        }
        (I::Bswap | I::BitReverse, &[x]) => {
            let a = cx.use_mem(x)?;
            let r = cx.def_mem(inst)?;
            reverse(cx, r, a, bits, which == I::Bswap)
        }
        (I::Abs, &[x, ..]) => {
            let a = cx.use_mem(x)?;
            let r = cx.def_mem(inst)?;
            abs(cx, r, a, bits);
            Ok(())
        }
        (I::Smax | I::Smin | I::Umax | I::Umin, &[lhs, rhs]) => {
            let pred = match which {
                I::Smax => IntPredicate::Sgt,
                I::Smin => IntPredicate::Slt,
                I::Umax => IntPredicate::Ugt,
                _ => IntPredicate::Ult,
            };
            let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
            let r = cx.def_mem(inst)?;
            let cond = compare_slots(cx, pred, bits, a, b);
            let (other, done): (Label, Label) = (cx.new_label(), cx.new_label());
            branch_cond(cx, cond.invert(), other)?;
            copy(cx, r, a, count);
            branch(cx, done)?;
            cx.bind(other)?;
            copy(cx, r, b, count);
            cx.bind(done)
        }
        _ => unsupported(INTRINSIC),
    }
}

/// The population count of the `bits`-bit integer at `a` into `eax`:
/// each limb's (the top one zero-extended) summed in `edx`.
fn pop_count(cx: &mut FnCx<'_, X86>, a: FrameRef, bits: u32) {
    let count = bits.div_ceil(32);
    for limb in 0..count {
        if limb == count - 1 {
            ld_top(cx, EAX, a, bits, Ext::Zero);
        } else {
            ld(cx, EAX, a, limb);
        }
        crate::int::pop32(cx, EAX, ECX);
        if limb == 0 {
            put(cx, mc::mov_rr(Size::D, EDX, EAX));
        } else {
            put(cx, mc::add_rr(Size::D, EDX, EAX));
        }
    }
    put(cx, mc::mov_rr(Size::D, EAX, EDX));
}

/// The leading (`leading`) or trailing zeros of the `bits`-bit integer at
/// `a` into `eax`, `bits` for 0. Leading: from the top limb (zero-extended)
/// down, the first that is not zero gives `bsr`·`xor 31` plus 32 per limb
/// above it, less the `32 L - N` bits above the width. Trailing: from limb
/// 0 up, the top limb with bit `N` set (below 32 bits), `bsf` plus 32 per
/// limb below it. Branches between the limbs; `ecx` is not written.
///
/// # Errors
/// What the branches report.
fn zeros_count(cx: &mut FnCx<'_, X86>, a: FrameRef, bits: u32, leading: bool) -> CgResult<()> {
    let count = bits.div_ceil(32);
    let top = top_bits(bits);
    let done = cx.new_label();
    for step in 0..count {
        let limb = if leading { count - 1 - step } else { step };
        if limb == count - 1 {
            ld_top(cx, EAX, a, bits, Ext::Zero);
            if !leading && top < 32 {
                put(cx, mc::or_ri(Size::D, EAX, i64::from(1i32 << top)));
            }
        } else {
            ld(cx, EAX, a, limb);
        }
        let next = cx.new_label();
        put(cx, mc::test_rr(Size::D, EAX, EAX));
        branch_cond(cx, Cond::E, next)?;
        if leading {
            put(cx, mc::bsr_rr(Size::D, EAX, EAX));
            put(cx, mc::xor_ri(Size::D, EAX, 31));
            let above = 32 * step;
            let adjust = i64::from(above) - i64::from(32 * count - bits);
            if adjust != 0 {
                put(cx, mc::add_ri(Size::D, EAX, adjust));
            }
        } else {
            put(cx, mc::bsf_rr(Size::D, EAX, EAX));
            if limb != 0 {
                put(cx, mc::add_ri(Size::D, EAX, i64::from(32 * limb)));
            }
        }
        branch(cx, done)?;
        cx.bind(next)?;
    }
    put(cx, mc::mov_ri(Size::D, EAX, i64::from(bits)));
    cx.bind(done)
}

/// `bswap` (`bytes`) or `bitreverse` of the `bits`-bit integer at `a`
/// into `r`: limb `L - 1 - i` reversed into limb `i` of a frame area
/// (the bits above the width land in its low `32 L - N` bits), then the
/// area shifted right by `32 L - N` (`shrd`), the top limb by `shr`.
///
/// # Errors
/// What the frame reports.
fn reverse(
    cx: &mut FnCx<'_, X86>,
    r: FrameRef,
    a: FrameRef,
    bits: u32,
    bytes: bool,
) -> CgResult<()> {
    let count = bits.div_ceil(32);
    let up = (32 * count - bits) as u8;
    let t = cx.alloc_slot(u64::from(4 * count), 4)?;
    for limb in 0..count {
        ld(cx, EAX, a, count - 1 - limb);
        if bytes {
            put(cx, mc::bswap(Size::D, EAX));
        } else {
            crate::int::rev32(cx, EAX, ECX);
        }
        st(cx, EAX, t, limb);
    }
    for limb in 0..count {
        ld(cx, EAX, t, limb);
        if up != 0 {
            if limb + 1 < count {
                ld(cx, EDX, t, limb + 1);
                put(cx, mc::shrd_rri(Size::D, EAX, EDX, up));
            } else {
                put(cx, mc::shr_ri(Size::D, EAX, up));
            }
        }
        st(cx, EAX, r, limb);
    }
    Ok(())
}

/// `abs` of the `bits`-bit integer at `a` into `r`: `edx` = its sign
/// (the top limb sign-extended, `sar 31`), each limb `xor edx` into `r`
/// (the top one extended), then `sub` / `sbb [r + 4 k], edx`.
fn abs(cx: &mut FnCx<'_, X86>, r: FrameRef, a: FrameRef, bits: u32) {
    let count = bits.div_ceil(32);
    ld_top(cx, EDX, a, bits, Ext::Sign);
    put(cx, mc::mov_rr(Size::D, ECX, EDX));
    put(cx, mc::sar_ri(Size::D, EDX, 31));
    for limb in 0..count {
        if limb == count - 1 {
            put(cx, mc::mov_rr(Size::D, EAX, ECX));
        } else {
            ld(cx, EAX, a, limb);
        }
        put(cx, mc::xor_rr(Size::D, EAX, EDX));
        st(cx, EAX, r, limb);
    }
    for limb in 0..count {
        op_m(cx, r, limb, |m| {
            if limb == 0 { mc::sub_mr(Size::D, m, EDX) } else { mc::sbb_mr(Size::D, m, EDX) }
        });
    }
}

// ---- switch ----

/// The integer constant `c` as a `u128` (`None` for another constant).
fn const_u128(c: Const<'_>) -> Option<u128> {
    match c {
        Const::Int { value, .. } => Some(u128::from(value)),
        Const::WideInt { words, .. } => Some(
            u128::from(words.first().copied().unwrap_or(0))
                | u128::from(words.get(1).copied().unwrap_or(0)) << 64,
        ),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// `switch iN %cond, ...`: the top limb zero-extended into `edx` once;
/// per case `cmp dword [cond + 4 k], imm`·`jne next` for each lower limb,
/// `cmp edx, imm`·`je case`; then the default. A constant condition
/// branches to its block.
///
/// # Errors
/// [`WIDTH`]; what the operand and the branches report.
pub fn switch<'a>(cx: &mut FnCx<'a, X86>, switch: &Switch<'a>) -> CgResult<()> {
    let bits = cx.types().int_bits(value_ty(cx, switch.cond)).unwrap_or(128);
    let count = limbs(bits)?;
    let all = if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 };
    let case_value =
        |cx: &FnCx<'a, X86>, id| const_u128(cx.module.consts.get(id)).unwrap_or(0) & all;
    if let ValueKind::Const(id) = switch.cond.kind()
        && let Some(c) = const_u128(cx.module.consts.get(id))
    {
        let target = switch
            .cases()
            .find(|&(case, _)| case_value(cx, case) == c & all)
            .map_or(switch.default, |(_, block)| block);
        return crate::control::folded_br(cx, target);
    }
    let a = cx.use_mem(switch.cond)?;
    ld_top(cx, EDX, a, bits, Ext::Zero);
    for (case, block) in switch.cases() {
        let value = case_value(cx, case);
        let next = cx.new_label();
        for limb in 0..count - 1 {
            let word = (value >> (32 * limb)) as u32;
            op_m(cx, a, limb, |m| mc::cmp_mi(Size::D, m, i64::from(word as i32)));
            branch_cond(cx, Cond::Ne, next)?;
        }
        let top = (value >> (32 * (count - 1))) as u32;
        put(cx, mc::cmp_ri(Size::D, EDX, i64::from(top as i32)));
        let label = cx.block_label(block);
        branch_cond(cx, Cond::E, label)?;
        cx.bind(next)?;
    }
    crate::control::br(cx, switch.default)
}

/// `%r = freeze %arg`: the limbs copied.
///
/// # Errors
/// [`WIDTH`]; what the operand reports.
pub fn freeze<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let count = limbs(cx.types().int_bits(inst.ty()).unwrap_or(128))?;
    let a = cx.use_mem(arg)?;
    let r = cx.def_mem(inst)?;
    copy(cx, r, a, count);
    Ok(())
}

// ---- compares ----

/// The compare of the `bits`-bit integers at `a` and `b` into the flags;
/// the condition that holds when `pred` does. Equality: the `xor` of each
/// limb, `or`ed into `ecx`. Order: both top limbs extended into `ecx` and
/// `edx` first, then `cmp` of limb 0, `sbb` of the middle limbs through
/// `eax`, `sbb ecx, edx`; the operands swapped for `>` and `<=`.
fn compare_slots(
    cx: &mut FnCx<'_, X86>,
    pred: IntPredicate,
    bits: u32,
    a: FrameRef,
    b: FrameRef,
) -> Cond {
    let count = bits.div_ceil(32);
    let signed = matches!(
        pred,
        IntPredicate::Sgt | IntPredicate::Sge | IntPredicate::Slt | IntPredicate::Sle
    );
    let ext = if signed { Ext::Sign } else { Ext::Zero };
    if matches!(pred, IntPredicate::Eq | IntPredicate::Ne) {
        ld_top(cx, ECX, a, bits, ext);
        ld_top(cx, EDX, b, bits, ext);
        put(cx, mc::xor_rr(Size::D, ECX, EDX));
        for limb in 0..count - 1 {
            ld(cx, EAX, a, limb);
            op_m(cx, b, limb, |m| mc::xor_rm(Size::D, EAX, m));
            put(cx, mc::or_rr(Size::D, ECX, EAX));
        }
        return alu::cond_of(pred);
    }
    let (x, y, cond) = match pred {
        IntPredicate::Ult => (a, b, Cond::B),
        IntPredicate::Uge => (a, b, Cond::Ae),
        IntPredicate::Ugt => (b, a, Cond::B),
        IntPredicate::Ule => (b, a, Cond::Ae),
        IntPredicate::Slt => (a, b, Cond::L),
        IntPredicate::Sge => (a, b, Cond::Ge),
        IntPredicate::Sgt => (b, a, Cond::L),
        _ => (b, a, Cond::Ge),
    };
    ld_top(cx, ECX, x, bits, ext);
    ld_top(cx, EDX, y, bits, ext);
    for limb in 0..count - 1 {
        ld(cx, EAX, x, limb);
        if limb == 0 {
            op_m(cx, y, limb, |m| mc::cmp_rm(Size::D, EAX, m));
        } else {
            op_m(cx, y, limb, |m| mc::sbb_rm(Size::D, EAX, m));
        }
    }
    put(cx, mc::sbb_rr(Size::D, ECX, EDX));
    cond
}

/// `%r = icmp <pred> %lhs, %rhs`: the compare, `setcc`, `movzx`.
///
/// # Errors
/// [`WIDTH`]; what the operands report.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let bits = cx.types().int_bits(value_ty(cx, lhs)).unwrap_or(128);
    limbs(bits)?;
    if cx.types().int_bits(inst.ty()) != Some(1) {
        return unsupported(alu::CAST);
    }
    let rd = cx.def_reg(inst)?;
    let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    let cond = compare_slots(cx, pred, bits, a, b);
    let r = gp(rd);
    let byte = if crate::reg::has_low_byte(r) { r } else { ECX };
    put(cx, mc::setcc_r(cond, byte));
    put(cx, mc::movzx_rr(Size::D, r, Size::B, byte));
    cx.finish(inst, rd)
}

/// `%r = select i1 %cond, %if_true, %if_false`: the condition in `eax`,
/// `test al, 1`, a copy of the chosen arm on each side of a branch.
///
/// # Errors
/// [`WIDTH`]; what the operands report.
pub fn select<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let count = limbs(cx.types().int_bits(inst.ty()).unwrap_or(128))?;
    let (t, f) = (cx.use_mem(if_true)?, cx.use_mem(if_false)?);
    let r = cx.def_mem(inst)?;
    if let Some(c) = crate::common::const_int(cx, cond) {
        copy(cx, r, if c & 1 != 0 { t } else { f }, count);
        return Ok(());
    }
    cx.put(cond, crate::reg::EAX)?;
    put(cx, mc::test_ri(Size::B, EAX, 1));
    let (other, done): (Label, Label) = (cx.new_label(), cx.new_label());
    branch_cond(cx, Cond::E, other)?;
    copy(cx, r, t, count);
    branch(cx, done)?;
    cx.bind(other)?;
    copy(cx, r, f, count);
    cx.bind(done)?;
    Ok(())
}

// ---- casts ----

/// `%r = <cast> %arg` (`zext`, `sext`, `trunc`, `ptrtoint`, `inttoptr`,
/// `bitcast`; `ext` from the opcode) with an integer wider than 64 bits on
/// either side. A narrower result takes the low limbs; a wider one the
/// source's limbs, its top limb extended, then limbs of its sign or 0.
///
/// # Errors
/// [`WIDTH`]; what the operand reports.
pub fn cast<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value, ext: Ext) -> CgResult<()> {
    let int_bits = |cx: &FnCx<'_, X86>, ty| cx.types().int_bits(ty).unwrap_or(32);
    let from_bits = int_bits(cx, value_ty(cx, arg));
    let to_bits = int_bits(cx, inst.ty());
    limbs(from_bits)?;
    limbs(to_bits)?;
    if to_bits <= 32 {
        let a = cx.use_mem(arg)?;
        let rd = cx.def_reg(inst)?;
        ld(cx, gp(rd), a, 0);
        return cx.finish(inst, rd);
    }
    let to_count = to_bits.div_ceil(32);
    let ext = if ext == Ext::None { Ext::Zero } else { ext };
    if from_bits <= 32 {
        let ra = gp(cx.use_reg(arg)?);
        let r = cx.def_mem(inst)?;
        alu::extend_to(cx, ra, from_bits, ext);
        st(cx, ra, r, 0);
        put(cx, mc::mov_rr(Size::D, ECX, ra));
        fill(cx, r, 1, to_count, ext == Ext::Sign);
        return Ok(());
    }
    let from_count = from_bits.div_ceil(32);
    let a = cx.use_mem(arg)?;
    let r = cx.def_mem(inst)?;
    if from_bits >= to_bits {
        copy(cx, r, a, to_count);
        return Ok(());
    }
    copy(cx, r, a, from_count - 1);
    ld_top(cx, ECX, a, from_bits, ext);
    st(cx, ECX, r, from_count - 1);
    fill(cx, r, from_count, to_count, ext == Ext::Sign);
    Ok(())
}

// ---- memory ----

/// `%r = load iN, ptr %ptr`: limb by limb through `ecx`, the last with
/// the rest of the store size.
///
/// # Errors
/// [`WIDTH`], `x86:atomic`; what the operands report.
pub fn load<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, ptr: Value) -> CgResult<()> {
    let bits = cx.types().int_bits(inst.ty()).unwrap_or(128);
    let count = limbs(bits)?;
    let bytes = bits.div_ceil(8);
    let p = crate::int::pointer(cx, ptr)?;
    let r = cx.def_mem(inst)?;
    for limb in 0..count {
        let n = (bytes - 4 * limb).min(4);
        let at = 4 * limb as i32;
        load_at(cx, ECX, n, EDX, |k| Mem::base(p, at + k));
        st(cx, ECX, r, limb);
    }
    Ok(())
}

/// `store iN %value, ptr %ptr`: limb by limb through `ecx`, the top limb
/// zero-extended from the width, exactly the store size.
///
/// # Errors
/// [`WIDTH`]; what the operands report.
pub fn store(cx: &mut FnCx<'_, X86>, value: Value, ptr: Value) -> CgResult<()> {
    let bits = cx.types().int_bits(value_ty(cx, value)).unwrap_or(128);
    let count = limbs(bits)?;
    let bytes = bits.div_ceil(8);
    let v = cx.use_mem(value)?;
    let p = crate::int::pointer(cx, ptr)?;
    for limb in 0..count {
        if limb == count - 1 {
            ld_top(cx, ECX, v, bits, Ext::Zero);
        } else {
            ld(cx, ECX, v, limb);
        }
        let n = (bytes - 4 * limb).min(4);
        let at = 4 * limb as i32;
        store_at(cx, ECX, n, EDX, |k| Mem::base(p, at + k));
    }
    Ok(())
}

// ---- overflow ----

/// `{iN, i1} @llvm.<s|u><add|sub>.with.overflow.iN(a, b)`, `N` from 65 to
/// 128: both top limbs extended (by sign for the signed operations) into
/// `ecx` and `edx`, then `add`/`adc` (`sub`/`sbb`) limb by limb through
/// `eax`, `adc ecx, edx` last. At `N = 32 L` the flags of the last limb
/// (`OF` signed, `CF` unsigned); below, the exact result fits `32 L` bits
/// and the operation overflowed when the top limb is not its own sign
/// (signed) or zero (unsigned) extension from `N`'s top bits. The integer
/// is stored zero-extended with its store size, the flag as a byte at
/// `flag_at`.
///
/// # Errors
/// [`WIDTH`]; what the operands report.
#[allow(clippy::too_many_arguments, reason = "the operation's parts, once")]
pub fn overflow<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    add: bool,
    signed: bool,
    bits: u32,
    flag_at: u32,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let count = limbs(bits)?;
    let ext = if signed { Ext::Sign } else { Ext::Zero };
    let sa = cx.use_mem(a)?;
    let sb = cx.use_mem(b)?;
    let r = cx.def_mem(inst)?;
    ld_top(cx, ECX, sa, bits, ext);
    ld_top(cx, EDX, sb, bits, ext);
    for limb in 0..count - 1 {
        ld(cx, EAX, sa, limb);
        op_m(cx, sb, limb, |m| match (add, limb) {
            (true, 0) => mc::add_rm(Size::D, EAX, m),
            (true, _) => mc::adc_rm(Size::D, EAX, m),
            (false, 0) => mc::sub_rm(Size::D, EAX, m),
            (false, _) => mc::sbb_rm(Size::D, EAX, m),
        });
        st(cx, EAX, r, limb);
    }
    put(cx, if add { mc::adc_rr(Size::D, ECX, EDX) } else { mc::sbb_rr(Size::D, ECX, EDX) });
    let top = top_bits(bits);
    let cond = if top == 32 {
        if signed { Cond::O } else { Cond::B }
    } else {
        put(cx, mc::mov_rr(Size::D, EDX, ECX));
        if signed {
            alu::extend_to(cx, EDX, top, Ext::Sign);
            put(cx, mc::cmp_rr(Size::D, EDX, ECX));
        } else {
            put(cx, mc::shr_ri(Size::D, EDX, top as u8));
        }
        Cond::Ne
    };
    put(cx, mc::setcc_r(cond, EDX));
    frame_inst(cx, r.offset_by(flag_at), |m| mc::mov_mr(Size::B, m, EDX));
    alu::extend_to(cx, ECX, top, Ext::Zero);
    let top_bytes = bits.div_ceil(8) - 4 * (count - 1);
    crate::common::store_gpr(cx, ECX, r.offset_by(4 * (count - 1)), top_bytes);
    Ok(())
}

// ---- multiplication ----

/// The low `out` limbs of the product of the `bits`-bit integers at `sa`
/// and `sb`, each first extended by `ext` to `out` limbs, in a frame area
/// of its own (returned): one `mul` per pair of limbs whose product lands
/// below limb `out`, its `edx:eax` added into the area with the carry
/// rippled up to the top limb. `out = 2 L` gives the exact product.
fn product(
    cx: &mut FnCx<'_, X86>,
    sa: FrameRef,
    sb: FrameRef,
    bits: u32,
    ext: Ext,
    out: u32,
) -> CgResult<FrameRef> {
    let count = bits.div_ceil(32);
    let area = cx.alloc_slot(u64::from(12 * out), 4)?;
    let (xa, xb, p) = (area, area.offset_by(4 * out), area.offset_by(8 * out));
    for (from, to) in [(sa, xa), (sb, xb)] {
        copy(cx, to, from, count - 1);
        ld_top(cx, ECX, from, bits, ext);
        st(cx, ECX, to, count - 1);
        fill(cx, to, count, out, ext == Ext::Sign);
    }
    for k in 0..out {
        st_imm(cx, p, k, 0);
    }
    for i in 0..out {
        for j in 0..out - i {
            let k = i + j;
            ld(cx, EAX, xa, i);
            op_m(cx, xb, j, |m| mc::mul_m(Size::D, m));
            op_m(cx, p, k, |m| mc::add_mr(Size::D, m, EAX));
            if k + 1 < out {
                op_m(cx, p, k + 1, |m| mc::adc_mr(Size::D, m, EDX));
            }
            for up in k + 2..out {
                op_m(cx, p, up, |m| mc::adc_mi(Size::D, m, 0));
            }
        }
    }
    Ok(p)
}

/// `%r = mul iN %lhs, %rhs`: the low `L` limbs of [`product`].
///
/// # Errors
/// [`WIDTH`]; what the operands report.
fn mul<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let bits = cx.types().int_bits(inst.ty()).unwrap_or(128);
    let count = limbs(bits)?;
    let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    let p = product(cx, a, b, bits, Ext::Zero, count)?;
    let r = cx.def_mem(inst)?;
    copy(cx, r, p, count);
    Ok(())
}

/// `{iN, i1} @llvm.<s|u>mul.with.overflow.iN(a, b)`, `N` from 65 to 128:
/// the exact product `p` ([`product`] over `2 L` limbs, the operands
/// extended by sign for `smul`, by zero for `umul`); overflow when `p` is
/// not its own sign extension from `N` bits (signed) or has a bit at `N`
/// or above (unsigned). The result as [`overflow`]'s.
///
/// # Errors
/// [`WIDTH`]; what the operands report.
#[allow(clippy::too_many_arguments, reason = "the operation's parts, once")]
pub fn mul_overflow<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    signed: bool,
    bits: u32,
    flag_at: u32,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let count = limbs(bits)?;
    let ext = if signed { Ext::Sign } else { Ext::Zero };
    let (sa, sb) = (cx.use_mem(a)?, cx.use_mem(b)?);
    let p = product(cx, sa, sb, bits, ext, 2 * count)?;
    let top = top_bits(bits);
    if signed {
        // `eax` = limb L-1 extended from the top bits; `ecx` collects its
        // difference from the limb and every higher limb's from its sign.
        ld(cx, EAX, p, count - 1);
        alu::extend_to(cx, EAX, top, Ext::Sign);
        put(cx, mc::mov_rr(Size::D, ECX, EAX));
        op_m(cx, p, count - 1, |m| mc::xor_rm(Size::D, ECX, m));
        put(cx, mc::sar_ri(Size::D, EAX, 31));
        for k in count..2 * count {
            ld(cx, EDX, p, k);
            put(cx, mc::xor_rr(Size::D, EDX, EAX));
            put(cx, mc::or_rr(Size::D, ECX, EDX));
        }
    } else {
        if top == 32 {
            put(cx, mc::xor_rr(Size::D, ECX, ECX));
        } else {
            ld(cx, ECX, p, count - 1);
            put(cx, mc::shr_ri(Size::D, ECX, top as u8));
        }
        for k in count..2 * count {
            op_m(cx, p, k, |m| mc::or_rm(Size::D, ECX, m));
        }
    }
    let r = cx.def_mem(inst)?;
    put(cx, mc::setcc_r(Cond::Ne, EDX));
    frame_inst(cx, r.offset_by(flag_at), |m| mc::mov_mr(Size::B, m, EDX));
    store_frame(cx, r, p, bits);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_stable() {
        assert_eq!(DIV, "x86:i64:wide:div");
        assert_eq!(SHIFT, "x86:i64:wide:shift");
        assert_eq!(INTRINSIC, "x86:i64:wide:intrinsic");
        assert_eq!(SWITCH, "x86:i64:wide:switch");
        assert_eq!(ABI, "x86:i64:wide:abi");
        assert_eq!(WIDTH, "x86:i64:wide:width");
    }

    #[test]
    fn limb_geometry() {
        assert_eq!((limbs(65).ok(), top_bits(65)), (Some(3), 1));
        assert_eq!((limbs(96).ok(), top_bits(96)), (Some(3), 32));
        assert_eq!((limbs(97).ok(), top_bits(97)), (Some(4), 1));
        assert_eq!((limbs(128).ok(), top_bits(128)), (Some(4), 32));
        assert!(limbs(129).is_err());
    }
}
