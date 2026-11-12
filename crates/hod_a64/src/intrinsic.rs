//! Intrinsic calls (`spec/ir.md` §5.5) other than those the dispatch
//! routes to `call` (`llvm.va_*`), `mem` (`llvm.stacksave`,
//! `llvm.stackrestore`) and `aggr` (`*.with.overflow`): memory
//! intrinsics, bit manipulation, `abs` and min/max, traps, the frame
//! intrinsics, and those that fold to a constant or to nothing.
//!
//! **Owner: WP-041; the second set WP-070** (the saturating arithmetic,
//! `scmp`/`ucmp`, the `i128` forms, `eh.typeid.for`, `clear_cache`;
//! the floating-point intrinsics are `fpintr`'s, dispatched from here).
//! Tests: `tests/intrinsic.rs`. The dispatch in
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
//!   is true for a literal constant operand, false otherwise (as `llc`);
//!   `prefetch` emits nothing; `ptrmask` is an `and`.
//! - **Saturating** `{s,u}{add,sub}.sat` (WP-070): at 32 and 64 bits the
//!   flags of `adds`/`subs` pick the bound (`csel`/`csinv`); narrower
//!   widths compute exactly in 64 bits from extended operands and clamp
//!   by `cmp` + `csel`. `scmp`/`ucmp`: `cmp`, `cset gt|hi`,
//!   `csinv ge|hs`.
//! - **`i128`** (class `Mem`, WP-070): every integer intrinsic above and
//!   the saturating ones work on the two limbs of the slots through
//!   `x16`/`x17` and at most four temporaries; a wider width is
//!   [`WIDE`].

use hod_cg::{CallArg, CgResult, Class, FnCx, FrameRef, RuntimeFn};
use hod_ir::{Const, Inst, Intrinsic, TypeKind, Value, ValueKind};
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

/// `llvm.readcyclecounter`: not refused any more (`tail`, WP-084); kept
/// for the coverage log's history.
pub const READCYCLECOUNTER: &str = "a64:readcyclecounter";
/// `llvm.eh.typeid.for` of something that is not a symbol (WP-070).
pub const TYPEID: &str = "a64:eh-typeid";

