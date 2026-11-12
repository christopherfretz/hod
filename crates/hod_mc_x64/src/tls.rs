//! The fixed-form instructions of the general-dynamic TLS access
//! (`spec/mc-x64.md` "Thread-local storage").
//!
//! A linker relaxes a TLS access by recognizing its exact bytes and
//! overwriting them, so the general-dynamic pair has a fixed length of 16
//! bytes, padded with prefixes that change nothing: `data16 lea rdi,
//! [rip + x@tlsgd]` then `data16 data16 rex64 call __tls_get_addr@PLT`.
//! The other models are sequences of ordinary constructors.

use crate::fixup::Fixup;
use crate::inst::{Inst, Op, plain, rel, rmem};
use crate::reg::{Mem, Reg, Size};

/// `data16 lea rdi, [rip + disp32]` (`66 48 8D 3D cd`): the first half of
/// a general-dynamic access, with a zero field at offset 4 for
/// `R_X86_64_TLSGD`.
#[inline]
pub const fn tls_gd_lea() -> Inst {
    let op = Op { p66: true, ..Op::one(0x8d).sized(Size::Q, 0) };
    rmem(op, Size::Q, Reg::Rdi, Mem::rip(0))
}

/// `data16 data16 rex64 call rel32` (`66 66 48 E8 cd`): the second half,
/// with a zero field at offset 4 for `R_X86_64_PLT32` against
/// `__tls_get_addr`.
#[inline]
pub const fn tls_gd_call() -> Inst {
    // The second `66` goes where a mandatory prefix would: after the first
    // and before REX.
    let op = Op { p66: true, w: true, ..Op::one(0xe8).rep(0x66) };
    rel(plain(op), Fixup::Rel32, 0)
}
