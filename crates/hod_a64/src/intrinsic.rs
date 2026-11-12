//! Intrinsic calls (`spec/ir.md` §5.5) other than those the dispatch
//! routes to `call` (`llvm.va_*`), `mem` (`llvm.stacksave`,
//! `llvm.stackrestore`) and `aggr` (`*.with.overflow`): memory
//! intrinsics, bit manipulation, `abs` and min/max, traps, the frame
//! intrinsics, and those that fold to a constant or to nothing.
//!
//! **Owner: WP-041.** Tests: `tests/intrinsic.rs`. The dispatch in
//! `target.rs` is final. Thread-local addresses are refused by the
//! dispatch (`a64:thread-local`, milestone M2). The intrinsics that
//! generate no code at all (`llvm.lifetime.*`, `llvm.assume`,
//! `llvm.expect*`, `llvm.launder.invariant.group`, ...) never reach this
//! module: the producer drops them (`hod_ir::no_code_intrinsic`). The
//! decisions are `spec/codegen.md` §5.5.1, "Intrinsics (WP-041)":
//!
//! - **Integers** of `i1` to `i64` (class `Gpr`), the bits above the
//!   width unspecified (§2.2) in operands and results. `ctlz` shifts the
//!   value to the top of its register and sets the bit below it, so that
//!   0 counts the IR width; `cttz` sets bit `N` first. `ctpop` counts in
//!   the scratch vector register `v31` (`cnt` + `uaddlv`). `bswap` and
//!   `bitreverse` reverse the register and shift the value back down.
//!   `fshl`/`fshr` take the amount modulo the width: the register shift
//!   does it at 32 and 64 bits, an `and` at other powers of two, a
//!   `udiv` + `msub` at any other width. A `ctlz`/`cttz`/`abs` whose
//!   poison flag is set is lowered as if it were clear.
//! - **Memory**: a constant length of at most [`INLINE_MAX`] bytes
//!   (`memmove`: [`INLINE_MOVE_MAX`]) is copied or set inline, touching
//!   exactly the bytes of the range; anything else calls `memcpy`,
//!   `memmove` or `memset` (`hod_cg::RuntimeFn`), with a length narrower
//!   than 64 bits zero-extended. The `.inline` forms never call: a
//!   longer constant length is a loop of 16-byte accesses, a variable
//!   one [`INLINE_LENGTH`]. A volatile access is lowered as the plain one.
//! - **Traps** are `brk` with `llc`'s immediates: `#1`, `#0xf000` for
//!   `debugtrap`, `#0x5500 | kind` for `ubsantrap`.
//! - **Frame**: `frameaddress(n)` follows `n` frame records from `x29`;
//!   `returnaddress(n)` loads the saved `lr` of that record (every hod
//!   function saves `lr` in its frame record, leaves included).
//! - **Folded**: `objectsize` is `-1` (`min` false) or `0`; `is.constant`
//!   is false; `prefetch` emits nothing; `ptrmask` is an `and`.