/// A call of intrinsic `which` with arguments `args`.
///
/// # Errors
/// [`INTRINSIC`], [`WIDE`], [`VECTOR`], [`INLINE_LENGTH`], [`FRAME_DEPTH`];
/// what placing
/// the operands and the runtime call report.
#[allow(clippy::too_many_lines, reason = "one arm per intrinsic")]
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    if let Some(done) = crate::lanes::intrinsic(cx, inst, which, args) {
        return done;
    }
    if crate::fpintr::is_fp(which) {
        return crate::fpintr::fp_intrinsic(cx, inst, which, args);
    }
    match (which, args) {
        (I::Ctlz | I::Cttz | I::Ctpop | I::Bswap | I::BitReverse | I::Abs, &[x, ..]) => {
            let Some(bits) = int_or_quad(cx, inst)? else {
                deferred_first(cx, inst, None)?;
                return wide_unary(cx, inst, which, x);
            };
            deferred_first(cx, inst, None)?;
            unary(cx, inst, which, x, bits)
        }
        (I::Smax | I::Smin | I::Umax | I::Umin, &[a, b]) => {
            let Some(bits) = int_or_quad(cx, inst)? else {
                deferred_first(cx, inst, None)?;
                return wide_two(cx, inst, which, a, b);
            };
            deferred_first(cx, inst, None)?;
            min_max(cx, inst, which, a, b, bits)
        }
        (I::SaddSat | I::UaddSat | I::SsubSat | I::UsubSat, &[a, b]) => {
            let Some(bits) = int_or_quad(cx, inst)? else {
                deferred_first(cx, inst, None)?;
                return wide_two(cx, inst, which, a, b);
            };
            deferred_first(cx, inst, None)?;
            saturating(cx, inst, which, a, b, bits)
        }
        (I::Scmp | I::Ucmp, &[a, b]) => {
            deferred_first(cx, inst, None)?;
            three_way(cx, inst, which == I::Scmp, a, b)
        }
        (I::Fshl | I::Fshr, &[a, b, c]) => {
            let Some(bits) = int_or_quad(cx, inst)? else {
                deferred_first(cx, inst, None)?;
                return wide_funnel(cx, inst, which == I::Fshl, [a, b, c]);
            };
            deferred_first(cx, inst, None)?;
            funnel(cx, inst, which == I::Fshl, [a, b, c], bits)
        }
        (I::EhTypeidFor, &[info]) => {
            let ValueKind::Sym(sym) = info.kind() else { return unsupported(TYPEID) };
            deferred_first(cx, inst, None)?;
            let id = cx.type_id(Some(sym));
            let rd = cx.def_reg(inst)?;
            mov_imm(cx, Size::W, xr(rd), u64::from(id));
            cx.finish(inst, rd)
        }
        (I::ClearCache, &[start, end]) => {
            deferred_first(cx, inst, None)?;
            cx.call_runtime(
                RuntimeFn::ClearCache,
                &[CallArg::Value(start), CallArg::Value(end)],
                None,
            )?;
            Ok(())
        }
        (I::ReadCycleCounter, _) => crate::tail::read_cycle_counter(cx, inst),
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
        (I::IsConstant, &[x]) => {
            // As `llc`'s `LowerConstantIntrinsics` (`tail`, WP-084).
            let manifest = crate::tail::is_manifest(cx, x);
            let rd = cx.def_reg(inst)?;
            mov_imm(cx, Size::W, xr(rd), u64::from(manifest));
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
        (
            I::SshlSat
            | I::UshlSat
            | I::Aarch64Crc32b
            | I::Aarch64Crc32h
            | I::Aarch64Crc32w
            | I::Aarch64Crc32x
            | I::Aarch64Crc32cb
            | I::Aarch64Crc32ch
            | I::Aarch64Crc32cw
            | I::Aarch64Crc32cx,
            _,
        ) => crate::tail::intrinsic(cx, inst, which, args),
        _ => unsupported(INTRINSIC),
    }
}

// ---- operands ----

