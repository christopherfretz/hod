//! Calls and intrinsics (`spec/codegen-x86.md` "Calls"). Owned by
//! WP-293; design 0008's I6 extends [`check`], I10 the unwinding rules,
//! I11 the variadic intrinsics.
//!
//! A call is `hod_cg`'s generic call sequence (`FnCx::call`) over
//! [`crate::abi::assign`]. An `invoke` is the same sequence: `hod_cg`
//! records its call site and pad, and writes the unwind tables (WP-314,
//! `crate::eh`).

use hod_cg::{CgResult, FnCx};
use hod_ir::{CallSite, Inst, Intrinsic, ValueKind};

use crate::common::unsupported;
use crate::target::X86;

/// `va_start`, `va_copy`, `va_end`, `va_arg`: design 0008's I11.
pub const VARARGS: &str = "x86:varargs";
/// An intrinsic this package does not lower.
pub const INTRINSIC: &str = "x86:intrinsic";
/// A call with operand bundles, a `callbr`.
pub const CALL: &str = "x86:call";

/// The checks of a call before the call sequence runs.
///
/// # Errors
/// [`CALL`], `x86:abi:musttail`.
pub fn check(cx: &FnCx<'_, X86>, inst: Inst<'_>, site: &CallSite<'_>) -> CgResult<()> {
    if inst.opcode() == hod_ir::Opcode::CallBr || site.bundle_count() != 0 {
        return unsupported(CALL);
    }
    crate::abi::check_call(cx, site)
}

/// The `call` arm of the dispatch.
///
/// # Errors
/// The refusals above, those of [`crate::asm::call`], [`VARARGS`], [`INTRINSIC`] and
/// those of the module owning the intrinsic; what the call sequence
/// reports.
pub fn lower<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, site: &CallSite<'a>) -> CgResult<()> {
    match site.intrinsic() {
        None if matches!(site.callee_value().kind(), ValueKind::Asm(_)) => {
            crate::asm::call(cx, inst)
        }
        None => {
            check(cx, inst, site)?;
            crate::x87::call_result(cx, inst, true)?;
            let done = cx.call(inst);
            crate::x87::call_result(cx, inst, false)?;
            done
        }
        Some(Intrinsic::EhTypeidFor) if site.args().len() == 1 => {
            crate::eh::typeid_for(cx, inst, site.args()[0])
        }
        Some(Intrinsic::Trap) => {
            cx.trap();
            Ok(())
        }
        Some(
            which @ (Intrinsic::Memcpy
            | Intrinsic::MemcpyInline
            | Intrinsic::Memmove
            | Intrinsic::Memset
            | Intrinsic::MemsetInline),
        ) => crate::bytes::intrinsic(cx, inst, which, site.args()),
        Some(Intrinsic::VaStart | Intrinsic::VaEnd | Intrinsic::VaCopy) => unsupported(VARARGS),
        Some(Intrinsic::StackSave | Intrinsic::StackRestore) => {
            unsupported(crate::frame::DYNAMIC_ALLOCA)
        }
        Some(which @ (Intrinsic::ThreadLocalAddress | Intrinsic::ThreadPointer)) => {
            crate::tls::intrinsic(cx, inst, which, site.args())
        }
        Some(which) if crate::int::is_intrinsic(cx, inst, which) => {
            crate::int::intrinsic(cx, inst, which, site.args())
        }
        Some(which) if crate::bits::is_intrinsic(cx, inst, which, site.args()) => {
            crate::bits::intrinsic(cx, inst, which, site.args())
        }
        Some(_) => unsupported(INTRINSIC),
    }
}