use hod_cg::{CallArg, CgResult, Class, FnCx, RuntimeFn};
use hod_ir::{Const, Inst, Intrinsic, TypeKind, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{Arr, Cond, Load, MemSize, Reg, RegZr, Shift, Size, VMemSize, VecBits};

use crate::common::{
    Cursor, FP, V30, V31, X16, X17, branch_cond, copy_bytes, deferred_first, load_fpr_at,
    load_gpr_at, mov_imm, put, size_for_bits, store_fpr_at, store_gpr_at, unsupported, value_ty,
    xr,
};
use crate::target::A64;

/// An intrinsic call this module does not lower: the floating-point math
/// intrinsics, the saturating arithmetic, `scmp`/`ucmp`, `clmul`, the
/// vector reductions, coroutines, and the others no package has planned.
pub const INTRINSIC: &str = "a64:intrinsic";
/// An integer intrinsic of a type wider than 64 bits (`i128`, class
/// `Mem`): its value class is WP-040's.
pub const WIDE: &str = "a64:intrinsic-wide";
/// An integer intrinsic of a vector type.
pub const VECTOR: &str = "a64:intrinsic-vector";
/// `llvm.memcpy.inline` or `llvm.memset.inline` with a length that is
/// not a constant: the `.inline` forms may not call a function, and a
/// loop of unknown count is not written yet.
pub const INLINE_LENGTH: &str = "a64:mem-inline-length";
/// `llvm.frameaddress` or `llvm.returnaddress` of a depth above
/// [`MAX_DEPTH`]: one load per frame record would be emitted.
pub const FRAME_DEPTH: &str = "a64:frame-depth";

/// The deepest frame record `frameaddress`/`returnaddress` follow.
pub const MAX_DEPTH: u64 = 256;

/// The longest constant length `memcpy` and `memset` copy or set inline:
/// at most four 16-byte accesses and the pieces of the rest, against a
/// call that displaces every live register.
pub const INLINE_MAX: u64 = 64;
/// The longest constant length `memmove` copies inline: every byte is
/// read before any is written, into the scratch registers (two 16-byte
/// vector registers at most).
pub const INLINE_MOVE_MAX: u64 = 32;

/// A call of intrinsic `which` with arguments `args`.
///
/// # Errors
/// [`INTRINSIC`], [`WIDE`], [`VECTOR`], [`INLINE_LENGTH`], [`FRAME_DEPTH`];
/// what placing
/// the operands and the runtime call report.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    match (which, args) {
        (I::Ctlz | I::Cttz | I::Ctpop | I::Bswap | I::BitReverse | I::Abs, &[x, ..]) => {
            let bits = int_type(cx, inst)?;
            deferred_first(cx, inst, None)?;
            unary(cx, inst, which, x, bits)
        }
        (I::Smax | I::Smin | I::Umax | I::Umin, &[a, b]) => {
            let bits = int_type(cx, inst)?;
            deferred_first(cx, inst, None)?;
            min_max(cx, inst, which, a, b, bits)
        }
        (I::Fshl | I::Fshr, &[a, b, c]) => {
            let bits = int_type(cx, inst)?;
            deferred_first(cx, inst, None)?;
            funnel(cx, inst, which == I::Fshl, [a, b, c], bits)
        }
        (I::Memcpy | I::MemcpyInline | I::Memmove, &[dst, src, len, _volatile]) => {
            deferred_first(cx, inst, None)?;
            copy(cx, which, dst, src, len)
        }
        (I::Memset | I::MemsetInline, &[dst, byte, len, _volatile]) => {
            deferred_first(cx, inst, None)?;
            set(cx, which == I::MemsetInline, dst, byte, len)
        }
        (I::Trap, []) => {
            put(cx, mc::brk(1));
            Ok(())
        }
        (I::DebugTrap, []) => {
            put(cx, mc::brk(0xf000));
            Ok(())
        }
        (I::UbsanTrap, &[kind]) => {
            let kind = const_int(cx, kind).unwrap_or(0) as u16 & 0xff;
            put(cx, mc::brk(0x5500 | kind));
            Ok(())
        }
        (I::FrameAddress | I::ReturnAddress, &[depth]) => {
            deferred_first(cx, inst, None)?;
            frame_walk(cx, inst, which == I::ReturnAddress, depth)
        }
        (I::ObjectSize, &[_, min, ..]) => {
            deferred_first(cx, inst, None)?;
            let bits = int_type(cx, inst)?;
            let value = if const_int(cx, min).unwrap_or(0) & 1 == 1 { 0 } else { u64::MAX };
            let rd = cx.def_reg(inst)?;
            mov_imm(cx, size_for_bits(bits), xr(rd), value);
            cx.finish(inst, rd)
        }
        (I::IsConstant, _) => {
            // At -O0 nothing is known to be a constant (LangRef: the
            // result may be false for anything).
            let rd = cx.def_reg(inst)?;
            mov_imm(cx, Size::W, xr(rd), 0);
            cx.finish(inst, rd)
        }
        // A hint: nothing to do.
        (I::Prefetch, _) => Ok(()),
        (I::Ptrmask, &[ptr, mask]) => {
            if cx.class_of(ptr) != Class::Gpr || cx.class_of(mask) != Class::Gpr {
                return unsupported(VECTOR);
            }
            deferred_first(cx, inst, None)?;
            let p = cx.use_reg(ptr)?;
            let m = cx.use_reg(mask)?;
            // The mask is as wide as the pointer's index (64 bits).
            put(cx, mc::and(Size::X, xr(p).zr(), xr(p).zr(), xr(m).zr()));
            cx.finish(inst, p)
        }
        _ => unsupported(INTRINSIC),
    }
}