/// The integer `value` is, zero-extended from its width, when it is an
/// integer constant (or a zero-like one).
pub(crate) fn const_int(cx: &FnCx<'_, A64>, value: Value) -> Option<u64> {
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

/// The width of the integer result of `inst` (`Some`, class `Gpr`), or
/// `None` for `i128` (class `Mem`, WP-070); the refusal for any other
/// type.
fn int_or_quad(cx: &FnCx<'_, A64>, inst: Inst<'_>) -> CgResult<Option<u32>> {
    match cx.types().kind(inst.ty()) {
        TypeKind::Int(bits) if (1..=64).contains(&bits) => Ok(Some(bits)),
        TypeKind::Int(128) => Ok(None),
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
    if which == Intrinsic::Bswap && !bits.is_multiple_of(16) {
        // The verifier rejects it: an odd number of bytes.
        return unsupported(INTRINSIC);
    }
    let r = cx.use_reg(x)?;
    lane_unary(cx, which, xr(r), bits)?;
    cx.finish(inst, r)
}

/// The register part of [`unary`], and one lane of the element-wise
/// form (`lanes.rs`, WP-080): `ctlz`, `cttz`, `ctpop`, `bswap`,
/// `bitreverse` or `abs` of the `bits`-bit value in `r`, in place.
/// Writes `v31` for `ctpop`.
///
/// # Errors
/// [`INTRINSIC`] for `bswap` of an odd number of bytes; `Internal` for
/// another intrinsic.
pub fn lane_unary(cx: &mut FnCx<'_, A64>, which: Intrinsic<'_>, r: Reg, bits: u32) -> CgResult<()> {
    use Intrinsic as I;
    if which == I::Bswap && !bits.is_multiple_of(16) {
        return unsupported(INTRINSIC);
    }
    let sz = size_for_bits(bits);
    let rb = reg_bits(bits);
    let rr = r.zr();
    match which {
        I::Ctlz => {
            if bits != rb {
                // To the top, with the bit below the value set: a zero
                // value counts `bits`.
                let s = rb - bits;
                put(cx, mc::lsl_imm(sz, rr, rr, s));
                put(cx, mc::orr_imm(sz, r.sp(), rr, 1 << (s - 1)));
            }
            put(cx, mc::clz(sz, rr, rr));
        }
        I::Cttz => {
            if bits != rb {
                put(cx, mc::orr_imm(sz, r.sp(), rr, 1 << bits));
            }
            put(cx, mc::rbit(sz, rr, rr));
            put(cx, mc::clz(sz, rr, rr));
        }
        I::Ctpop => {
            zero_extend(cx, r, bits);
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
            sign_extend(cx, r, bits);
            put(cx, mc::cmp_imm(sz, r.sp(), 0));
            put(cx, mc::cneg(sz, rr, rr, Cond::Mi));
        }
        _ => return cx.bug("an integer intrinsic without a lowering"),
    }
    Ok(())
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
    let ra = cx.use_reg(a)?;
    let rb = cx.use_reg(b)?;
    lane_min_max(cx, which, xr(ra), xr(rb), bits);
    cx.finish(inst, ra)
}

/// The register part of [`min_max`], and one lane of the element-wise
/// form (`lanes.rs`, WP-080): `x = smax|smin|umax|umin(x, y)` at `bits`
/// bits. Writes `x` and `y`.
pub fn lane_min_max(cx: &mut FnCx<'_, A64>, which: Intrinsic<'_>, x: Reg, y: Reg, bits: u32) {
    let (signed, cond) = match which {
        Intrinsic::Smax => (true, Cond::Gt),
        Intrinsic::Smin => (true, Cond::Lt),
        Intrinsic::Umax => (false, Cond::Hi),
        _ => (false, Cond::Lo),
    };
    let sz = size_for_bits(bits);
    if signed {
        sign_extend(cx, x, bits);
        sign_extend(cx, y, bits);
    } else {
        zero_extend(cx, x, bits);
        zero_extend(cx, y, bits);
    }
    put(cx, mc::cmp(sz, x.zr(), y.zr()));
    put(cx, mc::csel(sz, x.zr(), x.zr(), y.zr(), cond));
}

/// `fshl` (`left`) or `fshr` of `[a, b, c]`: the concatenation `a:b`
/// shifted by `c` modulo the width, its high (`fshl`) or low half.
#[allow(clippy::many_single_char_names, reason = "the operands are LangRef's a, b, c")]
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
    let left_result = lane_funnel(cx, left, [xr(ra), xr(rbv), xr(rc)], bits);
    cx.finish(inst, if left_result { ra } else { rbv })
}

/// The register part of [`funnel`] for an amount in a register, and one
/// lane of the element-wise form (`lanes.rs`, WP-080): `fshl` (`left`) or
/// `fshr` of the `bits`-bit values in `[a, b, c]`. The result is in `a`
/// when this returns `true`, else in `b`. Writes all three, `x16` and
/// `x17`.
#[allow(clippy::many_single_char_names, reason = "the operands are LangRef's a, b, c")]
pub fn lane_funnel(cx: &mut FnCx<'_, A64>, left: bool, [a, b, c]: [Reg; 3], bits: u32) -> bool {
    let sz = size_for_bits(bits);
    let rb = reg_bits(bits);
    let (x, y, z) = (a.zr(), b.zr(), c.zr());
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
            return true;
        }
        put(cx, mc::lsrv(sz, y, y, z));
        put(cx, mc::mvn(sz, z, z));
        put(cx, mc::lsl_imm(sz, x, x, 1));
        put(cx, mc::lslv(sz, x, x, z));
        put(cx, mc::orr(sz, y, y, x));
        return false;
    }
    // k = c mod N, in c's register; x16 holds N.
    zero_extend(cx, b, bits);
    if bits.is_power_of_two() {
        put(cx, mc::and_imm(sz, c.sp(), z, u64::from(bits - 1)));
        mov_imm(cx, sz, X16, u64::from(bits));
    } else {
        zero_extend(cx, c, bits);
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
        return true;
    }
    // (b >> k) | (a << (N - k)); for k = 0 the bits of a land above N.
    put(cx, mc::lsrv(sz, y, y, z));
    put(cx, mc::sub(sz, z, X16.zr(), z));
    put(cx, mc::lslv(sz, x, x, z));
    put(cx, mc::orr(sz, y, y, x));
    false
}

