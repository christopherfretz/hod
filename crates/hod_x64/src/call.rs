//! Calls, `invoke`, `musttail`, landing pads, `resume`, variadics:
//! WP-125 (X-call). Until then a plain direct or indirect call whose
//! signature the stub `abi` assigns goes through the core's call
//! sequence ([`FnCx::call`]), which is what the skeleton's tests need;
//! everything else refuses as [`CALL`].
//!
//! What WP-125 finds: `frame::emit_call` (`mov al, n` for a variadic
//! callee, `call rel32` with `R_X86_64_PLT32`, `call r11`), the tail
//! exits of `frame::end` (`jmp rel32` with `R_X86_64_PLT32`, `jmp r11`),
//! `common::extend` for `extend_arg`. The core keeps the no-deferral
//! rule of §5.6.1 by itself: a call emits every deferred argument before
//! it places any (`hod_cg/src/call.rs`, the loop before the signature),
//! so an instruction that writes `rcx`/`rdx` never runs inside the
//! argument window.

use hod_cg::abi::{AbiUnsupported, Assignment, Place, Sig};
use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, LandingPad, Opcode, TailKind, Value, ValueKind};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-125.
pub const CALL: &str = "x64:todo:call";

/// What of a right assignment (`abi::assign`) the lowerings that have
/// landed do not honour yet; `Target::assign` asks for every signature,
/// a definition's and a call's. WP-125 removes the first two conditions
/// with the code that honours them, WP-131 the third.
///
/// # Errors
/// [`crate::abi::ABI`] for a variadic signature (the register save area
/// and `va_start` are missing), for an `sret` parameter (a definition
/// has to hand the pointer back in `rax`, and nothing does), and for a
/// part in an x87 register (`Target::load` and `store` know the general
/// and the vector registers only).
pub fn guard(sig: &Sig<'_>, assignment: &Assignment) -> Result<(), AbiUnsupported> {
    let x87 = |place: Place| matches!(place, Place::Reg(reg) if crate::abi::is_x87(reg));
    let parts = assignment.args.iter().chain(&assignment.ret);
    if sig.variadic
        || assignment.sret_returned_in.is_some()
        || parts.into_iter().any(|p| x87(p.place))
    {
        return Err(AbiUnsupported(crate::abi::ABI));
    }
    Ok(())
}

/// A call or `invoke` of a function, not an intrinsic.
///
/// # Errors
/// [`CALL`] for an `invoke`, `callbr`, `musttail`, an operand bundle or an
/// inline-assembly callee; what `assign` and the call sequence report.
pub fn call<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> CgResult<()> {
    let Some(site) = inst.call() else {
        return cx.bug("a call lowering of something that is no call");
    };
    let plain = inst.opcode() == Opcode::Call
        && site.unwind_dest().is_none()
        && site.bundle_count() == 0
        && site.tail() != TailKind::MustTail
        && !matches!(site.callee_value().kind(), ValueKind::Asm(_));
    if !plain {
        return unsupported(CALL);
    }
    cx.call(inst)
}

/// `llvm.va_start`, `llvm.va_end`, `llvm.va_copy`.
///
/// # Errors
/// [`CALL`].
pub fn va_intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(CALL)
}

/// `va_arg`.
///
/// # Errors
/// [`CALL`].
pub fn va_arg<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, list: Value) -> CgResult<()> {
    let _ = (cx, inst, list);
    unsupported(CALL)
}

/// `landingpad`.
///
/// # Errors
/// [`CALL`].
pub fn landing_pad<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pad: &LandingPad<'a>,
) -> CgResult<()> {
    let _ = (cx, inst, pad);
    unsupported(CALL)
}

/// `resume`.
///
/// # Errors
/// [`CALL`].
pub fn resume<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, value: Value) -> CgResult<()> {
    let _ = (cx, inst, value);
    unsupported(CALL)
}
