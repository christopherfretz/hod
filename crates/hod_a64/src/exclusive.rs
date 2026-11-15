//! The exclusive-access intrinsics (WP-245, `spec/codegen.md` §5.6.39):
//! `llvm.aarch64.ldxr`, `ldaxr`, `stxr`, `stlxr` (the access size read
//! from the pointer argument's `elementtype`), the pair forms `ldxp`,
//! `ldaxp`, `stxp`, `stlxp`, and `clrex`.
//!
//! Each is the one instruction `llc -O0 -global-isel=0 -fast-isel=0` of
//! LLVM 19 and 22 selects for it (`tests/exclusive.rs` compares them):
//!
//! - `ldxr`/`ldaxr`: `ldxr{b,h,}`/`ldaxr{b,h,}` into the result's
//!   register; the `w` forms zero the upper bits, so the `i64` result is
//!   the loaded value zero-extended.
//! - `stxr`/`stlxr(i64 v, ptr p)`: `st{l,}xr{b,h,} ws, {w,x}v, [p]`, the
//!   low bits of `v` stored; the `i32` result is the status `ws`, 0 when
//!   the store happened.
//! - `ldxp`/`ldaxp(ptr p)`: `ld{a,}xp xa, xb, [p]`; `{a, b}` is the
//!   `{i64, i64}` result (`a` from the lower address), stored to its
//!   slot after the load.
//! - `stxp`/`stlxp(i64 lo, i64 hi, ptr p)`: `st{l,}xp ws, xlo, xhi, [p]`.
//! - `clrex`: `clrex`.
//!
//! The operands are placed before the instruction and the result after
//! it, by the policy of `hod_cg` (§4): like every value, a result that
//! lives in the frame is stored there after the instruction, and an
//! operand that lives there is loaded before it. The lowering adds no
//! memory access of its own; what lies between a program's
//! load-exclusive and its store-exclusive is the program's (§5.6.39
//! says what `-O0` code does there).
//!
//! Refused by name: [`ELEMENT`] for a missing `elementtype` or one other
//! than `i8`, `i16`, `i32`, `i64` (LLVM's verifier rejects the first,
//! `llc` aborts on the others); [`TYPE`] for a call whose types are not
//! the intrinsic's (`i64`, `i32`, `{i64, i64}`).

