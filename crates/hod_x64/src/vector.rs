//! `extractelement`, `insertelement`, `shufflevector`: WP-132
//! (X-vector). Refuses as [`VECTOR`] until then.

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-132.
pub const VECTOR: &str = "x64:todo:vector";

/// `extractelement`.
///
/// # Errors
/// [`VECTOR`].
pub fn extract_element<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    vector: Value,
    index: Value,
) -> CgResult<()> {
    let _ = (cx, inst, vector, index);
    unsupported(VECTOR)
}

/// `insertelement`.
///
/// # Errors
/// [`VECTOR`].
pub fn insert_element<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    vector: Value,
    value: Value,
    index: Value,
) -> CgResult<()> {
    let _ = (cx, inst, vector, value, index);
    unsupported(VECTOR)
}

/// `shufflevector`.
///
/// # Errors
/// [`VECTOR`].
pub fn shuffle_vector<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    first: Value,
    second: Value,
    mask: &[u32],
) -> CgResult<()> {
    let _ = (cx, inst, first, second, mask);
    unsupported(VECTOR)
}