// ---- operands ----

/// The integer `value` is, zero-extended from its width, when it is an
/// integer constant (or a zero-like one).
fn const_int(cx: &FnCx<'_, A64>, value: Value) -> Option<u64> {
    let id = value.as_const()?;
    match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(value),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// The width of the integer result of `inst`, or the refusal for a type
/// that is not an integer of class `Gpr`.
fn int_type(cx: &FnCx<'_, A64>, inst: Inst<'_>) -> CgResult<u32> {
    match cx.types().kind(inst.ty()) {
        TypeKind::Int(bits) if (1..=64).contains(&bits) => Ok(bits),
        TypeKind::Int(_) => unsupported(WIDE),
        _ => unsupported(VECTOR),
    }
}

/// The width of the register a value of `bits` bits lives in.
#[inline]
fn reg_bits(bits: u32) -> u32 {
    if bits <= 32 { 32 } else { 64 }
}

/// Zero-extends the low `bits` bits of `r` in place.
fn zero_extend(cx: &mut FnCx<'_, A64>, r: Reg, bits: u32) {
    if bits != reg_bits(bits) {
        put(cx, mc::ubfx(size_for_bits(bits), r.zr(), r.zr(), 0, bits));
    }
}

/// Sign-extends the low `bits` bits of `r` in place.
fn sign_extend(cx: &mut FnCx<'_, A64>, r: Reg, bits: u32) {
    if bits != reg_bits(bits) {
        put(cx, mc::sbfx(size_for_bits(bits), r.zr(), r.zr(), 0, bits));
    }
}

// ---- bit manipulation ----

/// `ctlz`, `cttz`, `ctpop`, `bswap`, `bitreverse`, `abs` of `x`, computed
/// in its register.
fn unary<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    x: Value,
    bits: u32,
) -> CgResult<()> {
    use Intrinsic as I;
    if which == I::Bswap && bits % 16 != 0 {
        // The verifier rejects it: an odd number of bytes.
        return unsupported(INTRINSIC);
    }
    let sz = size_for_bits(bits);
    let rb = reg_bits(bits);
    let r = cx.use_reg(x)?;
    let rr = xr(r).zr();
    match which {
        I::Ctlz => {
            if bits != rb {
                // To the top, with the bit below the value set: a zero
                // value counts `bits`.
                let s = rb - bits;
                put(cx, mc::lsl_imm(sz, rr, rr, s));
                put(cx, mc::orr_imm(sz, xr(r).sp(), rr, 1 << (s - 1)));
            }
            put(cx, mc::clz(sz, rr, rr));
        }
        I::Cttz => {
            if bits != rb {
                put(cx, mc::orr_imm(sz, xr(r).sp(), rr, 1 << bits));
            }
            put(cx, mc::rbit(sz, rr, rr));
            put(cx, mc::clz(sz, rr, rr));
        }
        I::Ctpop => {
            zero_extend(cx, xr(r), bits);
            put(cx, mc::fmov_gpr_to_fp(sz, V31, rr));
            put(cx, mc::cnt(VecBits::V64, V31, V31));
            put(cx, mc::uaddlv(Arr::B8, V31, V31));
            put(cx, mc::fmov_fp_to_gpr(Size::W, rr, V31));
        }
        I::Bswap => {
            if bits == 16 {
                put(cx, mc::rev16(Size::W, rr, rr));
            } else {
                put(cx, mc::rev(sz, rr, rr));
                if bits != rb {
                    put(cx, mc::lsr_imm(sz, rr, rr, rb - bits));
                }
            }
        }
        I::BitReverse => {
            put(cx, mc::rbit(sz, rr, rr));
            if bits != rb {
                put(cx, mc::lsr_imm(sz, rr, rr, rb - bits));
            }
        }
        I::Abs => {
            // The smallest value is its own negation at the width.
            sign_extend(cx, xr(r), bits);
            put(cx, mc::cmp_imm(sz, xr(r).sp(), 0));
            put(cx, mc::cneg(sz, rr, rr, Cond::Mi));
        }
        _ => return cx.bug("an integer intrinsic without a lowering"),
    }
    cx.finish(inst, r)
}

