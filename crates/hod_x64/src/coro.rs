//! Coroutine handle operations in any function (`spec/codegen.md` §7.6,
//! design 0005 §1.1 and §2): what LLVM's `coro-early` and `coro-cleanup`
//! make of them, which is the ABI every object that holds a
//! `std::coroutine_handle` relies on. The same decisions as AArch64's
//! (WP-105, `hod_a64/src/coro.rs`).
//!
//! **Owner: WP-137.** Tests: `tests/coro.rs`. Coroutine bodies are
//! `coro_body`.
//!
//! - **The frame header**: the resume function's address at offset 0,
//!   the destroy function's at offset 8. Each is a `void(ptr frame)`
//!   function, called `fastcc`, which is the C convention for one
//!   pointer argument on x86-64.
//! - `coro.resume(p)` / `coro.destroy(p)`: `mov r, [p + 0 | 8]` into a
//!   temporary, then the core's call sequence through it with `p` as the
//!   argument ([`hod_cg::FnCx::call_through`]); as an `invoke` too.
//! - `coro.done(p)`: the resume pointer is null: `cmp qword [p], 0` +
//!   `sete`.
//! - `coro.promise(p, align, from)`: `lea r, [p ± alignTo(16, align)]`.
//!   `align` must be a constant power of two of at most 2^30 ([`PROMISE`]
//!   otherwise).
//! - `coro.noop()`: the address of the object's noop frame
//!   ([`hod_cg::FnCx::noop_coro_frame`]), `lea r, [rip + frame]`
//!   (`R_X86_64_PC32` to its private section), as `llc` reaches it.

use hod_cg::{CallArg, CgResult, Class, FnCx, SymRef};
use hod_ir::{CallConv, CodeModel, Inst, Intrinsic, TypeId, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Size};
use hod_obj::X64Reloc;

use crate::common::{CODE_MODEL, deferred_first, gp, put, put_reloc, unsupported};
use crate::intrinsic::const_int;
use crate::target::X64;

/// `llvm.coro.promise` whose alignment is not a constant power of two of
/// at most 2^30, or whose direction is not a constant.
pub const PROMISE: &str = "x64:coro-promise";

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
/// [`PROMISE`]; [`CODE_MODEL`] for `coro.noop` outside the small code
/// model; what the call sequence reports; `Internal` for another
/// intrinsic or an argument list the verifier rejects.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
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
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    frame: Value,
    offset: u32,
) -> CgResult<()> {
    let p = cx.use_reg(frame)?;
    let callee = cx.temp(Class::Gpr)?;
    put(cx, mc::mov_rm(Size::Q, gp(callee), Mem::base(gp(p), offset as i32)));
    cx.call_through(inst, callee, &[CallArg::Value(frame)], &[TypeId::PTR], CallConv::FAST)
}

/// `done`: whether the resume pointer is null.
fn done<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, frame: Value) -> CgResult<()> {
    let p = cx.use_reg(frame)?;
    let rd = cx.def_reg(inst)?;
    put(cx, mc::cmp_mi(Size::Q, Mem::base(gp(p), RESUME as i32), 0));
    put(cx, mc::setcc_r(Cond::E, gp(rd)));
    cx.finish(inst, rd)
}

/// `promise`: the frame and the promise are `alignTo(16, align)` apart.
fn promise<'a>(
    cx: &mut FnCx<'a, X64>,
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
    let offset = align.max(16) as i32;
    let p = cx.use_reg(frame)?;
    let rd = cx.def_reg(inst)?;
    let disp = if from & 1 == 1 { -offset } else { offset };
    put(cx, mc::lea(Size::Q, gp(rd), Mem::base(gp(p), disp)));
    cx.finish(inst, rd)
}

/// `noop`: the address of the object's noop frame.
fn noop<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> CgResult<()> {
    if cx.options.code_model(cx.module) != CodeModel::Small {
        return unsupported(CODE_MODEL);
    }
    let frame = cx.noop_coro_frame()?;
    debug_assert_eq!(frame, SymRef::NoopCoroFrame, "the noop frame's symbol");
    let rd = cx.def_reg(inst)?;
    put_reloc(cx, mc::lea(Size::Q, gp(rd), Mem::rip(0)), X64Reloc::Pc32, frame, 0)?;
    cx.finish(inst, rd)
}