// ---- saturating arithmetic and three-way compares (WP-070) ----

/// `{s,u}{add,sub}.sat` of `a` and `b` at `bits` (1 to 64) bits.
///
/// At 32 and 64 bits `adds`/`subs` set the flags: a signed overflow
/// (`vs`) gives `(r >> (N-1)) ^ (1 << (N-1))` (the wrapped result's sign
/// is the opposite of the true one), an unsigned add's carry all ones
/// (`csinv cc`), an unsigned sub's borrow 0 (`csel hs`). Narrower, the
/// operands are extended to 64 bits, where the exact result fits, and it
/// is clamped to the width's range.
#[allow(clippy::many_single_char_names, reason = "the operands a, b; x, y their registers")]
fn saturating<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    a: Value,
    b: Value,
    bits: u32,
) -> CgResult<()> {
    let ra = cx.use_reg(a)?;
    let rb = cx.use_reg(b)?;
    lane_saturating(cx, which, xr(ra), xr(rb), bits);
    cx.finish(inst, ra)
}

/// The register part of [`saturating`], and one lane of the element-wise
/// form (`lanes.rs`, WP-080): `x = {s,u}{add,sub}.sat(x, y)` at `bits`
/// bits. Writes `x`, `y` and `x16`.
#[allow(clippy::many_single_char_names, reason = "x, y the operands' registers")]
pub fn lane_saturating(cx: &mut FnCx<'_, A64>, which: Intrinsic<'_>, x: Reg, y: Reg, bits: u32) {
    let signed = matches!(which, Intrinsic::SaddSat | Intrinsic::SsubSat);
    let add = matches!(which, Intrinsic::SaddSat | Intrinsic::UaddSat);
    let (xz, yz) = (x.zr(), y.zr());
    if bits == 32 || bits == 64 {
        let sz = size_for_bits(bits);
        put(cx, if add { mc::adds(sz, xz, xz, yz) } else { mc::subs(sz, xz, xz, yz) });
        if signed {
            put(cx, mc::asr_imm(sz, X16.zr(), xz, bits - 1));
            put(cx, mc::eor_imm(sz, X16.sp(), X16.zr(), 1 << (bits - 1)));
            put(cx, mc::csel(sz, xz, X16.zr(), xz, Cond::Vs));
        } else if add {
            put(cx, mc::csinv(sz, xz, xz, RegZr::ZR, Cond::Lo));
        } else {
            put(cx, mc::csel(sz, xz, xz, RegZr::ZR, Cond::Hs));
        }
        return;
    }
    let extend = if signed { mc::sbfx } else { mc::ubfx };
    put(cx, extend(Size::X, xz, xz, 0, bits));
    put(cx, extend(Size::X, yz, yz, 0, bits));
    put(cx, if add { mc::add(Size::X, xz, xz, yz) } else { mc::sub(Size::X, xz, xz, yz) });
    if signed {
        let max = (1u64 << (bits - 1)) - 1;
        mov_imm(cx, Size::X, X16, max);
        put(cx, mc::cmp(Size::X, xz, X16.zr()));
        put(cx, mc::csel(Size::X, xz, xz, X16.zr(), Cond::Lt));
        mov_imm(cx, Size::X, X16, !max);
        put(cx, mc::cmp(Size::X, xz, X16.zr()));
        put(cx, mc::csel(Size::X, xz, xz, X16.zr(), Cond::Gt));
    } else if add {
        mov_imm(cx, Size::X, X16, (1u64 << bits) - 1);
        put(cx, mc::cmp(Size::X, xz, X16.zr()));
        put(cx, mc::csel(Size::X, xz, xz, X16.zr(), Cond::Ls));
    } else {
        // Both operands are below 2^63: the difference is negative
        // exactly when it saturates.
        put(cx, mc::cmp_imm(Size::X, x.sp(), 0));
        put(cx, mc::csel(Size::X, xz, xz, RegZr::ZR, Cond::Ge));
    }
}