/// `smax`, `smin`, `umax`, `umin`: both operands extended, `cmp`,
/// `csel`.
fn min_max<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    a: Value,
    b: Value,
    bits: u32,
) -> CgResult<()> {
    let (signed, cond) = match which {
        Intrinsic::Smax => (true, Cond::Gt),
        Intrinsic::Smin => (true, Cond::Lt),
        Intrinsic::Umax => (false, Cond::Hi),
        _ => (false, Cond::Lo),
    };
    let sz = size_for_bits(bits);
    let ra = cx.use_reg(a)?;
    let rb = cx.use_reg(b)?;
    let (x, y) = (xr(ra), xr(rb));
    if signed {
        sign_extend(cx, x, bits);
        sign_extend(cx, y, bits);
    } else {
        zero_extend(cx, x, bits);
        zero_extend(cx, y, bits);
    }
    put(cx, mc::cmp(sz, x.zr(), y.zr()));
    put(cx, mc::csel(sz, x.zr(), x.zr(), y.zr(), cond));
    cx.finish(inst, ra)
}

/// `fshl` (`left`) or `fshr` of `[a, b, c]`: the concatenation `a:b`
/// shifted by `c` modulo the width, its high (`fshl`) or low half.
fn funnel<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    left: bool,
    [a, b, c]: [Value; 3],
    bits: u32,
) -> CgResult<()> {
    let sz = size_for_bits(bits);
    let rb = reg_bits(bits);
    // At one bit every amount is 0 modulo the width.
    if let Some(amount) = const_int(cx, c).or((bits == 1).then_some(0)) {
        let k = (amount % u64::from(bits)) as u32;
        if k == 0 {
            let r = cx.use_reg(if left { a } else { b })?;
            return cx.finish(inst, r);
        }
        let ra = cx.use_reg(a)?;
        let rbv = cx.use_reg(b)?;
        let (x, y) = (xr(ra).zr(), xr(rbv).zr());
        if bits == rb {
            // `extr` is the `rb`-bit field of `a:b` at its `lsb`.
            put(cx, mc::extr(sz, x, x, y, if left { rb - k } else { k }));
            return cx.finish(inst, ra);
        }
        if left {
            // (b >> (N - k)) | (a << k).
            put(cx, mc::ubfx(sz, y, y, bits - k, k));
            put(cx, mc::orr_shifted(sz, x, y, x, Shift::Lsl, k));
            return cx.finish(inst, ra);
        }
        // (b >> k) | (a << (N - k)).
        put(cx, mc::ubfx(sz, y, y, k, bits - k));
        put(cx, mc::orr_shifted(sz, y, y, x, Shift::Lsl, bits - k));
        return cx.finish(inst, rbv);
    }
    let ra = cx.use_reg(a)?;
    let rbv = cx.use_reg(b)?;
    let rc = cx.use_reg(c)?;
    let (x, y, z) = (xr(ra).zr(), xr(rbv).zr(), xr(rc).zr());
    if bits == rb {
        // The register shifts take their amount modulo `rb`; the other
        // half moves by one, then by the complement: `rb - k` without a
        // shift by `rb` when `k` is 0.
        if left {
            put(cx, mc::lslv(sz, x, x, z));
            put(cx, mc::mvn(sz, z, z));
            put(cx, mc::lsr_imm(sz, y, y, 1));
            put(cx, mc::lsrv(sz, y, y, z));
            put(cx, mc::orr(sz, x, x, y));
            return cx.finish(inst, ra);
        }
        put(cx, mc::lsrv(sz, y, y, z));
        put(cx, mc::mvn(sz, z, z));
        put(cx, mc::lsl_imm(sz, x, x, 1));
        put(cx, mc::lslv(sz, x, x, z));
        put(cx, mc::orr(sz, y, y, x));
        return cx.finish(inst, rbv);
    }
    // k = c mod N, in c's register; x16 holds N.
    zero_extend(cx, xr(rbv), bits);
    if bits.is_power_of_two() {
        put(cx, mc::and_imm(sz, xr(rc).sp(), z, u64::from(bits - 1)));
        mov_imm(cx, sz, X16, u64::from(bits));
    } else {
        zero_extend(cx, xr(rc), bits);
        mov_imm(cx, sz, X16, u64::from(bits));
        put(cx, mc::udiv(sz, X17.zr(), z, X16.zr()));
        put(cx, mc::msub(sz, z, X17.zr(), X16.zr(), z));
    }
    if left {
        // (a << k) | (b >> (N - k)); for k = 0 the shift by N gives 0.
        put(cx, mc::lslv(sz, x, x, z));
        put(cx, mc::sub(sz, z, X16.zr(), z));
        put(cx, mc::lsrv(sz, y, y, z));
        put(cx, mc::orr(sz, x, x, y));
        return cx.finish(inst, ra);
    }
    // (b >> k) | (a << (N - k)); for k = 0 the bits of a land above N.
    put(cx, mc::lsrv(sz, y, y, z));
    put(cx, mc::sub(sz, z, X16.zr(), z));
    put(cx, mc::lslv(sz, x, x, z));
    put(cx, mc::orr(sz, y, y, x));
    cx.finish(inst, rbv)
}

