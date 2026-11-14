//! Sweeps for `spec/mc-x64.md` "System and padding", "String
//! instructions".

use hod_mc_x64::*;

use crate::harness::Batch;
use crate::text::SIZES;

pub fn build(b: &mut Batch) {
    emit!(b, ud2(), "ud2");
    for disp in [0, 1, 5, 127, 128, 200, 255] {
        emit!(b, ud1_eax(disp), "ud1 eax, dword ptr [eax + {disp}]");
    }
    emit!(b, int3(), "int3");
    emit!(b, endbr64(), "endbr64");
    for len in 1..=15 {
        emit!(b, try_nop(len), ".nops {len}, {len}");
    }
    reject!(b, try_nop(0));
    reject!(b, try_nop(16));
    emit!(b, rdtsc(), "rdtsc");
    emit!(b, rdtscp(), "rdtscp");
    emit!(b, cpuid(), "cpuid");
    emit!(b, cld(), "cld");
    emit!(b, std(), "std");
    emit!(b, hlt(), "hlt");
    emit!(b, fwait(), "fwait");
    emit!(b, syscall(), "syscall");
    for n in [0, 1, 3, 4, 0x80, 0xff] {
        emit!(b, int_i(n), "int {n}");
    }
    for (size, suffix) in SIZES.into_iter().zip(['b', 'w', 'd', 'q']) {
        let (movs, stos) = (format!("rep movs{suffix}"), format!("rep stos{suffix}"));
        emit_as!(b, &movs, rep_movs(size), "{movs}");
        emit_as!(b, &stos, rep_stos(size), "{stos}");
    }
}
