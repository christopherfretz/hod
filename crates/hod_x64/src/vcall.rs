//! Vectors across calls: the parts of kind `PartKind::Lanes`
//! (`Target::lanes_out`, `Target::lanes_in`): WP-125 with WP-132. Both
//! refuse as [`VCALL`] until then; the stub `abi` makes no such part.

use hod_cg::abi::{LaneLoc, Part};
use hod_cg::{CgResult, FnCx};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-125/WP-132.
pub const VCALL: &str = "x64:todo:vcall";

/// The producer's side of a `Lanes` part.
///
/// # Errors
/// [`VCALL`].
pub fn lanes_out(cx: &mut FnCx<'_, X64>, part: &Part, from: LaneLoc, to: LaneLoc) -> CgResult<()> {
    let _ = (cx, part, from, to);
    unsupported(VCALL)
}

/// The consumer's side of a `Lanes` part.
///
/// # Errors
/// [`VCALL`].
pub fn lanes_in(cx: &mut FnCx<'_, X64>, part: &Part, from: LaneLoc, to: LaneLoc) -> CgResult<()> {
    let _ = (cx, part, from, to);
    unsupported(VCALL)
}