// ---- memory ----

/// The length of a memory intrinsic as a runtime argument: `size_t`,
/// zero-extended from a narrower type into a temporary.
fn length_arg(cx: &mut FnCx<'_, A64>, len: Value) -> CgResult<CallArg> {
    let bits = cx.types().int_bits(value_ty(cx, len)).unwrap_or(64);
    if bits == 64 || const_int(cx, len).is_some() {
        // A constant is materialized zero-extended.
        return Ok(CallArg::Value(len));
    }
    let r = cx.use_reg(len)?;
    let t = cx.temp(Class::Gpr)?;
    if bits == 32 {
        put(cx, mc::mov(Size::W, xr(t).zr(), xr(r).zr()));
    } else {
        put(cx, mc::ubfx(Size::X, xr(t).zr(), xr(r).zr(), 0, bits));
    }
    cx.release(len);
    cx.free_temp(r);
    Ok(CallArg::Reg(t))
}

/// `memcpy`, `memcpy.inline`, `memmove` of `len` bytes from `src` to
/// `dst`.
fn copy<'a>(
    cx: &mut FnCx<'a, A64>,
    which: Intrinsic<'a>,
    dst: Value,
    src: Value,
    len: Value,
) -> CgResult<()> {
    let moving = which == Intrinsic::Memmove;
    let Some(bytes) = const_int(cx, len) else {
        if which == Intrinsic::MemcpyInline {
            return unsupported(INLINE_LENGTH);
        }
        let func = if moving { RuntimeFn::Memmove } else { RuntimeFn::Memcpy };
        let len = length_arg(cx, len)?;
        cx.call_runtime(func, &[CallArg::Value(dst), CallArg::Value(src), len], None)?;
        return Ok(());
    };
    if bytes == 0 {
        return Ok(());
    }
    let limit = if moving { INLINE_MOVE_MAX } else { INLINE_MAX };
    if bytes > limit && which != Intrinsic::MemcpyInline {
        let func = if moving { RuntimeFn::Memmove } else { RuntimeFn::Memcpy };
        cx.call_runtime(
            func,
            &[CallArg::Value(dst), CallArg::Value(src), CallArg::Value(len)],
            None,
        )?;
        return Ok(());
    }
    let d = xr(cx.use_reg(dst)?);
    let s = xr(cx.use_reg(src)?);
    if moving {
        move_bytes(cx, d, s, bytes as u32);
    } else if bytes <= INLINE_MAX {
        copy_bytes(
            cx,
            &mut Cursor::at_reg(d, 0, X16),
            &mut Cursor::at_reg(s, 0, X17),
            bytes as u32,
        );
    } else {
        copy_loop(cx, d, s, bytes)?;
    }
    Ok(())
}

