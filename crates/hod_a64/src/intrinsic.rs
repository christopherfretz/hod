//! Intrinsic calls (`spec/ir.md` §5.5) other than those the dispatch
//! routes to `call` (`llvm.va_*`), `mem` (`llvm.stacksave`,
//! `llvm.stackrestore`) and `aggr` (`*.with.overflow`): memory
//! intrinsics, bit manipulation, saturating and min/max arithmetic,
//! floating-point math, traps, and the intrinsics that emit nothing.
//!
//! **Owner: WP-041.** Tests: `tests/intrinsic.rs`. Until the package lands
//! the entry point refuses with the stable name below; the dispatch in
//! `target.rs` is final. Thread-local addresses are refused by the
//! dispatch (`a64:thread-local`, milestone M2).

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, Value};

use crate::common::unsupported;
use crate::target::A64;

/// An intrinsic call this module does not lower (yet).
pub const INTRINSIC: &str = "a64:intrinsic";

/// A call of intrinsic `which` with arguments `args`.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(INTRINSIC)
}
