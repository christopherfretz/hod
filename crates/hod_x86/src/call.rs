//! Calls and intrinsics (`spec/codegen-x86.md` "Calls"). Owned by
//! WP-293; design 0008's I6 extends [`check`], I10 the unwinding rules,
//! I11 the variadic intrinsics.
//!
//! A call is `hod_cg`'s generic call sequence (`FnCx::call`) over
//! [`crate::abi::assign`]. Without unwind tables (I10), a frame the
//! unwinder may have to pass through is refused: an `invoke`, and a call
//! that may unwind made by a function that may unwind.

use hod_cg::{CgResult, FnCx};
use hod_ir::{CallSite, FnAttrs, Inst, Intrinsic, Module, SymId, SymbolKind, TailKind, ValueKind};

use crate::common::unsupported;
use crate::target::X86;

/// An `invoke`, a `landingpad`, a `resume`, and a call that may unwind
/// in a function that may unwind: design 0008's I10.
pub const EH: &str = "x86:eh";
/// `va_start`, `va_copy`, `va_end`, `va_arg`: design 0008's I11.
pub const VARARGS: &str = "x86:varargs";
/// An intrinsic this package does not lower.
pub const INTRINSIC: &str = "x86:intrinsic";
/// A call with operand bundles, a `callbr`.
pub const CALL: &str = "x86:call";

/// Whether function `sym` is `nounwind`.
fn nounwind(module: &Module, sym: SymId) -> bool {
    match module.symbol(sym).kind() {
        SymbolKind::Function { attrs, .. } => module.fn_attrs(attrs).has(FnAttrs::NOUNWIND),
        _ => false,
    }
}

/// Whether the call `site` may unwind into a function that may unwind.
fn may_unwind(cx: &FnCx<'_, X86>, site: &CallSite<'_>) -> bool {
    if site.is_nounwind() || nounwind(cx.module, cx.body.symbol()) {
        return false;
    }
    match site.callee_value().kind() {
        ValueKind::Sym(sym) => !nounwind(cx.module, sym),
        _ => true,
    }
}

/// The checks of a call before the call sequence runs.
///
/// # Errors
/// [`EH`], [`CALL`], `x86:abi:musttail`.
pub fn check(cx: &FnCx<'_, X86>, inst: Inst<'_>, site: &CallSite<'_>) -> CgResult<()> {
    if site.unwind_dest().is_some() || may_unwind(cx, site) {
        return unsupported(EH);
    }
    if inst.opcode() == hod_ir::Opcode::CallBr || site.bundle_count() != 0 {
        return unsupported(CALL);
    }
    if site.tail() == TailKind::MustTail {
        return unsupported(crate::frame::MUSTTAIL);
    }
    Ok(())
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
            cx.call(inst)
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
        Some(Intrinsic::ThreadLocalAddress | Intrinsic::ThreadPointer) => {
            unsupported(crate::tls::TLS)
        }
        Some(_) => unsupported(INTRINSIC),
    }
}
