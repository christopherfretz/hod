//! Sweeps for `spec/mc-x64.md` "Thread-local storage": the fixed-form
//! instructions of the general-dynamic access. Their relocations and
//! the linkers' relaxation of every model are tested in `fixups`.

use hod_mc_x64::*;

use crate::harness::Batch;

pub fn build(b: &mut Batch) {
    emit!(b, tls_gd_lea(), "data16 lea rdi, [rip]");
    // The target is the end of the 8-byte instruction: a zero field.
    emit!(b, tls_gd_call(), "data16 data16 rex64 call $HERE+8");
}
