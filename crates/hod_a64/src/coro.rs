//! Coroutine handle operations in any function (`spec/codegen.md`
//! "Coroutines", design 0005 §1.1 and §2): what LLVM's `coro-early` and
//! `coro-cleanup` make of them, which is the ABI every object that holds
//! a `std::coroutine_handle` relies on.
//!
//! **Owner: WP-105.** Tests: `tests/coro.rs`. Hooked from
//! `intrinsic::intrinsic` (one arm). Coroutine bodies (`coro.id`,
//! `begin`, `suspend`, `end`, ..., `presplitcoroutine`) are not here:
//! `classify` refuses them until WP-106.
//!
//! - **The frame header**: the resume function's address at offset 0,
//!   the destroy function's at offset 8. Each is a `void(ptr frame)`
//!   function, called `fastcc`, which is the C convention for one
//!   pointer argument on AArch64.
//! - `coro.resume(p)` / `coro.destroy(p)`: `ldr x, [p, #0 | #8]` into a
//!   temporary, then the core's call sequence through it with `p` as the
//!   argument ([`hod_cg::FnCx::call_through`]); as an `invoke` too.
//! - `coro.done(p)`: the resume pointer is null: `ldr` + `cmp #0` +
//!   `cset eq`.
//! - `coro.promise(p, align, from)`: `p ± alignTo(16, align)`, minus when
//!   `from` is true. `align` must be a constant power of two of at most
//!   2^30 ([`PROMISE`] otherwise: LLVM computes the offset in `i32`
//!   from the floor of its logarithm, so another value has no meaning a
//!   front end relies on).
//! - `coro.noop()`: the address of the object's noop frame
//!   ([`hod_cg::FnCx::noop_coro_frame`]), `adrp` + `add :lo12:` to a
//!   private symbol, as `llc` reaches it.

use hod_cg::{CallArg, CgResult, Class, FnCx, SymRef};
use hod_ir::{CallConv, CodeModel, Inst, Intrinsic, TypeId, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{Cond, Load, Size};
use hod_obj::{A64Reloc, RelocKind};

use crate::common::{CODE_MODEL, add_const, deferred_first, put, unsupported, xr};
use crate::intrinsic::const_int;
use crate::target::A64;

/// `llvm.coro.promise` whose alignment is not a constant power of two of
/// at most 2^30, or whose direction is not a constant.
pub const PROMISE: &str = "a64:coro-promise";

/// The frame offset of the resume function's address.
pub const RESUME: u32 = 0;
/// The frame offset of the destroy function's address.
pub const DESTROY: u32 = 8;

/// The largest promise alignment lowered.
const MAX_PROMISE: u64 = 1 << 30;

/// A call (or, for `resume` and `destroy`, an `invoke`) of a handle
/// intrinsic.
///
/// # Errors
/// [`PROMISE`]; `a64:code-model` for `coro.noop` outside the small
/// code model; what the call sequence reports; `Internal` for another
/// intrinsic or an argument list the verifier rejects.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    match (which, args) {
        (Intrinsic::CoroResume, &[frame]) => call_slot(cx, inst, frame, RESUME),
        (Intrinsic::CoroDestroy, &[frame]) => call_slot(cx, inst, frame, DESTROY),
        (Intrinsic::CoroDone, &[frame]) => done(cx, inst, frame),
        (Intrinsic::CoroPromise, &[frame, align, from]) => promise(cx, inst, frame, align, from),
        (Intrinsic::CoroNoop, []) => noop(cx, inst),
        _ => cx.bug("a coroutine handle intrinsic with unexpected arguments"),
    }
}

/// `resume` or `destroy`: calls the function whose address is at
/// `frame + offset`, with `frame`.
fn call_slot<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    frame: Value,
    offset: u32,
) -> CgResult<()> {
    let p = cx.use_reg(frame)?;
    let callee = cx.temp(Class::Gpr)?;
    put(cx, mc::ldr_off(Load::X, xr(callee).zr(), xr(p).sp(), i64::from(offset)));
    cx.call_through(inst, callee, &[CallArg::Value(frame)], &[TypeId::PTR], CallConv::FAST)
}

/// `done`: whether the resume pointer is null.
fn done<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, frame: Value) -> CgResult<()> {
    let p = cx.use_reg(frame)?;
    let rd = cx.def_reg(inst)?;
    put(cx, mc::ldr_off(Load::X, xr(rd).zr(), xr(p).sp(), i64::from(RESUME)));
    put(cx, mc::cmp_imm(Size::X, xr(rd).sp(), 0));
    put(cx, mc::cset(Size::W, xr(rd).zr(), Cond::Eq));
    cx.finish(inst, rd)
}

/// `promise`: the frame and the promise are `alignTo(16, align)` apart.
fn promise<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    frame: Value,
    align: Value,
    from: Value,
) -> CgResult<()> {
    let Some(align) = const_int(cx, align).filter(|&a| a.is_power_of_two() && a <= MAX_PROMISE)
    else {
        return unsupported(PROMISE);
    };
    let Some(from) = const_int(cx, from) else { return unsupported(PROMISE) };
    let offset = align.max(16) as i64;
    let p = cx.use_reg(frame)?;
    let rd = cx.def_reg(inst)?;
    add_const(cx, xr(rd).sp(), xr(p).sp(), if from & 1 == 1 { -offset } else { offset });
    cx.finish(inst, rd)
}

/// `noop`: the address of the object's noop frame.
fn noop<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> CgResult<()> {
    if cx.options.code_model(cx.module) != CodeModel::Small {
        return unsupported(CODE_MODEL);
    }
    let frame = cx.noop_coro_frame()?;
    debug_assert_eq!(frame, SymRef::NoopCoroFrame, "the noop frame's symbol");
    let rd = cx.def_reg(inst)?;
    let at = cx.offset();
    put(cx, mc::adrp(xr(rd).zr(), 0));
    cx.reloc(at, RelocKind::A64(A64Reloc::AdrPrelPgHi21), frame, 0);
    put(cx, mc::add_imm(Size::X, xr(rd).sp(), xr(rd).sp(), 0));
    cx.reloc(at + 4, RelocKind::A64(A64Reloc::AddAbsLo12Nc), frame, 0);
    cx.finish(inst, rd)
}
