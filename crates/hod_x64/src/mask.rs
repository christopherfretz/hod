//! Vectors of `i1` (masks, class `Gpr`, bit-packed as on AArch64):
//! compares that produce them, vector-condition `select`, mask casts,
//! their element access and shuffles, `llvm.vector.reduce.*`: WP-132
//! (X-vector). The dispatch sends here every instruction whose result or
//! first operand is a mask; refuses as [`MASK`] until then.

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, TypeId, TypeKind, TypeTable};

use crate::common::{unsupported, value_ty};
use crate::target::X64;

/// The group's refusal until WP-132.
pub const MASK: &str = "x64:todo:mask";

/// The longest mask of class `Gpr`: one bit per element in 64 bits.
pub const MAX_MASK: u32 = 64;

/// Whether `ty` is a mask of class `Gpr`.
pub fn is_mask(types: &TypeTable, ty: TypeId) -> bool {
    matches!(types.kind(ty), TypeKind::Vector { elem, len }
        if len <= MAX_MASK && matches!(types.kind(elem), TypeKind::Int(1)))
}

/// The dispatch's first step: `Some` for an instruction whose result or
/// any operand is a mask, `None` for the others.
///
/// # Errors
/// [`MASK`], inside the `Some`.
pub fn lower<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> Option<CgResult<()>> {
    let types = cx.types();
    let involved = is_mask(types, inst.ty())
        || inst.operands().iter().any(|&value| is_mask(types, value_ty(cx, value)));
    involved.then(|| unsupported(MASK))
}