/// Copies `bytes` (1 to [`INLINE_MOVE_MAX`]) bytes from `[s]` to `[d]`,
/// all of them read before any is written: two accesses of the largest
/// size that fits, at the start and at the end of the range (they
/// overlap when `bytes` is not that size).
fn move_bytes(cx: &mut FnCx<'_, A64>, d: Reg, s: Reg, bytes: u32) {
    let size = if bytes >= 16 {
        16
    } else if bytes >= 8 {
        8
    } else if bytes >= 4 {
        4
    } else if bytes >= 2 {
        2
    } else {
        1
    };
    let last = i64::from(bytes - size);
    let at = |reg: Reg, off: i64| Cursor::at_reg(reg, off, X16);
    if size == 16 {
        load_fpr_at(cx, VMemSize::Q, V30, &mut at(s, 0));
        load_fpr_at(cx, VMemSize::Q, V31, &mut at(s, last));
        store_fpr_at(cx, VMemSize::Q, V30, &mut at(d, 0));
        store_fpr_at(cx, VMemSize::Q, V31, &mut at(d, last));
        return;
    }
    let (op, ms) = match size {
        8 => (Load::X, MemSize::X),
        4 => (Load::W, MemSize::W),
        2 => (Load::H, MemSize::H),
        _ => (Load::B, MemSize::B),
    };
    load_gpr_at(cx, op, X16, &mut at(s, 0));
    if last != 0 {
        load_gpr_at(cx, op, X17, &mut at(s, last));
    }
    store_gpr_at(cx, ms, X16.zr(), &mut at(d, 0));
    if last != 0 {
        store_gpr_at(cx, ms, X17.zr(), &mut at(d, last));
    }
}

/// Copies `bytes` (more than 16) bytes from `[s]` to `[d]` by a loop of
/// 16-byte accesses that moves both registers, then the last 16 bytes of
/// the range again if `bytes` is not a multiple of 16.
fn copy_loop(cx: &mut FnCx<'_, A64>, d: Reg, s: Reg, bytes: u64) -> CgResult<()> {
    mov_imm(cx, Size::X, X16, bytes / 16);
    let top = cx.new_label();
    cx.bind(top)?;
    put(cx, mc::ldr_fp_post(VMemSize::Q, V31, s.sp(), 16));
    put(cx, mc::str_fp_post(VMemSize::Q, V31, d.sp(), 16));
    put(cx, mc::subs_imm(Size::X, X16.zr(), X16.sp(), 1));
    branch_cond(cx, Cond::Ne, top)?;
    let rest = (bytes % 16) as i64;
    if rest != 0 {
        put(cx, mc::ldur_fp(VMemSize::Q, V31, s.sp(), rest - 16));
        put(cx, mc::stur_fp(VMemSize::Q, V31, d.sp(), rest - 16));
    }
    Ok(())
}