/// The register part of [`three_way`] for operands of 1 to 64 bits, and
/// one lane of the element-wise form (`lanes.rs`, WP-080): `x =
/// scmp|ucmp(x, y)` (`signed`), -1, 0 or 1 in all 64 bits of `x`. Writes
/// `x` and `y`.
pub fn lane_three_way(cx: &mut FnCx<'_, A64>, signed: bool, x: Reg, y: Reg, operand_bits: u32) {
    let gt = if signed { Cond::Gt } else { Cond::Hi };
    let sz = size_for_bits(operand_bits);
    if signed {
        sign_extend(cx, x, operand_bits);
        sign_extend(cx, y, operand_bits);
    } else {
        zero_extend(cx, x, operand_bits);
        zero_extend(cx, y, operand_bits);
    }
    put(cx, mc::cmp(sz, x.zr(), y.zr()));
    let r = x.zr();
    put(cx, mc::cset(Size::X, r, gt));
    // r = (a >= b) ? r : -1.
    let ge = if signed { Cond::Ge } else { Cond::Hs };
    put(cx, mc::csinv(Size::X, r, r, RegZr::ZR, ge));
}

/// `scmp`/`ucmp` (`signed`) of `a` and `b`: -1, 0 or 1 in the result
/// type, whose width may differ from the operands' (2 to 64 bits, or
/// `i128`). Operands of 1 to 64 bits are extended and compared; `i128`
/// ones are ordered by two `cmp` + `sbcs` chains (which set `N`, `V`
/// and `C` but not `Z` for the whole value).
#[allow(clippy::many_single_char_names, reason = "the operands a, b; x, y their registers")]
fn three_way<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    signed: bool,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let wide_result = match cx.types().kind(inst.ty()) {
        TypeKind::Int(bits) if (2..=64).contains(&bits) => false,
        TypeKind::Int(128) => true,
        TypeKind::Int(_) => return unsupported(WIDE),
        _ => return unsupported(VECTOR),
    };
    let operand_bits = match cx.types().kind(value_ty(cx, a)) {
        TypeKind::Int(bits) if (1..=64).contains(&bits) || bits == 128 => bits,
        TypeKind::Int(_) => return unsupported(WIDE),
        _ => return unsupported(VECTOR),
    };
    let lt = if signed { Cond::Lt } else { Cond::Lo };
    let rd = if operand_bits == 128 {
        let (sa, sb) = (cx.use_mem(a)?, cx.use_mem(b)?);
        let (t1, t2) = (xr(cx.temp(Class::Gpr)?), xr(cx.temp(Class::Gpr)?));
        let rd = cx.temp(Class::Gpr)?;
        let r = xr(rd).zr();
        ld(cx, X16, sa, 0);
        ld(cx, X17, sa, 1);
        ld(cx, t1, sb, 0);
        ld(cx, t2, sb, 1);
        // a < b: -1.
        put(cx, mc::cmp(Size::X, X16.zr(), t1.zr()));
        put(cx, mc::sbcs(Size::X, RegZr::ZR, X17.zr(), t2.zr()));
        put(cx, mc::csetm(Size::X, r, lt));
        // b < a: 1.
        put(cx, mc::cmp(Size::X, t1.zr(), X16.zr()));
        put(cx, mc::sbcs(Size::X, RegZr::ZR, t2.zr(), X17.zr()));
        put(cx, mc::cset(Size::X, X16.zr(), lt));
        put(cx, mc::orr(Size::X, r, r, X16.zr()));
        rd
    } else {
        let ra = cx.use_reg(a)?;
        let rb = cx.use_reg(b)?;
        lane_three_way(cx, signed, xr(ra), xr(rb), operand_bits);
        ra
    };
    if !wide_result {
        return cx.finish(inst, rd);
    }
    let dst = cx.def_mem(inst)?;
    let r = xr(rd);
    st(cx, r.zr(), X16, dst, 0);
    put(cx, mc::asr_imm(Size::X, X17.zr(), r.zr(), 63));
    st(cx, X17.zr(), X16, dst, 1);
    Ok(())
}