use hod_cg::{CgResult, Class, FnCx, PReg};
use hod_ir::{Inst, Intrinsic, TypeId, TypeKind, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{MemSize, Size};

use crate::common::{Cursor, X16, deferred_first, put, store_gpr_at, unsupported, value_ty, xr};
use crate::target::A64;

/// A load or store exclusive whose pointer has no `elementtype`, or one
/// that is not `i8`, `i16`, `i32` or `i64`.
pub const ELEMENT: &str = "a64:exclusive-element-type";
/// An exclusive-access call whose result or operand types are not the
/// intrinsic's.
pub const TYPE: &str = "a64:exclusive-type";

/// Whether `which` is one of this module's intrinsics.
#[must_use]
pub fn handles(which: Intrinsic<'_>) -> bool {
    use Intrinsic as I;
    matches!(
        which,
        I::Aarch64Ldxr
            | I::Aarch64Ldaxr
            | I::Aarch64Stxr
            | I::Aarch64Stlxr
            | I::Aarch64Ldxp
            | I::Aarch64Ldaxp
            | I::Aarch64Stxp
            | I::Aarch64Stlxp
            | I::Aarch64Clrex
    )
}

/// A call of exclusive-access intrinsic `which` ([`handles`]).
///
/// # Errors
/// [`ELEMENT`], [`TYPE`]; what placing the operands reports.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    match (which, args) {
        (I::Aarch64Ldxr | I::Aarch64Ldaxr, &[p]) => {
            let ms = element(cx, inst, 0)?;
            want(inst.ty(), TypeId::I64)?;
            deferred_first(cx, inst, None)?;
            let pp = cx.use_reg(p)?;
            let rp = xr(pp);
            // Into the pointer's register, as `llc` does for `x`.
            let word = if which == I::Aarch64Ldaxr {
                mc::ldaxr(ms, rp.zr(), rp.sp())
            } else {
                mc::ldxr(ms, rp.zr(), rp.sp())
            };
            put(cx, word);
            cx.finish(inst, pp)
        }
        (I::Aarch64Stxr | I::Aarch64Stlxr, &[v, p]) => {
            let ms = element(cx, inst, 1)?;
            want(inst.ty(), TypeId::I32)?;
            want(value_ty(cx, v), TypeId::I64)?;
            deferred_first(cx, inst, None)?;
            let pv = cx.use_reg(v)?;
            let pp = operand(cx, p, &[(v, pv)])?;
            let rs = cx.def_reg(inst)?;
            let (status, data, base) = (xr(rs).zr(), xr(pv).zr(), xr(pp).sp());
            let word = if which == I::Aarch64Stlxr {
                mc::try_stlxr(ms, status, data, base)
            } else {
                mc::try_stxr(ms, status, data, base)
            };
            let Some(word) = word else {
                return cx.bug("the status register of a store exclusive is an operand's");
            };
            put(cx, word);
            cx.finish(inst, rs)
        }
        (I::Aarch64Ldxp | I::Aarch64Ldaxp, &[p]) => {
            pair_type(cx, inst.ty())?;
            deferred_first(cx, inst, None)?;
            let rp = xr(cx.use_reg(p)?);
            let a = cx.temp(Class::Gpr)?;
            let b = cx.temp(Class::Gpr)?;
            let (ra, rb) = (xr(a), xr(b));
            let word = if which == I::Aarch64Ldaxp {
                mc::try_ldaxp(Size::X, ra.zr(), rb.zr(), rp.sp())
            } else {
                mc::try_ldxp(Size::X, ra.zr(), rb.zr(), rp.sp())
            };
            let Some(word) = word else {
                return cx.bug("two temporaries in one register");
            };
            put(cx, word);
            let slot = cx.def_mem(inst)?;
            let mut at = Cursor::frame(cx, slot, X16);
            store_gpr_at(cx, MemSize::X, ra.zr(), &mut at);
            let mut at = Cursor::frame(cx, slot.offset_by(8), X16);
            store_gpr_at(cx, MemSize::X, rb.zr(), &mut at);
            cx.free_temp(a);
            cx.free_temp(b);
            Ok(())
        }
        (I::Aarch64Stxp | I::Aarch64Stlxp, &[lo, hi, p]) => {
            want(inst.ty(), TypeId::I32)?;
            want(value_ty(cx, lo), TypeId::I64)?;
            want(value_ty(cx, hi), TypeId::I64)?;
            deferred_first(cx, inst, None)?;
            let pl = cx.use_reg(lo)?;
            let ph = operand(cx, hi, &[(lo, pl)])?;
            let pp = operand(cx, p, &[(lo, pl), (hi, ph)])?;
            let rs = cx.def_reg(inst)?;
            let status = xr(rs).zr();
            let (low, high, base) = (xr(pl).zr(), xr(ph).zr(), xr(pp).sp());
            let word = if which == I::Aarch64Stlxp {
                mc::try_stlxp(Size::X, status, low, high, base)
            } else {
                mc::try_stxp(Size::X, status, low, high, base)
            };
            let Some(word) = word else {
                return cx.bug("the status register of a store exclusive is an operand's");
            };
            put(cx, word);
            cx.finish(inst, rs)
        }
        (I::Aarch64Clrex, []) => {
            put(cx, mc::clrex());
            Ok(())
        }
        _ => unsupported(TYPE),
    }
}

/// The access size of argument `index`'s `elementtype`.
fn element(cx: &FnCx<'_, A64>, inst: Inst<'_>, index: usize) -> CgResult<MemSize> {
    let Some(site) = inst.call() else {
        return cx.bug("an intrinsic that is not a call");
    };
    match site.arg_attrs(index).mem_type() {
        Some(TypeId::I8) => Ok(MemSize::B),
        Some(TypeId::I16) => Ok(MemSize::H),
        Some(TypeId::I32) => Ok(MemSize::W),
        Some(TypeId::I64) => Ok(MemSize::X),
        _ => unsupported(ELEMENT),
    }
}

/// [`TYPE`] unless `ty` is `expected`.
fn want(ty: TypeId, expected: TypeId) -> CgResult<()> {
    if ty == expected { Ok(()) } else { unsupported(TYPE) }
}

/// [`TYPE`] unless `ty` is `{i64, i64}`.
fn pair_type(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<()> {
    match cx.types().kind(ty) {
        TypeKind::Struct { fields: &[TypeId::I64, TypeId::I64], packed: false } => Ok(()),
        _ => unsupported(TYPE),
    }
}

/// `value` in a register, or the register of an earlier operand that is
/// the same value (a register value may be asked for once).
fn operand(cx: &mut FnCx<'_, A64>, value: Value, earlier: &[(Value, PReg)]) -> CgResult<PReg> {
    match earlier.iter().find(|(v, _)| *v == value) {
        Some(&(_, reg)) => Ok(reg),
        None => cx.use_reg(value),
    }
}
