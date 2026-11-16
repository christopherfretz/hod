//! Position-independent code (the GOT, `R_386_GOTPC`, `R_386_GOTOFF`):
//! design 0008's I8 owns this file. WP-293 compiles the static
//! relocation model only and refuses the others under [`PIC`].

use hod_cg::{CgResult, FnCx, RelocModel};

use crate::common::unsupported;
use crate::target::X86;

/// A relocation model other than static: removed by I8.
pub const PIC: &str = "x86:pic";

/// Refuses every relocation model but the static one.
///
/// # Errors
/// [`PIC`].
pub fn check_static(cx: &FnCx<'_, X86>) -> CgResult<()> {
    if cx.options.reloc_model == RelocModel::Static { Ok(()) } else { unsupported(PIC) }
}