// ---- `i128` (class `Mem`, WP-070) ----

/// `slot` moved `off` bytes on.
fn slot_at(slot: FrameRef, off: u32) -> FrameRef {
    match slot {
        FrameRef::Local(o) => FrameRef::Local(o + off),
        FrameRef::Incoming(o) => FrameRef::Incoming(o + off),
        FrameRef::Outgoing(o) => FrameRef::Outgoing(o + off),
    }
}

/// `ldr rt, [slot + 8 * limb]`; a far offset is rebased into `rt`.
fn ld(cx: &mut FnCx<'_, A64>, rt: Reg, slot: FrameRef, limb: u32) {
    let mut c = Cursor::frame(cx, slot_at(slot, 8 * limb), rt);
    load_gpr_at(cx, Load::X, rt, &mut c);
}

/// `str rt, [slot + 8 * limb]`; a far offset is rebased into `tmp`.
fn st(cx: &mut FnCx<'_, A64>, rt: RegZr, tmp: Reg, slot: FrameRef, limb: u32) {
    let mut c = Cursor::frame(cx, slot_at(slot, 8 * limb), tmp);
    store_gpr_at(cx, MemSize::X, rt, &mut c);
}

/// Writes `lo` and `hi` as the two limbs of the `i128` result of `inst`.
fn wide_result<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    lo: RegZr,
    hi: RegZr,
    tmp: Reg,
) -> CgResult<()> {
    let dst = cx.def_mem(inst)?;
    st(cx, lo, tmp, dst, 0);
    st(cx, hi, tmp, dst, 1);
    Ok(())
}

/// `ctlz`, `cttz`, `ctpop`, `bswap`, `bitreverse`, `abs` of an `i128`:
/// the limbs in `x16` (low) and `x17` (high).
fn wide_unary<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    x: Value,
) -> CgResult<()> {
    use Intrinsic as I;
    let src = cx.use_mem(x)?;
    let t = xr(cx.temp(Class::Gpr)?);
    let (lo, hi, tz) = (X16.zr(), X17.zr(), t.zr());
    ld(cx, X16, src, 0);
    ld(cx, X17, src, 1);
    match which {
        I::Ctlz => {
            // clz(hi), or 64 + clz(lo) when hi is 0.
            put(cx, mc::clz(Size::X, tz, hi));
            put(cx, mc::clz(Size::X, lo, lo));
            put(cx, mc::add_imm(Size::X, X16.sp(), X16.sp(), 64));
            put(cx, mc::cmp_imm(Size::X, X17.sp(), 0));
            put(cx, mc::csel(Size::X, tz, tz, lo, Cond::Ne));
            wide_result(cx, inst, tz, RegZr::ZR, X16)
        }
        I::Cttz => {
            put(cx, mc::rbit(Size::X, tz, lo));
            put(cx, mc::clz(Size::X, tz, tz));
            put(cx, mc::rbit(Size::X, hi, hi));
            put(cx, mc::clz(Size::X, hi, hi));
            put(cx, mc::add_imm(Size::X, X17.sp(), X17.sp(), 64));
            put(cx, mc::cmp_imm(Size::X, X16.sp(), 0));
            put(cx, mc::csel(Size::X, tz, tz, hi, Cond::Ne));
            wide_result(cx, inst, tz, RegZr::ZR, X16)
        }
        I::Ctpop => {
            put(cx, mc::fmov_gpr_to_fp(Size::X, V31, lo));
            put(cx, mc::fmov_gpr_to_hi(V31, hi));
            put(cx, mc::cnt(VecBits::V128, V31, V31));
            put(cx, mc::uaddlv(Arr::B16, V31, V31));
            put(cx, mc::fmov_fp_to_gpr(Size::W, tz, V31));
            wide_result(cx, inst, tz, RegZr::ZR, X16)
        }
        I::Bswap | I::BitReverse => {
            let word = if which == I::Bswap { mc::rev } else { mc::rbit };
            put(cx, word(Size::X, lo, lo));
            put(cx, word(Size::X, hi, hi));
            wide_result(cx, inst, hi, lo, t)
        }
        I::Abs => {
            // (x ^ s) - s with s the sign: INT_MIN maps to itself.
            put(cx, mc::asr_imm(Size::X, tz, hi, 63));
            put(cx, mc::eor(Size::X, lo, lo, tz));
            put(cx, mc::eor(Size::X, hi, hi, tz));
            put(cx, mc::subs(Size::X, lo, lo, tz));
            put(cx, mc::sbc(Size::X, hi, hi, tz));
            wide_result(cx, inst, lo, hi, t)
        }
        _ => cx.bug("an i128 intrinsic without a lowering"),
    }
}

