//! Sweeps for `spec/mc-x64.md` "System and padding".

use hod_mc_x64::*;

use crate::harness::Batch;

pub fn build(b: &mut Batch) {
    emit!(b, ud2(), "ud2");
    emit!(b, int3(), "int3");
    emit!(b, endbr64(), "endbr64");
    for len in 1..=15 {
        emit!(b, try_nop(len), ".nops {len}, {len}");
    }
    reject!(b, try_nop(0));
    reject!(b, try_nop(16));
}
