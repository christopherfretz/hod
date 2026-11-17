//! Coroutine handle operations in any function (`spec/codegen.md` §7.6,
//! `spec/codegen-x86.md` §18): what LLVM's `coro-early` and
//! `coro-cleanup` make of them on i386, the ABI every object that holds
//! a `std::coroutine_handle` relies on. The same decisions as x86-64's
//! (`hod_x64/src/coro.rs`) with 4-byte pointers.
//!
//! **Owner: WP-327.** Tests: `crates/hod/tests/i386_coro.rs`. Coroutine
//! bodies are `coro_body` (WP-335); [`owns`] and [`intrinsic`] take its
//! intrinsics too, so that `call::lower` has one arm for the family.
//!
//! - **The frame header**: the resume function's address at offset 0,
//!   the destroy function's at offset 4. Each is a `fastcc void(ptr
//!   frame)` function: on i386 `fastcc` passes the frame in `ecx` and
//!   the caller pops nothing (`abi::assign`).
//! - `coro.resume(p)` / `coro.destroy(p)`: `mov r, [p + 0 | 4]` into a
//!   temporary, then the core's call sequence through it with `p` as the
//!   argument ([`hod_cg::FnCx::call_through`]); as an `invoke` too.
//! - `coro.done(p)`: the resume pointer is null: `cmp dword [p], 0` +
//!   `sete`.
//! - `coro.promise(p, align, from)`: `lea r, [p ± alignTo(8, align)]`
//!   (LLVM's `{ptr, ptr, i8}` field 2). `align` must be a constant power
//!   of two of at most 2^30 ([`PROMISE`] otherwise).
//! - `coro.noop()`: the address of the object's noop frame
//!   ([`hod_cg::FnCx::noop_coro_frame`]): `lea r, [frame]` with
//!   `R_386_32`, or from the GOT pointer with `R_386_GOTOFF` under PIC,
//!   as `llc` writes it.

use hod_cg::{CallArg, CgResult, Class, FnCx, SymRef};
use hod_ir::{CallConv, Inst, Intrinsic, TypeId, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Size};
use hod_obj::I386Reloc;

use crate::common::{ECX, const_int, gp, put, put_abs, unsupported};
use crate::target::X86;

/// `llvm.coro.promise` whose alignment is not a constant power of two of
/// at most 2^30, or whose direction is not a constant.
pub const PROMISE: &str = "x86:coro-promise";

/// The frame offset of the resume function's address.
pub const RESUME: u32 = 0;
/// The frame offset of the destroy function's address.
pub const DESTROY: u32 = 4;
/// The header's size: two pointers. The promise is at
/// `alignTo(HEADER, align)`.
pub const HEADER: u64 = 8;

/// The largest promise alignment lowered.
const MAX_PROMISE: u64 = 1 << 30;

/// The code of `__NoopCoro_ResumeDestroy`: `ret` (`fastcc`: the caller
/// pops nothing).
pub const NOOP_FN: &[u8] = &[0xc3];

/// Whether `which` is a coroutine intrinsic of this module or of
/// `crate::coro_body` (a body intrinsic, WP-335): `call::lower`'s one
/// arm for the family.
#[must_use]
pub fn owns(which: Intrinsic<'_>) -> bool {
    is_handle(which) || crate::coro_body::owns(which)
}

/// Whether `which` is a handle intrinsic.
fn is_handle(which: Intrinsic<'_>) -> bool {
    matches!(
        which,
        Intrinsic::CoroResume
            | Intrinsic::CoroDestroy
            | Intrinsic::CoroDone
            | Intrinsic::CoroPromise
            | Intrinsic::CoroNoop
    )
}

/// Whether `which` is a call that `llc` leaves no x87 value across
/// (`crate::x87`'s barrier rule): `resume` and `destroy` call through
/// the frame (measured: llc 19 and 22 spill a `double` live across
/// either to the stack); the body's suspend points
/// (`crate::coro_body::is_barrier`, WP-335).
#[must_use]
pub fn is_barrier(which: Intrinsic<'_>) -> bool {
    matches!(which, Intrinsic::CoroResume | Intrinsic::CoroDestroy)
        || crate::coro_body::is_barrier(which)
}