/// `smax`, `smin`, `umax`, `umin` and the saturating arithmetic of two
/// `i128`s: `a` in `x16`/`x17`, `b` in two temporaries.
fn wide_two<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    a: Value,
    b: Value,
) -> CgResult<()> {
    use Intrinsic as I;
    let (sa, sb) = (cx.use_mem(a)?, cx.use_mem(b)?);
    let (t1, t2) = (xr(cx.temp(Class::Gpr)?), xr(cx.temp(Class::Gpr)?));
    let (lo, hi, blo, bhi) = (X16.zr(), X17.zr(), t1.zr(), t2.zr());
    ld(cx, X16, sa, 0);
    ld(cx, X17, sa, 1);
    ld(cx, t1, sb, 0);
    ld(cx, t2, sb, 1);
    match which {
        I::Smax | I::Smin | I::Umax | I::Umin => {
            // The flags of a - b order the whole values (not `Z`).
            put(cx, mc::cmp(Size::X, lo, blo));
            put(cx, mc::sbcs(Size::X, RegZr::ZR, hi, bhi));
            let keep_a = match which {
                I::Smax => Cond::Ge,
                I::Smin => Cond::Lt,
                I::Umax => Cond::Hs,
                _ => Cond::Lo,
            };
            put(cx, mc::csel(Size::X, lo, lo, blo, keep_a));
            put(cx, mc::csel(Size::X, hi, hi, bhi, keep_a));
        }
        I::SaddSat | I::SsubSat => {
            if which == I::SaddSat {
                put(cx, mc::adds(Size::X, lo, lo, blo));
                put(cx, mc::adcs(Size::X, hi, hi, bhi));
            } else {
                put(cx, mc::subs(Size::X, lo, lo, blo));
                put(cx, mc::sbcs(Size::X, hi, hi, bhi));
            }
            // On overflow: the bound of the sign opposite to the wrapped
            // result's, low limb s, high limb s ^ (1 << 63).
            put(cx, mc::asr_imm(Size::X, blo, hi, 63));
            put(cx, mc::eor_imm(Size::X, t2.sp(), blo, 1 << 63));
            put(cx, mc::csel(Size::X, lo, blo, lo, Cond::Vs));
            put(cx, mc::csel(Size::X, hi, bhi, hi, Cond::Vs));
        }
        I::UaddSat => {
            put(cx, mc::adds(Size::X, lo, lo, blo));
            put(cx, mc::adcs(Size::X, hi, hi, bhi));
            put(cx, mc::csinv(Size::X, lo, lo, RegZr::ZR, Cond::Lo));
            put(cx, mc::csinv(Size::X, hi, hi, RegZr::ZR, Cond::Lo));
        }
        I::UsubSat => {
            put(cx, mc::subs(Size::X, lo, lo, blo));
            put(cx, mc::sbcs(Size::X, hi, hi, bhi));
            put(cx, mc::csel(Size::X, lo, lo, RegZr::ZR, Cond::Hs));
            put(cx, mc::csel(Size::X, hi, hi, RegZr::ZR, Cond::Hs));
        }
        _ => return cx.bug("an i128 intrinsic without a lowering"),
    }
    wide_result(cx, inst, lo, hi, t1)
}

