//! Thread-local addresses (`common::sym_address` of a thread-local
//! symbol), `llvm.threadlocal.address`, `llvm.thread.pointer`: WP-134
//! (X-tls; design 0006 §2.8). Refuses as [`TLS`] until then.

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-134.
pub const TLS: &str = "x64:todo:tls";

/// `llvm.threadlocal.address`, `llvm.thread.pointer`.
///
/// # Errors
/// [`TLS`].
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(TLS)
}