/// A call (or, for `resume` and `destroy`, an `invoke`) of a handle
/// intrinsic.
///
/// # Errors
/// [`PROMISE`]; what the call sequence and the GOT pointer report;
/// `Internal` for another intrinsic or an argument list the verifier
/// rejects.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    match (which, args) {
        (Intrinsic::CoroResume, &[frame]) => call_slot(cx, inst, frame, RESUME),
        (Intrinsic::CoroDestroy, &[frame]) => call_slot(cx, inst, frame, DESTROY),
        (Intrinsic::CoroDone, &[frame]) => done(cx, inst, frame),
        (Intrinsic::CoroPromise, &[frame, align, from]) => promise(cx, inst, frame, align, from),
        (Intrinsic::CoroNoop, []) => noop(cx, inst),
        _ if crate::coro_body::owns(which) => crate::coro_body::intrinsic(cx, inst, which, args),
        _ => cx.bug("a coroutine handle intrinsic with unexpected arguments"),
    }
}

/// `resume` or `destroy`: calls the function whose address is at
/// `frame + offset`, with `frame` (`fastcc`: in `ecx`).
fn call_slot<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    frame: Value,
    offset: u32,
) -> CgResult<()> {
    let p = cx.use_reg(frame)?;
    let callee = cx.temp(Class::Gpr)?;
    put(cx, mc::mov_rm(Size::D, gp(callee), Mem::base(gp(p), offset as i32)));
    cx.call_through(inst, callee, &[CallArg::Value(frame)], &[TypeId::PTR], CallConv::FAST)
}

/// `done`: whether the resume pointer is null (`setcc` into `cl` when
/// the result's register has no low byte, as `alu::icmp`).
fn done<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, frame: Value) -> CgResult<()> {
    let p = cx.use_reg(frame)?;
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    put(cx, mc::cmp_mi(Size::D, Mem::base(gp(p), RESUME as i32), 0));
    let byte = if crate::reg::has_low_byte(r) { r } else { ECX };
    put(cx, mc::setcc_r(Cond::E, byte));
    put(cx, mc::movzx_rr(Size::D, r, Size::B, byte));
    cx.finish(inst, rd)
}

/// The distance between a frame and its promise of alignment `align`.
#[must_use]
pub fn promise_offset(align: u64) -> u64 {
    HEADER.next_multiple_of(align.max(1))
}

/// `promise`: the frame and the promise are `alignTo(8, align)` apart.
fn promise<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    frame: Value,
    align: Value,
    from: Value,
) -> CgResult<()> {
    let Some(align) =
        const_int(cx, align).map(u64::from).filter(|&a| a.is_power_of_two() && a <= MAX_PROMISE)
    else {
        return unsupported(PROMISE);
    };
    let Some(from) = const_int(cx, from) else { return unsupported(PROMISE) };
    let offset = promise_offset(align) as i32;
    let p = cx.use_reg(frame)?;
    let rd = cx.def_reg(inst)?;
    let disp = if from & 1 == 1 { -offset } else { offset };
    put(cx, mc::lea(Size::D, gp(rd), Mem::base(gp(p), disp)));
    cx.finish(inst, rd)
}

/// `noop`: the address of the object's noop frame, a private constant
/// (`GOTOFF` under PIC, as any local symbol).
fn noop<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
    let frame = cx.noop_coro_frame()?;
    debug_assert_eq!(frame, SymRef::NoopCoroFrame, "the noop frame's symbol");
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    if cx.options.reloc_model.is_pic() {
        crate::pic::got_pointer(cx, r)?;
        let m = Mem::base(r, 0);
        let field = m.with_abs32().unwrap_or(m);
        crate::pic::put_reloc(cx, mc::lea(Size::D, r, field), I386Reloc::GotOff, frame, 0)?;
    } else {
        let m = Mem::abs(0);
        put_abs(cx, mc::lea(Size::D, r, m.with_abs32().unwrap_or(m)), frame, 0)?;
    }
    cx.finish(inst, rd)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LLVM's `{ptr, ptr, i8}` field 2 aligned up: 8 for a promise of
    /// alignment 1 to 8, the alignment above.
    #[test]
    fn the_promise_follows_two_pointers() {
        for (align, offset) in [(1, 8), (4, 8), (8, 8), (16, 16), (32, 32), (1 << 30, 1 << 30)] {
            assert_eq!(promise_offset(align), offset, "{align}");
        }
    }

    #[test]
    fn the_noop_function_only_returns() {
        assert_eq!(NOOP_FN, mc::ret().bytes());
    }
}