/// `fshl`/`fshr` (`left`) of three `i128`s: with `k = c mod 128`,
/// `q = k >> 6` picks three consecutive limbs of `a:b` by `csel`
/// (`tst k, #64`), and each result limb is a 64-bit funnel of two of
/// them by `r = k mod 64` (the register shifts take `r` and `~r` modulo
/// 64; the other limb moves by one more, so `r = 0` needs no branch).
/// Four temporaries.
#[allow(clippy::many_single_char_names, reason = "the operands are LangRef's a, b, c")]
fn wide_funnel<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    left: bool,
    [a, b, c]: [Value; 3],
) -> CgResult<()> {
    let (sa, sb, sc) = (cx.use_mem(a)?, cx.use_mem(b)?, cx.use_mem(c)?);
    let k = xr(cx.temp(Class::Gpr)?);
    let p = xr(cx.temp(Class::Gpr)?);
    let q = xr(cx.temp(Class::Gpr)?);
    let r = xr(cx.temp(Class::Gpr)?);
    let (kz, pz, qz, rz) = (k.zr(), p.zr(), q.zr(), r.zr());
    let (x, y) = (X16.zr(), X17.zr());
    ld(cx, k, sc, 0);
    // `ldr` and a rebasing `add` leave the flags alone.
    put(cx, mc::tst_imm(Size::X, kz, 64));
    if left {
        // p, q, r = a.hi, a.lo, b.hi (q = 0) or a.lo, b.hi, b.lo.
        ld(cx, X16, sa, 0);
        ld(cx, X17, sa, 1);
        put(cx, mc::csel(Size::X, pz, x, y, Cond::Ne));
        ld(cx, X17, sb, 1);
        put(cx, mc::csel(Size::X, qz, y, x, Cond::Ne));
        ld(cx, X16, sb, 0);
        put(cx, mc::csel(Size::X, rz, x, y, Cond::Ne));
        put(cx, mc::mvn(Size::X, y, kz));
        // hi = (p << r) | (q >> 1 >> ~r); lo = (q << r) | (r_ >> 1 >> ~r).
        put(cx, mc::lslv(Size::X, pz, pz, kz));
        put(cx, mc::lsr_imm(Size::X, x, qz, 1));
        put(cx, mc::lsrv(Size::X, x, x, y));
        put(cx, mc::orr(Size::X, pz, pz, x));
        put(cx, mc::lslv(Size::X, qz, qz, kz));
        put(cx, mc::lsr_imm(Size::X, x, rz, 1));
        put(cx, mc::lsrv(Size::X, x, x, y));
        put(cx, mc::orr(Size::X, qz, qz, x));
        return wide_result(cx, inst, qz, pz, r);
    }
    // p, q, r = b.lo, b.hi, a.lo (q = 0) or b.hi, a.lo, a.hi.
    ld(cx, X16, sb, 0);
    ld(cx, X17, sb, 1);
    put(cx, mc::csel(Size::X, pz, y, x, Cond::Ne));
    ld(cx, X16, sa, 0);
    put(cx, mc::csel(Size::X, qz, x, y, Cond::Ne));
    ld(cx, X17, sa, 1);
    put(cx, mc::csel(Size::X, rz, y, x, Cond::Ne));
    put(cx, mc::mvn(Size::X, y, kz));
    // lo = (p >> r) | (q << 1 << ~r); hi = (q >> r) | (r_ << 1 << ~r).
    put(cx, mc::lsrv(Size::X, pz, pz, kz));
    put(cx, mc::lsl_imm(Size::X, x, qz, 1));
    put(cx, mc::lslv(Size::X, x, x, y));
    put(cx, mc::orr(Size::X, pz, pz, x));
    put(cx, mc::lsrv(Size::X, qz, qz, kz));
    put(cx, mc::lsl_imm(Size::X, x, rz, 1));
    put(cx, mc::lslv(Size::X, x, x, y));
    put(cx, mc::orr(Size::X, qz, qz, x));
    wide_result(cx, inst, pz, qz, r)
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
        Some(value) if value.trailing_zeros() >= 8 => RegZr::ZR,
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
