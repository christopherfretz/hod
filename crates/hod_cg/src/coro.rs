//! Coroutine handles, the target-independent part (`spec/codegen.md`
//! "Coroutines", WP-105): the noop coroutine frame a function refers to.
//! Module emission places the frame and its function
//! (`emit/noop.rs`); the lowering of the handle intrinsics is the
//! target's (`hod_a64::coro`), and the call through a frame's resume or
//! destroy pointer is [`FnCx::call_through`].

use crate::cx::FnCx;
use crate::error::{CgError, CgResult};
use crate::sym::SymRef;
use crate::target::Target;

/// `llvm.coro.noop` on a target whose `Target::NOOP_CORO_FN` is empty.
pub const NOOP_CORO_TARGET: &str = "coro:noop-target";

impl<T: Target> FnCx<'_, T> {
    /// The symbol of the object's noop coroutine frame
    /// ([`SymRef::NoopCoroFrame`]): what `llvm.coro.noop` returns the
    /// address of. Asking for it is what makes module emission write the
    /// frame and its function, once per object.
    ///
    /// # Errors
    /// [`NOOP_CORO_TARGET`] when the target has no such function.
    pub fn noop_coro_frame(&mut self) -> CgResult<SymRef> {
        if T::NOOP_CORO_FN.is_empty() {
            return Err(CgError::unsupported(NOOP_CORO_TARGET));
        }
        self.out.noop_coro_fn = T::NOOP_CORO_FN;
        Ok(SymRef::NoopCoroFrame)
    }
}
