//! Thread-local storage (`%gs`, `R_386_TLS_*`): design 0008's I8 owns
//! this file. WP-293 refuses every thread-local access under [`TLS`].

use hod_cg::{CgResult, FnCx};

use crate::common::unsupported;
use crate::target::X86;

/// A thread-local symbol's address, `llvm.threadlocal.address`,
/// `llvm.thread.pointer`: removed by I8.
pub const TLS: &str = "x86:tls";

/// The address of a thread-local symbol.
///
/// # Errors
/// [`TLS`].
pub fn address(_cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    unsupported(TLS)
}