/// `memset` or `memset.inline` (`inline`) of `len` bytes at `dst` to
/// `byte`.
fn set(cx: &mut FnCx<'_, A64>, inline: bool, dst: Value, byte: Value, len: Value) -> CgResult<()> {
    let Some(bytes) = const_int(cx, len) else {
        if inline {
            return unsupported(INLINE_LENGTH);
        }
        let len = length_arg(cx, len)?;
        cx.call_runtime(
            RuntimeFn::Memset,
            &[CallArg::Value(dst), CallArg::Value(byte), len],
            None,
        )?;
        return Ok(());
    };
    if bytes == 0 {
        return Ok(());
    }
    if bytes > INLINE_MAX && !inline {
        cx.call_runtime(
            RuntimeFn::Memset,
            &[CallArg::Value(dst), CallArg::Value(byte), CallArg::Value(len)],
            None,
        )?;
        return Ok(());
    }
    let d = xr(cx.use_reg(dst)?);
    // The byte in every byte of x17 (or xzr for zero).
    let pattern = match const_int(cx, byte) {
        Some(value) if value & 0xff == 0 => RegZr::ZR,
        Some(value) => {
            mov_imm(cx, Size::X, X17, (value & 0xff) * 0x0101_0101_0101_0101);
            X17.zr()
        }
        None => {
            let b = xr(cx.use_reg(byte)?);
            put(cx, mc::and_imm(Size::X, X17.sp(), b.zr(), 0xff));
            mov_imm(cx, Size::X, X16, 0x0101_0101_0101_0101);
            put(cx, mc::mul(Size::X, X17.zr(), X17.zr(), X16.zr()));
            X17.zr()
        }
    };
    if bytes <= INLINE_MAX {
        let mut at = Cursor::at_reg(d, 0, X16);
        let mut left = bytes as u32;
        for (size, ms) in [(8, MemSize::X), (4, MemSize::W), (2, MemSize::H), (1, MemSize::B)] {
            while left >= size {
                store_gpr_at(cx, ms, pattern, &mut at);
                at.advance(size);
                left -= size;
            }
        }
        return Ok(());
    }
    if pattern == RegZr::ZR {
        put(cx, mc::movi_mask(VecBits::V128, V31, 0));
    } else {
        put(cx, mc::dup_gpr(Arr::B16, V31, pattern));
    }
    mov_imm(cx, Size::X, X16, bytes / 16);
    let top = cx.new_label();
    cx.bind(top)?;
    put(cx, mc::str_fp_post(VMemSize::Q, V31, d.sp(), 16));
    put(cx, mc::subs_imm(Size::X, X16.zr(), X16.sp(), 1));
    branch_cond(cx, Cond::Ne, top)?;
    let rest = (bytes % 16) as i64;
    if rest != 0 {
        put(cx, mc::stur_fp(VMemSize::Q, V31, d.sp(), rest - 16));
    }
    Ok(())
}

// ---- the frame ----

/// `frameaddress(depth)` or `returnaddress(depth)` (`ret`): `x29`, then
/// `depth` saved frame pointers loaded from the chain of frame records,
/// then for `ret` the saved `lr` of the last record (`[x, #8]`), as
/// `llc` walks it.
fn frame_walk<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, ret: bool, depth: Value) -> CgResult<()> {
    let depth = const_int(cx, depth).unwrap_or(0) & 0xffff_ffff;
    if depth > MAX_DEPTH {
        return unsupported(FRAME_DEPTH);
    }
    let rd = cx.def_reg(inst)?;
    let r = xr(rd);
    let mut base = FP;
    for _ in 0..depth {
        put(cx, mc::ldr_off(Load::X, r.zr(), base, 0));
        base = r.sp();
    }
    if ret {
        put(cx, mc::ldr_off(Load::X, r.zr(), base, 8));
    } else if depth == 0 {
        put(cx, mc::mov_sp(Size::X, r.sp(), FP));
    }
    cx.finish(inst, rd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_widths() {
        assert_eq!(reg_bits(1), 32);
        assert_eq!(reg_bits(32), 32);
        assert_eq!(reg_bits(33), 64);
        assert_eq!(reg_bits(64), 64);
    }

    /// The pattern of a non-zero byte fills every byte.
    #[test]
    fn byte_patterns() {
        assert_eq!(0xabu64 * 0x0101_0101_0101_0101, 0xabab_abab_abab_abab);
    }
}
